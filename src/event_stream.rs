use anyhow::Result;
use std::os::fd::AsRawFd;
use std::io::Read;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::io::unix::AsyncFd;
use crate::keybind::parser;

#[derive(Debug)]
enum InputMessage {
    CursorPosition(oneshot::Sender<(usize, usize)>),
    Pause(oneshot::Sender<()>),
    Resume(oneshot::Sender<()>),
    Exit(i32, Option<oneshot::Sender<()>>),
}

#[derive(Clone)]
pub struct EventController {
    queue: mpsc::UnboundedSender<InputMessage>,
}

impl EventController {
    pub async fn pause(&mut self) {
        let (sender, receiver) = oneshot::channel();
        let _ = self.queue.send(InputMessage::Pause(sender));
        receiver.await.unwrap()
    }

    pub async fn resume(&mut self) {
        let (sender, receiver) = oneshot::channel();
        let _ = self.queue.send(InputMessage::Resume(sender));
        receiver.await.unwrap()
    }

    pub async fn get_cursor_position(&mut self) -> (usize, usize) {
        let (sender, receiver) = oneshot::channel();
        self.queue.send(InputMessage::CursorPosition(sender)).unwrap();
        receiver.await.unwrap()
    }

    pub async fn exit(&mut self, code: i32) {
        let (sender, receiver) = oneshot::channel();
        self.queue.send(InputMessage::Exit(code, Some(sender))).unwrap();
        receiver.await.unwrap()
    }
}

pub struct EventStream {
    queue: mpsc::UnboundedReceiver<InputMessage>,
}

impl EventStream {
    pub fn new() -> (Self, EventController) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            Self { queue: receiver },
            EventController { queue: sender },
        )
    }

    pub async fn run<T: Read+AsRawFd+Send+Sync+'static>(mut self, file: T, mut ui: crate::ui::Ui) -> anyhow::Result<i32> {
        let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
        let (pauser, mut paused) = watch::channel(false);
        let (position_sender, mut position_receiver) = mpsc::unbounded_channel::<oneshot::Sender<_>>();

        // read events
        let _x: tokio::task::JoinHandle<Result<()>> = {
            let mut reader = AsyncFd::new(file)?;
            let mut parser = parser::Parser::new();
            let mut paused = paused.clone();
            tokio::task::spawn(async move {
                let mut buf = [0; 1024];
                loop {
                    tokio::select!(

                        guard = reader.readable() => {
                            guard?.clear_ready();
                            match reader.get_mut().read(&mut buf) {
                                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => (),
                                Ok(0) => return Ok(()),
                                Err(err) => return Err(err)?,

                                Ok(n) => {
                                    parser.feed(&buf[..n]);
                                    log::debug!("DEBUG(bypass)\t{}\t= {:?}", stringify!(bstr::BString::from(&buf[..n])), bstr::BString::from(&buf[..n]));
                                    for (event, event_buffer) in parser.iter() {
                                        log::debug!("DEBUG(timmy) \t{}\t= {:?}", stringify!(event), event);
                                        match event {
                                            parser::Event::CursorPosition{x, y} => {
                                                log::debug!("DEBUG(damps) \t{}\t= {:?}", stringify!((x,y)), (x,y));
                                                if let Ok(sender) = position_receiver.try_recv() {
                                                    log::debug!("DEBUG(nadir) \t{}\t= {:?}", stringify!(sender), sender);
                                                    let _ = sender.send((x, y));
                                                }
                                            },
                                            _ => {
                                                event_sender.send((event, event_buffer))?;
                                            },
                                        }
                                    }
                                },
                            }
                        },

                        mut result = paused.changed() => loop {
                            log::debug!("DEBUG(roll)  \t{}\t= {:?}", stringify!(result), result);
                            match result {
                                Err(_) => return Ok(()),
                                Ok(()) => if !*paused.borrow_and_update() {
                                    log::debug!("DEBUG(pass)  \t{}\t= {:?}", stringify!("resulme"), "resulme");
                                    break;
                                },
                            }
                            result = paused.changed().await;
                        },

                    );
                }
            })
        };

        // process events
        let _x: tokio::task::JoinHandle<Result<()>> = tokio::task::spawn(async move {
            loop {
                tokio::select!(
                    item = event_receiver.recv() => {
                        let Some((event, event_buffer)) = item else { return Ok(()) };
                        if !ui.handle_event(event, event_buffer).await? {
                            return Ok(())
                        }
                    },

                    mut result = paused.changed() => loop {
                        match result {
                            Err(_) => return Ok(()),
                            Ok(()) => if !*paused.borrow_and_update() {
                                break;
                            },
                        }
                        result = paused.changed().await;
                    },
                );
            }
        });

        // read messages
        loop {
            let msg = self.queue.recv().await;
            log::debug!("DEBUG(domain)\t{}\t= {:?}", stringify!(msg), msg);
            let msg = msg.unwrap_or(InputMessage::Exit(0, None));
            match msg {
                InputMessage::CursorPosition(result) => {
                    position_sender.send(result)?;
                    crossterm::execute!(
                        std::io::stdout(),
                        crossterm::style::Print("\x1b[6n"),
                    )?;
                },
                InputMessage::Exit(code, result) => {
                    if let Some(result) = result {
                        let _ = result.send(());
                    }
                    return Ok(code)
                },
                InputMessage::Pause(result) => {
                    pauser.send(true)?;
                    let _ = result.send(());
                },
                InputMessage::Resume(result) => {
                    pauser.send(false)?;
                    let _ = result.send(());
                },
            }
        }

    }
}
