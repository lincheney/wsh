use tokio::sync::{oneshot};

#[allow(dead_code)]
pub struct Canceller(oneshot::Sender<()>);
pub struct Cancellable(oneshot::Receiver<()>);

impl Cancellable {
    pub async fn run<T, F: Future<Output=T>>(&mut self, f: F) -> Option<T> {
        tokio::select!(
            result = f => Some(result),
            _ = &mut self.0 => None,
        )
    }
}

pub fn new() -> (Canceller, Cancellable) {
    let (sender, receiver) = oneshot::channel();
    (Canceller(sender), Cancellable(receiver))
}
