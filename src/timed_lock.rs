use tokio::time::timeout;
use std::time::Duration;

#[derive(Default)]
pub struct Mutex<T>(tokio::sync::Mutex<T>);
#[derive(Default)]
pub struct RwLock<T>(tokio::sync::RwLock<T>);

pub const DEFAULT_DURATION: Duration = Duration::from_millis(1000);

fn block_on<F: Future>(future: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.block_on(future)
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

}

impl<T> Mutex<T> {
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, T> {
        self.lock_within(DEFAULT_DURATION).await
    }

    pub async fn lock_within(&self, duration: Duration) -> tokio::sync::MutexGuard<'_, T> {
        timeout(duration, self.0.lock()).await.unwrap()
    }

    pub fn blocking_lock(&self) -> tokio::sync::MutexGuard<'_, T> {
        self.blocking_lock_within(DEFAULT_DURATION)
    }

    pub fn blocking_lock_within(&self, duration: Duration) -> tokio::sync::MutexGuard<'_, T> {
        block_on(self.lock_within(duration))
    }

    pub fn try_lock(&self) -> Result<tokio::sync::MutexGuard<'_, T>, tokio::sync::TryLockError> {
        self.0.try_lock()
    }
}

impl<T> RwLock<T> {
    pub fn new(inner: T) -> Self {
        Self(tokio::sync::RwLock::new(inner))
    }

    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, T> {
        self.read_within(DEFAULT_DURATION).await
    }

    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, T> {
        self.write_within(DEFAULT_DURATION).await
    }

    pub async fn read_within(&self, duration: Duration) -> tokio::sync::RwLockReadGuard<'_, T> {
        timeout(duration, self.0.read()).await.unwrap()
    }

    pub async fn write_within(&self, duration: Duration) -> tokio::sync::RwLockWriteGuard<'_, T> {
        timeout(duration, self.0.write()).await.unwrap()
    }

    pub fn blocking_write(&self) -> tokio::sync::RwLockWriteGuard<'_, T> {
        self.blocking_write_within(DEFAULT_DURATION)
    }

    pub fn blocking_write_within(&self, duration: Duration) -> tokio::sync::RwLockWriteGuard<'_, T> {
        block_on(self.write_within(duration))
    }
}
