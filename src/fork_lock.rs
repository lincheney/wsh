use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, Condvar, MutexGuard, atomic::{AtomicUsize, Ordering}};

pub struct RawForkLock {
    counter: AtomicUsize,
    mutex: Mutex<()>,
    condvar: Condvar,
}

pub struct RawForkLockReadGuard<'a> {
    parent: &'a RawForkLock,
}

impl Drop for RawForkLockReadGuard<'_> {
    fn drop(&mut self) {
        self.parent.remove_reader();
    }
}

pub struct RawForkLockWriteGuard<'a, 'b> {
    parent: &'a RawForkLock,
    _lock: MutexGuard<'b, ()>,
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
        if self.counter.fetch_sub(2, Ordering::AcqRel) == 1 {
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

        RawForkLockReadGuard{ parent: self }
    }

    pub fn write(&self) -> RawForkLockWriteGuard<'_, '_> {
        // always take the mutex first
        let mut lock = self.mutex.lock().unwrap();
        // indicate we want a write lock
        let value = self.counter.fetch_add(1, Ordering::AcqRel);

        if value > 1 {
            // there are readers in there first
            // wait until readers release the lock
            lock = self.condvar.wait_while(lock, |()| self.counter.load(Ordering::Acquire) > 1).unwrap();
        }

        RawForkLockWriteGuard{ parent: self, _lock: lock }
    }

    pub const fn wrap<T>(&self, inner: T) -> ForkLock<'_, T> {
        ForkLock{ lock: self, cell: UnsafeCell::new(inner) }
    }
}

pub struct ForkLock<'a, T> {
    lock: &'a RawForkLock,
    cell: UnsafeCell<T>,
}

unsafe impl<T> Sync for ForkLock<'_, T> {}

pub struct ForkLockReadGuard<'a, T> {
    _guard: RawForkLockReadGuard<'a>,
    _inner: &'a T,
}

pub struct ForkLockWriteGuard<'a, T> {
    _guard: RawForkLockReadGuard<'a>,
    _inner: &'a mut T,
}

impl<T> ForkLock<'_, T> {
    pub fn read(&self) -> ForkLockReadGuard<'_, T> {
        let guard = self.lock.read();
        ForkLockReadGuard{ _guard: guard, _inner: unsafe{ &*self.cell.get() } }
    }

    pub fn write(&self) -> ForkLockWriteGuard<'_, T> {
        let guard = self.lock.read();
        ForkLockWriteGuard{ _guard: guard, _inner: unsafe{ &mut *self.cell.get() } }
    }
}

impl<'a, T: Clone> Clone for ForkLock<'a, T> {
    fn clone(&self) -> Self {
        let clone = self.read().clone();
        Self {
            lock: self.lock,
            cell: UnsafeCell::new(clone),
        }
    }
}

impl<T> Deref for ForkLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self._inner
    }
}

impl<T> Deref for ForkLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self._inner
    }
}

impl<T> DerefMut for ForkLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self._inner
    }
}
