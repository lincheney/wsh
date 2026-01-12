use nix::sys::signal;

pub fn disable_all_signals() -> nix::Result<()> {
    let mask = signal::SigSet::all();
    signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&mask), None)
}
