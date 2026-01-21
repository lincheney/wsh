use anyhow::Result;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};

mod const_hash_map;
pub use const_hash_map::ConstHashMap;
#[macro_use]
pub mod strong_weak_wrapper;
#[macro_use]
pub mod impl_deref_helper;

pub fn set_nonblocking_fd<R: AsRawFd>(file: &R) -> Result<()> {
    let raw_fd = file.as_raw_fd();
    // 3. Set non-blocking mode
    let flags = nix::fcntl::fcntl(raw_fd, nix::fcntl::FcntlArg::F_GETFL)?;
    let new_flags = nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK;
    nix::fcntl::fcntl(raw_fd, nix::fcntl::FcntlArg::F_SETFL(new_flags))?;
    Ok(())
}

pub fn dup_fd(fd: BorrowedFd) -> std::io::Result<OwnedFd> {
    // behave like zsh_sys::movefd
    // why not use zsh_sys::movefd, because it stores info in fdtable
    // and we would have to clear it on drop
    // we need to store them for a bit to prevent them getting dropped
    const SLOT: Option<OwnedFd> = None;
    let mut fds = [SLOT; 10];
    let mut i = 0;
    let mut fd = fd.try_clone_to_owned()?;
    while fd.as_raw_fd() < 10 {
        fds[i] = Some(fd);
        fd = fds[i].as_ref().unwrap().try_clone()?;
        i += 1;
    }
    Ok(fd)
}
