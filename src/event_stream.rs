use bstr::BString;
use anyhow::Result;
use std::os::fd::AsRawFd;
use std::io::Read;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::io::unix::AsyncFd;
use crate::keybind::parser;
use crate::pauser;

#[derive(Debug)]
enum Message {
    Event(parser::Event, BString),
    Exit(i32),
    Draw,
    WindowResize(u32, u32),
}

#[derive(Clone)]
pub struct EventController {
    queue: mpsc::UnboundedSender<Message>,
    pauser: Arc<pauser::Pauser>,
    position_queue: mpsc::UnboundedSender<oneshot::Sender<(usize, usize)>>,
}

impl EventController {
    pub fn pause(&self) {
        self.pauser.pause();
    }

    pub fn unpause(&self) {
        self.pauser.unpause();
    }

    pub fn get_cursor_position(&self) -> impl Future<Output=Result<(usize, usize)>> + use<> {
        // this returns an async block instead of being an async fn
        // so that you can await it without holding on to the &self reference
        let (sender, receiver) = oneshot::channel();
        let result = self.position_queue.send(sender);
        async move {
            result?;
            crossterm::execute!(
                std::io::stdout(),
                crossterm::style::Print("\x1b[6n"),
            )?;
            Ok(receiver.await?)
        }
    }

    pub fn queue_draw(&self) {
        let _ = self.queue.send(Message::Draw);
    }

    pub fn exit(&self, code: i32) {
        let _ = self.queue.send(Message::Exit(code));
    }
}

pub struct EventStream {
    queue: mpsc::UnboundedReceiver<Message>,
    queue_sender: mpsc::UnboundedSender<Message>,
    pausable: pauser::Pausable,
    position_queue: mpsc::UnboundedReceiver<oneshot::Sender<(usize, usize)>>,
}

impl EventStream {
    pub fn new() -> (Self, EventController) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (pauser, pausable) = pauser::new();
        let (position_sender, position_receiver) = mpsc::unbounded_channel();

        let stream = Self {
            queue: receiver,
            queue_sender: sender.clone(),
            pausable,
            position_queue: position_receiver,
        };
        let controller = EventController {
            queue: sender,
            pauser: Arc::new(pauser),
            position_queue: position_sender,
        };
        (stream, controller)
    }

    pub async fn run<T: Read+AsRawFd+Send+Sync+'static>(mut self, file: T, mut ui: crate::ui::Ui) -> anyhow::Result<i32> {

        // read events
        let mut reader = AsyncFd::new(file)?;
        let mut parser = parser::Parser::new();

        let queue_sender = self.queue_sender.clone();
        let mut pausable = self.pausable.clone();
        crate::spawn_and_log::<_, _, anyhow::Error>(async move {
            let mut buf = [0; 1024];
            loop {
                let Some(guard) = pausable.run(reader.readable()).await
                    else { continue };
                guard?.clear_ready();
                match reader.get_mut().read(&mut buf) {
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => (),
                    Ok(0) => return Ok(()),
                    Err(err) => return Err(err)?,

                    Ok(n) => {
                        parser.feed(&buf[..n]);
                        for (event, event_buffer) in parser.iter() {
                            match event {
                                parser::Event::CursorPosition{x, y} => {
                                    if let Ok(sender) = self.position_queue.try_recv() {
                                        let _ = sender.send((x, y));
                                    }
                                },
                                _ => {
                                    let _ = queue_sender.send(Message::Event(event, event_buffer));
                                },
                            }
                        }
                    },
                }
            }
        });

        let queue_sender = self.queue_sender.clone();
        let mut pausable = self.pausable.clone();
        crate::spawn_and_log::<_, _, anyhow::Error>(async move {
            let Some(mut window_size) = crate::shell::signals::sigwinch::get_subscriber()
                else { anyhow::bail!("cannot subscribe to window resize events"); };

            loop {
                match pausable.run(window_size.changed()).await {
                    None => continue,
                    Some(Err(_)) => return Ok(()),
                    Some(Ok(())) => {
                        let size = *window_size.borrow_and_update();
                        let _ = queue_sender.send(Message::WindowResize(size.0, size.1));
                    },
                }
            }
        });

        // process events
        loop {
            let Some(msg) = self.pausable.run(self.queue.recv()).await
                else { continue };
            match msg {
                Some(Message::Event(event, event_buffer)) => {
                    if !ui.handle_event(event, event_buffer).await? {
                        return Ok(0)
                    }
                },
                Some(Message::WindowResize(width, height)) => {
                    if !ui.handle_window_resize(width, height).await? {
                        return Ok(0)
                    }
                },
                Some(Message::Draw) => {
                    crate::log_if_err(ui.draw().await);
                },
                Some(Message::Exit(code)) => return Ok(code),
                None => return Ok(1),
            }
        }

    }

    pub fn spawn(self, ui: &crate::ui::Ui) {
        // spawn a task to take care of keyboard input
        let ui = ui.clone();
        crate::spawn_and_log(async move {
            let tty = std::fs::File::open("/dev/tty")?;
            crate::utils::set_nonblocking_fd(&tty)?;
            self.run(tty, ui).await
        });
    }
}
