use tokio::sync::{Notify, Mutex, MutexGuard};

#[derive(Default, Debug)]
pub struct PrintLock {
    lock: Mutex<usize>,
    notify: Notify,
}

impl PrintLock {

    pub fn try_lock(&self) -> Result<PrintLockGuard<'_>, tokio::sync::TryLockError> {
        Ok(PrintLockGuard{ inner: self.lock.try_lock()?, notify: &self.notify })
    }

    pub fn blocking_lock(&self) -> PrintLockGuard<'_> {
        PrintLockGuard{ inner: self.lock.blocking_lock(), notify: &self.notify }
    }

    pub async fn lock(&self) -> PrintLockGuard<'_> {
        PrintLockGuard{ inner: self.lock.lock().await, notify: &self.notify }
    }

    pub async fn lock_exclusive(&self) -> PrintLockGuard<'_> {
        let mut guard = self.lock().await;
        // this is just a condition
        while *guard.inner != 0 {
            drop(guard);
            self.notify.notified().await;
            guard = self.lock().await;
        }
        guard
    }

}

pub struct PrintLockGuard<'a> {
    inner: MutexGuard<'a, usize>,
    notify: &'a Notify,
}

impl PrintLockGuard<'_> {
    pub fn acquire(&mut self) {
        *self.inner += 1;
    }

    pub fn release(&mut self) {
        *self.inner -= 1;
        if *self.inner == 0 {
            self.notify.notify_waiters();
        }
    }

    pub fn get_value(&self) -> usize {
        *self.inner
    }
}
