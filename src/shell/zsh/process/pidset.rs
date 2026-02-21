use std::sync::{Arc, RwLock, RwLockReadGuard, Mutex, MutexGuard, atomic::{AtomicI32, Ordering}};

pub type Pid = i32;

pub type PidSet = crate::utils::ConstHashMap<Pid, (Arc<AtomicI32>, bool)>;

pub struct PidTable {
    read: RwLock<PidSet>,
    write: Mutex<PidSet>,
}

pub struct WriteGuard<'a> {
    guard: MutexGuard<'a, PidSet>,
    table: &'a PidTable,
}
impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        let mut pidset = self.table.read.write().unwrap();
        pidset.clone_from(&*self.guard);
    }
}
crate::impl_deref_helper!(self: WriteGuard<'a>, &self.guard => PidSet);
crate::impl_deref_helper!(mut self: WriteGuard<'a>, &mut self.guard => PidSet);

pub static PID_TABLE: PidTable = PidTable {
    // RwLock is re-entrant for read() so it is ok to use in signal handlers
    read: RwLock::new(PidSet::new()),
    write: Mutex::new(PidSet::new()),
};

impl PidTable {
    pub fn get(&self) -> RwLockReadGuard<'_, PidSet> {
        self.read.read().unwrap()
    }

    pub fn get_mut(&self) -> WriteGuard<'_> {
        // this should never be run from the "main/signal-handling" thread
        WriteGuard {
            guard: self.write.lock().unwrap(),
            table: self,
        }
    }

    pub fn register_pid(&self, pid: Pid, add_to_jobtab: bool) {
        self.get_mut().insert(pid, (Arc::new(AtomicI32::new(-1)), add_to_jobtab));
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
        });
    }
}
