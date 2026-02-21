use anyhow::Result;
use nix::sys::signal;
use std::os::raw::{c_int};
use std::sync::atomic::{AtomicI32, Ordering};
use tokio::io::AsyncReadExt;
pub mod sigwinch;

// how the heck does this work
//
// i can't just wrap zhandler in the sigchld handler
// ... because zsh might not actually do any waitpid() !
// this is due to queueing
// the handler will actually be run at some arbitrary point
// additionally statuses for any pids not already in the jobtab are dropped
//
// what does this do
// we extend the signal trap array to accommodate some fake signals
// we install a trap on a fake signal
// we install a sigchld handler that redirects it to our fake signal
// zhandler() has no special handling for the fake signal so all it does is run our trap
// either immediately or even later when it is queued
// either way we know when our trap is run, it is being run for real (not queued)
// before zhandler() we stick any additional pids into the jobtab that we need
// and after zhandler() we loop over the jobtab for any statuses that have updated

const SIGTRAPPED_COUNT: c_int = 1024;
static TRAP_QUEUING_ENABLED: AtomicI32 = AtomicI32::new(0);

fn convert_to_custom_signal(sig: c_int) -> c_int {
    sig + SIGTRAPPED_COUNT / 2
}

fn convert_from_custom_signal(sig: c_int) -> c_int {
    sig - SIGTRAPPED_COUNT / 2
}

extern "C" fn sighandler(sig: c_int) {
    // this *should* run in the main thread
    #[allow(static_mut_refs)]
    unsafe {
        // allow our trap to run
        // is this safe? trap queuing is only used for pid waiting
        // we just run a builtin so should be ok

        let trap_queueing_enabled = zsh_sys::trap_queueing_enabled;
        if trap_queueing_enabled > 0 {
            TRAP_QUEUING_ENABLED.store(trap_queueing_enabled, Ordering::Release);
            zsh_sys::trap_queueing_enabled = 0;
        }
        // this should call our trap
        zsh_sys::zhandler(convert_to_custom_signal(sig));
        zsh_sys::trap_queueing_enabled = trap_queueing_enabled;
    }
}

pub fn invoke_signal_handler(arg: Option<&[u8]>) -> c_int {
    let Some(arg) = arg
        else { return 1 };
    let Ok(arg) = std::str::from_utf8(arg)
        else { return 1 };
    let Ok(signal) = arg.parse::<c_int>()
        else { return 1 };
    let signal = convert_from_custom_signal(signal);

    #[allow(static_mut_refs)]
    unsafe {
        debug_assert_eq!(zsh_sys::queueing_enabled, 0);
        zsh_sys::trap_queueing_enabled = TRAP_QUEUING_ENABLED.load(Ordering::Acquire);
    }

    match signal.try_into() {
        Ok(signal::Signal::SIGCHLD) => super::process::sighandler(),
        Ok(signal::Signal::SIGWINCH) => sigwinch::sighandler(),
        _ => 1, // unknown
    }
}

fn resize_array<T: Copy + Default>(dst: &mut *mut T, old_len: usize, new_len: usize) {
    let mut new = vec![T::default(); new_len];
    new[..old_len].copy_from_slice(unsafe{ std::slice::from_raw_parts(*dst, old_len) });
    *dst = Box::into_raw(new.into_boxed_slice()).cast();
}

pub(super) fn hook_signal(signal: signal::Signal) -> Result<()> {
    unsafe {
        // set the sighandler
        let handler = signal::SigHandler::Handler(sighandler);
        let action = signal::SigAction::new(handler, signal::SaFlags::empty(), signal::SigSet::empty());
        signal::sigaction(signal, &action)?;

        // now set the trap
        let signal = convert_to_custom_signal(signal as _);
        let script: super::MetaString = format!("\\builtin wsh .invoke-signal-handler {signal}").into();
        let func = super::functions::Function::new(script.as_ref())?;
        let eprog = func.0.as_ref().funcdef;
        (&mut *eprog).nref += 1;
        zsh_sys::settrap(signal, eprog, zsh_sys::ZSIG_TRAPPED as _);
    }

    Ok(())
}

pub(super) fn self_pipe<C, T, E>(callback: C) -> Result<std::io::PipeWriter>
where
    C: Fn() -> Result<T, E> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let (reader, writer) = std::io::pipe()?;
    crate::utils::set_nonblocking_fd(&writer)?;
    crate::utils::set_nonblocking_fd(&reader)?;

    // spawn a reader task
    let mut reader = tokio::net::unix::pipe::Receiver::from_owned_fd(reader.into())?;
    crate::spawn_and_log::<_, (), anyhow::Error>(async move {
        let mut buf = [0];
        loop {
            reader.read_exact(&mut buf).await?;
            callback()?;
        }
    });

    Ok(writer)
}

pub fn init(ui: &crate::ui::Ui) -> Result<()> {
    #[allow(static_mut_refs)]
    unsafe {
        let trapcount = (zsh_sys::SIGCOUNT + 3 + nix::libc::SIGRTMAX() as u32 - nix::libc::SIGRTMIN() as u32 + 1) as usize;

        // make extra space so we can stuff our own "custom" signals
        debug_assert!(SIGTRAPPED_COUNT as usize > trapcount * 2);
        resize_array(&mut super::sigtrapped, trapcount, SIGTRAPPED_COUNT as usize);
        debug_assert!(SIGTRAPPED_COUNT as usize > trapcount * 2);
        resize_array(&mut super::siglists, trapcount, SIGTRAPPED_COUNT as usize);
    }

    super::process::init(ui)?;
    sigwinch::init()?;

    Ok(())
}
