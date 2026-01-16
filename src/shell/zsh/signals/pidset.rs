use std::ptr::null_mut;
use std::sync::{Arc, Mutex, MutexGuard, atomic::{AtomicI32, AtomicUsize, AtomicPtr, Ordering}};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, DefaultHasher};

pub type Pid = i32;

type PidHashMap = HashMap<Pid, (Arc<AtomicI32>, bool), BuildHasherDefault<DefaultHasher>>;

#[derive(Default)]
pub struct PidSet {
    inner: PidHashMap,
    borrows: AtomicUsize,
}
crate::impl_deref_helper!(self: PidSet, &self.inner => PidHashMap);
crate::impl_deref_helper!(mut self: PidSet, &mut self.inner => PidHashMap);

impl PidSet {
    fn borrow_exclusively(&self) -> bool {
        self.borrows.fetch_or(1, Ordering::SeqCst) <= 1
    }

    fn borrow(&self) {
        self.borrows.fetch_add(2, Ordering::SeqCst);
    }

    fn unborrow(&self) {
        self.borrows.fetch_sub(2, Ordering::AcqRel);
    }
}

pub struct PidTable {
    read: AtomicPtr<PidSet>,
    write: Mutex<(PidSet, Vec<PidSet>)>,
}

pub struct WriteGuard<'a> {
    guard: MutexGuard<'a, (PidSet, Vec<PidSet>)>,
    table: &'a PidTable,
}
impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        // free old sets; get the last one so that the iterator is consumed
        let available = self.guard.1.extract_if(.., |set| set.borrow_exclusively()).last().unwrap_or_default();

        let new = std::mem::replace(&mut self.guard.0, available);
        let new = Box::into_raw(Box::new(new));
        let old = self.table.read.swap(new, Ordering::SeqCst);

        if !old.is_null() {
            let old = unsafe{ Box::from_raw(old) };
            if !old.borrow_exclusively() {
                // borrowed, free it later
                self.guard.1.push(*old);
            } else if self.guard.0.inner.capacity() < old.inner.capacity() {
                // use old as it is bigger
                self.guard.0 = *old;
            }
        }

        // since i have the lock, its ok for me to just read from the raw ptr, nobody else can free it
        let new = unsafe{ &*new };
        self.guard.0.inner.clear();
        self.guard.0.inner.extend(new.inner.iter().map(|x| (*x.0, x.1.clone())));
        self.guard.0.borrows.store(0, Ordering::Release);
    }
}
crate::impl_deref_helper!(self: WriteGuard<'a>, &self.guard.0 => PidSet);
crate::impl_deref_helper!(mut self: WriteGuard<'a>, &mut self.guard.0 => PidSet);

pub struct ReadGuard<'a> {
    inner: &'a PidSet,
}
impl<'a> ReadGuard<'a> {
    fn new(inner: &'a PidSet) -> Self {
        inner.borrow();
        Self{ inner }
    }
}
impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        self.inner.unborrow();
    }
}
crate::impl_deref_helper!(self: ReadGuard<'a>, &self.inner => PidSet);

pub static PID_TABLE: PidTable = PidTable {
    read: AtomicPtr::new(null_mut()),
    write: Mutex::new((
        PidSet {
            inner: HashMap::with_hasher(BuildHasherDefault::new()),
            borrows: AtomicUsize::new(0),
        },
        Vec::new(),
    )),
};

impl PidTable {
    // this is lock free
    pub fn get(&self) -> Option<ReadGuard<'_>> {
        let ptr = self.read.load(Ordering::SeqCst);
        let pid_set = unsafe{ ptr.as_ref()? };
        Some(ReadGuard::new(pid_set))
    }

    pub fn get_mut(&self) -> WriteGuard<'_> {
        WriteGuard {
            guard: self.write.lock().unwrap(),
            table: self,
        }
    }

    pub fn register_pid(&self, pid: Pid, add_to_jobtab: bool) {
        self.get_mut().inner.insert(pid, (Arc::new(AtomicI32::new(-1)), add_to_jobtab));
    }

    pub fn extract_finished_pids<F: FnMut(Pid, i32)>(&self, mut callback: F) {
        self.get_mut().retain(|&pid, (status, _)| {
            let status = status.load(Ordering::Acquire);
            if status == -1 {
                true
            } else {
                callback(pid, status);
                false
            }
        })
    }
}
