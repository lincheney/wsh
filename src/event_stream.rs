use std::sync::Arc;
use std::default::Default;
use futures::channel::mpsc;
use futures::{select, SinkExt, StreamExt, FutureExt};
use tokio::sync::{Mutex, MutexGuard, OwnedMutexGuard, RwLock};

#[derive(Default)]
struct Lock {
    inner: Arc<Mutex<UnlockedEvents>>,
    outer: RwLock<()>,
}

impl Lock {
    async fn lock_exclusive(&self) -> MutexGuard<'_, UnlockedEvents> {
        let _outer = self.outer.write().await;
        self.inner.lock().await
    }
}

pub struct EventStream {
    lock: Arc<Lock>,
    receiver: mpsc::UnboundedReceiver<()>,
}

#[derive(Default)]
pub struct UnlockedEvents{
    exit: Option<i32>,
}

impl UnlockedEvents {
    pub fn get_cursor_position(&self) -> Result<(u16, u16), std::io::Error> {
        loop {
            // let now = std::time::SystemTime::now();
            match crossterm::cursor::position() {
                // Err(e) if now.elapsed().unwrap().as_millis() < 1500 && format!("{}", e) == "The cursor position could not be read within a normal duration" => {
                    // // crossterm times out in 2s
                    // // but it also fails on EINTR whereas we would like to retry
                    // log::debug!("{:?}", e);
                // },
                x => return x,
            }
        }
    }

    pub fn exit(&mut self, code: i32) {
        self.exit = Some(code);
    }
}

pub struct EventLocker {
    lock: Arc<Lock>,
    sender: mpsc::UnboundedSender<()>,
}

impl EventLocker {
    pub async fn lock(&mut self) -> MutexGuard<'_, UnlockedEvents> {
        let _outer = self.lock.outer.read().await;
        if let Ok(lock) = self.lock.inner.try_lock() {
            return lock;
        }
        self.sender.send(()).await.unwrap();
        self.lock.inner.lock().await
    }

    pub async fn lock_owned(&mut self) -> OwnedMutexGuard<UnlockedEvents> {
        let _outer = self.lock.outer.read().await;
        let inner = self.lock.inner.clone();
        if let Ok(lock) = inner.clone().try_lock_owned() {
            return lock;
        }
        self.sender.send(()).await.unwrap();
        inner.lock_owned().await
    }

}

impl EventStream {
    pub fn new() -> (Self, EventLocker) {
        let (sender, receiver) = mpsc::unbounded::<()>();
        let lock: Arc<Lock> = Arc::new(Default::default());
        let stream = Self{ lock: lock.clone(), receiver };
        let locker = EventLocker{ lock, sender };
        (stream, locker)
    }

    pub async fn run(&mut self, ui: &mut crate::ui::Ui) -> anyhow::Result<i32> {
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
                if let Some(exit_code) = lock.as_ref().unwrap().exit {
                    return Ok(exit_code)
                }

                select! {
                    _ = waker => {
                        break;
                    },
                    e = event => {
                        drop(lock.take());

                        match e {
                            Some(Ok(event)) => {
                                if !ui.handle_event(event).await? {
                                    return Ok(0)
                                }
                            }
                            Some(Err(event)) => { println!("Error: {:?}\r", event); },
                            None => return Ok(0),
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
