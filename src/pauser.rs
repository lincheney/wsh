use tokio::sync::{watch};

#[allow(dead_code)]
pub struct Pauser(watch::Sender<bool>);
#[derive(Clone)]
pub struct Pausable(watch::Receiver<bool>, bool);

impl Pauser {
    pub fn pause(&self) {
        let _ = self.0.send(true);
    }
    pub fn unpause(&self) {
        let _ = self.0.send(false);
    }
}

impl Pausable {
    pub async fn run<T, F: Future<Output=T> >(&mut self, f: F) -> Option<T> {
        tokio::select!(
            result = f => return Some(result),
            mut pause = self.0.changed(), if !self.1 => {
                // loop until unpaused
                loop {
                    self.1 = pause.is_err();
                    if self.1 || !*self.0.borrow_and_update() {
                        return None
                    }
                    pause = self.0.changed().await;
                }
            }
        )
    }
}

pub fn new() -> (Pauser, Pausable) {
    let (sender, receiver) = watch::channel(false);
    (Pauser(sender), Pausable(receiver, false))
}
