use std::ops::{Deref, DerefMut};
use std::cell::UnsafeCell;
use std::sync::{Mutex, Condvar, MutexGuard, atomic::{AtomicUsize, Ordering}};

#[cfg(debug_assertions)]
pub mod tracing {
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub static MAP: Mutex<Option<HashMap<usize, String>>> = Mutex::new(None);

    pub struct TraceKey(usize);

    impl TraceKey {
        pub fn new() -> Self {
            let mut map = MAP.lock().unwrap();
            let map = map.get_or_insert_default();
            let key = map.len();
            map.insert(key, std::backtrace::Backtrace::force_capture().to_string());
            Self(key)
        }
    }

    impl Drop for TraceKey {
        fn drop(&mut self) {
            let mut map = MAP.lock().unwrap();
            map.as_mut().unwrap().remove(&self.0);
        }
    }

    pub fn debug() {
        if let Some(map) = &*MAP.lock().unwrap() {
            for v in map.values() {
                ::log::debug!("DEBUG(lesson)\t{}\t= {}", stringify!(v), v);
            }
        }
    }
}

pub struct RawForkLock {
    counter: AtomicUsize,
    mutex: Mutex<()>,
    condvar: Condvar,
}

pub struct RawForkLockReadGuard<'a> {
    parent: &'a RawForkLock,
    #[cfg(debug_assertions)]
    _trace_key: tracing::TraceKey,
}

impl Drop for RawForkLockReadGuard<'_> {
    fn drop(&mut self) {
        self.parent.remove_reader();
    }
}

pub struct RawForkLockWriteGuard<'a: 'b, 'b> {
    parent: &'a RawForkLock,
    #[allow(dead_code)]
    lock: MutexGuard<'b, ()>,
}

impl RawForkLockWriteGuard<'_, '_> {
    pub fn reset(&self) {
        self.parent.counter.store(1, Ordering::Relaxed);
    }
}

impl Drop for RawForkLockWriteGuard<'_, '_> {
    fn drop(&mut self) {
        self.parent.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

impl RawForkLock {
    pub const fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
            mutex: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }

    fn has_writer(&self) -> bool {
        self.counter.load(Ordering::Acquire) % 2 == 1
    }

    fn remove_reader(&self) {
        if self.counter.fetch_sub(2, Ordering::AcqRel) == 3 {
            self.condvar.notify_all();
        }
    }

    fn read(&self) -> RawForkLockReadGuard<'_> {
        // indicate we want a read lock
        let value = self.counter.fetch_add(2, Ordering::AcqRel);

        if value % 2 == 1 {
            // the writer got there first
            let mut lock = self.mutex.lock().unwrap();
            if self.has_writer() {
                // remove ourselves while we wait
                self.remove_reader();
                // wait until writer releases the lock
                lock = self.condvar.wait_while(lock, |()| self.has_writer()).unwrap();
                // add ourselves back in
                self.counter.fetch_add(2, Ordering::AcqRel);
            }
            drop(lock);
        }

        RawForkLockReadGuard{
            parent: self,
            _trace_key: tracing::TraceKey::new(),
        }
    }

    pub fn write(&self) -> RawForkLockWriteGuard<'_, '_> {
        // tracing::debug();
        // always take the mutex first
        let mut lock = self.mutex.lock().unwrap();
        // indicate we want a write lock
        let value = self.counter.fetch_add(1, Ordering::AcqRel);

        if value > 1 {
            // there are readers in there first
            // wait until readers release the lock
            lock = self.condvar.wait_while(lock, |()| self.counter.load(Ordering::Acquire) > 1).unwrap();
        }

        RawForkLockWriteGuard{ parent: self, lock }
    }

    pub const fn wrap<T>(&self, inner: T) -> ForkLock<'_, T> {
        ForkLock{ lock: self, inner: UnsafeCell::new(inner) }
    }
}

pub struct ForkLock<'a, T> {
    lock: &'a RawForkLock,
    inner: UnsafeCell<T>,
}

unsafe impl<T> Sync for ForkLock<'_, T> {}

pub struct ForkLockReadGuard<'a, T> {
    #[allow(dead_code)]
    guard: RawForkLockReadGuard<'a>,
    inner: &'a T,
}

pub struct ForkLockWriteGuard<'a, 'b, T> {
    #[allow(dead_code)]
    guard: RawForkLockWriteGuard<'a, 'b>,
    inner: &'b mut T,
}

impl<'a, T> ForkLock<'a, T> {
    pub fn read(&self) -> ForkLockReadGuard<'_, T> {
        let guard = self.lock.read();
        ForkLockReadGuard{ guard, inner: unsafe{ &*self.inner.get() } }
    }

    pub fn read_with_lock(&self, lock: &'a RawForkLockWriteGuard) -> &T {
        assert!(std::ptr::eq(self.lock, lock.parent));
        unsafe{ &*self.inner.get() }
    }

    pub fn write(&self) -> ForkLockWriteGuard<'_, '_, T> {
        let guard = self.lock.write();
        ForkLockWriteGuard{ guard, inner: unsafe{ &mut *self.inner.get() } }
    }
}

impl<T> Deref for ForkLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<T> Deref for ForkLockWriteGuard<'_, '_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<T> DerefMut for ForkLockWriteGuard<'_, '_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner
    }
}
