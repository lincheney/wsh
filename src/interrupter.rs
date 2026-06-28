use std::rc::Rc;
use anyhow::Result;
use tokio::sync::{Notify};

pub struct Interruptable(Rc<Notify>);

impl Interruptable {
    pub async fn run<T, F: Future<Output=T>>(&mut self, f: F) -> Option<T> {
        tokio::select!(
            result = f => Some(result),
            () = self.0.notified() => None,
        )
    }
}

pub fn new() -> Result<Interruptable> {
    let Some(sigint) = crate::shell::signals::sigint::get_subscriber().and_then(|x| x.upgrade())
        else {
            anyhow::bail!("cannot subscribe to sigint events");
        };
    Ok(Interruptable(sigint))
}

pub async fn run<T, F: Future<Output=T>>(f: F) -> Result<Option<T>> {
    Ok(new()?.run(f).await)
}
