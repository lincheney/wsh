use std::ptr::{NonNull};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{atomic::{AtomicI32, Ordering, AtomicPtr}};

pub type Pid = i32;

pub type PidSet = crate::utils::ConstHashMap<Pid, (Rc<AtomicI32>, bool)>;

pub struct PidTable {
    read: AtomicPtr<Rc<PidSet>>,
    write: RefCell<PidSet>,
}

pub struct WriteGuard<'a> {
    guard: std::cell::RefMut<'a, PidSet>,
    table: &'a PidTable,
}
impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        let mut pidset = PidSet::new();
        pidset.clone_from(&self.guard);
        let pidset = Rc::new(pidset);
        let ptr = Box::into_raw(Box::new(pidset));
        let old_ptr = self.table.read.swap(ptr, Ordering::AcqRel);
        drop(unsafe { Box::from_raw(old_ptr) });
    }
}
crate::impl_deref_helper!(self: WriteGuard<'a>, &self.guard => PidSet);
crate::impl_deref_helper!(mut self: WriteGuard<'a>, &mut self.guard => PidSet);

thread_local! {
    pub static PID_TABLE: PidTable = const{ PidTable {
        read: AtomicPtr::new(std::ptr::null_mut()),
        write: RefCell::new(PidSet::new()),
    } };
}

impl PidTable {
    pub fn get(&self) -> Option<Rc<PidSet>> {
        // loading and cloning can be separate
        // because self.read is only written to by get_mut()
        // which is never called in signal handlers
        let pidset = NonNull::new(self.read.load(Ordering::Acquire))?;
        Some(unsafe{ pidset.as_ref() }.clone())
    }

    pub fn get_mut(&self) -> WriteGuard<'_> {
        WriteGuard {
            guard: self.write.borrow_mut(),
            table: self,
        }
    }

    pub fn clear(&self) {
        self.get_mut().clear();
    }

    pub fn register_pid(&self, pid: Pid, add_to_jobtab: bool) {
        self.get_mut().insert(pid, (Rc::new(AtomicI32::new(-1)), add_to_jobtab));
    }

    pub fn deregister_pid(&self, pid: Pid) -> Option<bool> {
        self.get_mut().remove(&pid).map(|(_, add_to_jobtab)| add_to_jobtab)
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
