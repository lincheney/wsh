use std::marker::PhantomData;
use std::cell::UnsafeCell;
use std::sync::{Mutex, Condvar, MutexGuard, atomic::{AtomicUsize, Ordering}};

#[cfg(debug_assertions)]
pub mod tracing {
    use std::collections::HashMap;
    use std::sync::{Mutex};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    pub static MAP: Mutex<Option<HashMap<usize, String>>> = Mutex::new(None);

    pub struct TraceKey(usize);

    impl TraceKey {
        pub fn new() -> Self {
            let key = COUNTER.fetch_add(1, Ordering::AcqRel);
            let mut map = MAP.lock().unwrap();
            let map = map.get_or_insert_default();
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
                let result = self.condvar.wait_timeout_while(
                    lock,
                    crate::timed_lock::DEFAULT_DURATION,
                    |()| self.has_writer(),
                ).unwrap();

                if result.1.timed_out() {
                    panic!("timed out waiting for fork lock read");
                }

                lock = result.0;

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
            let result = self.condvar.wait_timeout_while(
                lock,
                crate::timed_lock::DEFAULT_DURATION,
                |()| self.counter.load(Ordering::Acquire) > 1,
            ).unwrap();

            if result.1.timed_out() {
                tracing::debug();
                panic!("timed out waiting for fork lock write");
            }

            lock = result.0;
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
    _phantom: PhantomData<*const usize>,
}
crate::impl_deref_helper!(self: ForkLockReadGuard<'a, T>, self.inner => T);

impl<'a, T> ForkLock<'a, T> {
    pub fn read(&self) -> ForkLockReadGuard<'_, T> {
        let guard = self.lock.read();
        ForkLockReadGuard{ guard, inner: unsafe{ &*self.inner.get() }, _phantom: PhantomData }
    }

    pub fn read_with_lock(&self, lock: &'a RawForkLockWriteGuard) -> &T {
        assert!(std::ptr::eq(self.lock, lock.parent));
        unsafe{ &*self.inner.get() }
    }
}
