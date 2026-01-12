use anyhow::Result;
use std::os::fd::AsRawFd;
pub mod bounded_queue;
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
