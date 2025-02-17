use std::future::Future;
use std::sync::Arc;
use futures::channel::mpsc;
use futures::{select, SinkExt, StreamExt, FutureExt};
use async_std::sync::{Mutex, MutexGuard, RwLock};

struct Lock {
    inner: Mutex<UnlockedEvents>,
    outer: RwLock<()>,
}

impl Lock {
    async fn lock_exclusive(&self) -> MutexGuard<UnlockedEvents> {
        let _outer = self.outer.write().await;
        self.inner.lock().await
    }
}

pub struct EventStream {
    lock: Arc<Lock>,
    receiver: mpsc::UnboundedReceiver<()>,
}

pub struct UnlockedEvents();

impl UnlockedEvents {
    pub fn get_cursor_position(&self) -> Result<(u16, u16), std::io::Error> {
        loop {
            let now = std::time::SystemTime::now();
            match crossterm::cursor::position() {
                Err(e) if now.elapsed().unwrap().as_millis() < 1500 && format!("{}", e) == "The cursor position could not be read within a normal duration" => {
                    // crossterm times out in 2s
                    // but it also fails on EINTR whereas we would like to retry
                },
                x => return x,
            }
        }
    }
}

pub struct EventLocker {
    lock: Arc<Lock>,
    sender: mpsc::UnboundedSender<()>,
}

impl EventLocker {
    pub async fn lock(&mut self) -> MutexGuard<UnlockedEvents> {
        let _outer = self.lock.outer.read().await;
        if let Some(lock) = self.lock.inner.try_lock() {
            return lock;
        }
        self.sender.send(()).await.unwrap();
        self.lock.inner.lock().await
    }

    pub async fn get_cursor_position(&mut self) -> Result<(u16, u16), std::io::Error> {
        self.lock().await.get_cursor_position()
    }
}

impl EventStream {
    pub fn new() -> (Self, EventLocker) {
        let (sender, receiver) = mpsc::unbounded::<()>();
        let lock = Arc::new(Lock{
            inner: Mutex::new(UnlockedEvents()),
            outer: RwLock::new(()),
        });
        let stream = Self{ lock: lock.clone(), receiver };
        let locker = EventLocker{ lock, sender };
        (stream, locker)
    }

    pub async fn run(&mut self, ui: &mut crate::ui::Ui, shell: &crate::shell::Shell) -> anyhow::Result<()> {
        loop {
            let mut lock = None;
            let mut waker = self.receiver.next().fuse();

            let mut events = crossterm::event::EventStream::new();
            let mut event = events.next().fuse();

            // keep looping over events until woken up
            loop {
                if lock.is_none() {
                    // get an exclusive lock
                    lock = Some(self.lock.lock_exclusive().await);
                }

                select! {
                    _ = waker => {
                        break;
                    },
                    e = event => {
                        drop(lock.take());

                        match e {
                            Some(Ok(event)) => {
                                if !ui.handle_event(event, &shell).await? {
                                    return Ok(())
                                }
                            }
                            Some(Err(event)) => { println!("Error: {:?}\r", event); },
                            None => return Ok(()),
                        }
                        event = events.next().fuse();
                        lock = Some(self.lock.lock_exclusive().await);
                    }
                };
            };

            drop(events);
            drop(lock);
        }
    }
}
