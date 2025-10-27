use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
#[macro_use]
pub mod strong_weak_wrapper;

pub type ArcMutex<T> = Arc<Mutex<T>>;
pub type AsyncArcMutex<T> = Arc<AsyncMutex<T>>;

macro_rules! ArcMutexNew {
    ($expr:expr) => (
        ::std::sync::Arc::new(::std::sync::Mutex::new($expr))
    )
}

macro_rules! AsyncArcMutexNew {
    ($expr:expr) => (
        ::std::sync::Arc::new(::tokio::sync::Mutex::new($expr))
    )
}

pub(crate) use ArcMutexNew;
pub(crate) use AsyncArcMutexNew;
