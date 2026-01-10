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

    pub async fn get_cursor_position(&self) -> Result<(usize, usize)> {
        let (sender, receiver) = oneshot::channel();
        self.position_queue.send(sender)?;
        crossterm::execute!(
            std::io::stdout(),
            crossterm::style::Print("\x1b[6n"),
        )?;
        Ok(receiver.await?)
    }

    pub fn queue_draw(&self) {
        let _ = self.queue.send(Message::Draw);
    }

    pub async fn exit(&self, code: i32) {
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
        let _x: tokio::task::JoinHandle<Result<()>> = {
            let mut reader = AsyncFd::new(file)?;
            let mut parser = parser::Parser::new();
            let mut pausable = self.pausable.clone();

            tokio::task::spawn(async move {
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
                                        let _ = self.queue_sender.send(Message::Event(event, event_buffer));
                                    },
                                }
                            }
                        },
                    }
                }
            })
        };

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
                Some(Message::Draw) => {
                    ui.try_draw().await;
                },
                Some(Message::Exit(code)) => return Ok(code),
                None => return Ok(1),
            }
        }

    }

    pub fn spawn(self, ui: &crate::ui::Ui) {
        // spawn a task to take care of keyboard input
        let ui = ui.clone();
        tokio::task::spawn(async move {
            let tty = std::fs::File::open("/dev/tty").unwrap();
            crate::utils::set_nonblocking_fd(&tty).unwrap();
            self.run(tty, ui).await.unwrap();
        });
    }
}
