use std::cell::{Cell, RefCell};

pub type Pid = i32;

type PidSet = crate::utils::ConstHashMap<Pid, (Cell<i32>, bool)>;
type Result<T> = std::result::Result<T, std::thread::AccessError>;

pub struct PidTable {
    inner: RefCell<PidSet>,
}

thread_local! {
    pub static PID_TABLE: PidTable = const { PidTable {
        inner: RefCell::new(PidSet::new()),
    } };
}

impl PidTable {
    pub fn try_with<T, F: FnOnce(&mut PidSet) -> T>(f: F) -> Result<T> {
        crate::shell::zsh::queue_signals();
        let result = PID_TABLE.try_with(|p| f(&mut *p.inner.borrow_mut()));
        crate::shell::zsh::unqueue_signals().unwrap();
        result
    }

    pub fn clear() -> Result<()> {
        Self::try_with(|p| p.clear())
    }

    pub fn register_pid(pid: Pid, add_to_jobtab: bool) ->Result<()> {
        Self::try_with(|p| {
            p.insert(pid, (Cell::new(-1), add_to_jobtab));
        })
    }

    pub fn deregister_pid(pid: Pid) -> Result<Option<bool>> {
        Self::try_with(|p| p.remove(&pid).map(|(_, add_to_jobtab)| add_to_jobtab))
    }

    pub fn extract_finished_pids<F: FnMut(Pid, i32)>(mut callback: F) -> Result<()> {
        Self::try_with(|p| {
            p.retain(|&pid, (status, _)| {
                let status = status.get();
                if status == -1 {
                    true
                } else {
                    callback(pid, status);
                    false
                }
            });
        })
    }

}
