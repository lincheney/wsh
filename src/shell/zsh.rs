use nix::sys::signal;
use std::sync::{LazyLock};
use std::os::fd::{RawFd};
use std::os::raw::*;
use std::default::Default;
use std::ptr::null_mut;
use bstr::{BString, ByteSlice};

mod bindings;
mod linked_list;
mod builtin;
pub mod variables;
pub mod functions;
pub mod signals;
pub mod process;
mod widget;
#[macro_use]
mod meta_string;
pub mod zle_watch_fds;
pub use widget::ZleWidget;
pub mod history;
pub mod completion;
pub mod bin_zle;
pub mod parser;
pub(super) use bindings::*;
use variables::{Variable};
pub use meta_string::{MetaStr, MetaString};

pub static JOB: LazyLock<c_int> = LazyLock::new(|| unsafe{ zsh_sys::initjob() });

// pub type HandlerFunc = unsafe extern "C" fn(name: *mut c_char, argv: *mut *mut c_char, options: *mut zsh_sys::options, func: c_int) -> c_int;

pub fn opt_isset(opts: &zsh_sys::options, c: u8) -> bool {
    opts.ind[c as usize] != 0
}

pub fn shell_quote(string: &MetaStr) -> &MetaStr {
    unsafe {
        // allocated on the arena, so don't free it
        // TODO we should make an owned value
        MetaStr::from_ptr(zsh_sys::quotestring(string.as_ptr(), zsh_sys::QT_SINGLE_OPTIONAL as _))
    }
}

#[derive(Clone, Copy)]
pub struct ExecstringOpts<'a> {
    dont_change_job: bool,
    exiting: bool,
    context: Option<&'a MetaStr>,
}

impl Default for ExecstringOpts<'_> {
    fn default() -> Self {
        Self{ dont_change_job: true, exiting: false, context: None }
    }
}

pub fn execstring(cmd: &MetaStr, opts: ExecstringOpts) -> c_long {
    unsafe{
        zsh_sys::execstring(
            cmd.as_ptr().cast_mut(),
            opts.dont_change_job.into(),
            opts.exiting.into(),
            opts.context.map_or(null_mut(), |x| x.as_ptr().cast_mut()),
        );
    }
    get_return_code()
}

#[derive(Default, Clone, Copy)]
pub struct ZptyOpts {
    pub echo_input: bool,
    pub non_blocking: bool,
}

pub struct Zpty {
    pub pid: u32,
    pub fd: RawFd,
    pub name: MetaString,
}

pub fn zpty(name: MetaString, cmd: &MetaStr, opts: ZptyOpts) -> anyhow::Result<Zpty> {
    let silent = 0;
    if unsafe{ zsh_sys::require_module(c"zsh/zpty".as_ptr(), null_mut(), silent) } > 0 {
        anyhow::bail!("failed to load module zsh/zpty")
    }

    let mut cmd = shell_quote(cmd).to_owned();
    // reversed
    // add a read so that we have time to get the pid
    cmd.insert_str(0, meta_str!(c" '\\builtin read -k1;'"));
    cmd.insert_str(0, shell_quote(name.as_ref()));
    if opts.echo_input {
        cmd.insert_str(0, meta_str!(c"-e "));
    }
    if opts.non_blocking {
        cmd.insert_str(0, meta_str!(c"-b "));
    }
    cmd.insert_str(0, meta_str!(c"zpty "));

    unsafe {
        zsh_sys::startparamscope();
    }

    let code = execstring(cmd.as_ref(), Default::default());

    let result = (|| {
        if code > 0 {
            anyhow::bail!("zpty failed with code {code}")
        }

        // get fd from $REPLY
        let Some(mut fd) = variables::Variable::get(meta_str!(c"REPLY"))
            else { anyhow::bail!("could not get $REPLY") };

        let fd = if let Some(fd) = fd.try_as_int()? {
            fd
        } else if let Ok(fd) = std::str::from_utf8(&fd.as_bytes()) && let Ok(fd) = fd.parse() {
            fd
        } else {
            anyhow::bail!("could not get fd: {:?}", fd.as_value());
        };


        // how to get pid????
        // this seems yuck
        // why do i fork?
        // because zpty *insists* on making a read from the pty
        // this is broken even with normal zsh
        // (try do `zpty NAME sleep inf` and then `zpty -L`; this hangs because it never prints anything)
        // *however* if i fork the child proc can't read the pty and read will always fail immediately
        // bless
        // add a newline to help with parsing
        execstring(meta_str!(c"zpty_output=$'\\n'\"$(zpty & wait $!)\""), Default::default());
        let Some(mut output) = variables::Variable::get(meta_str!(c"zpty_output"))
            else { anyhow::bail!("could not get $zpty_output") };
        let output = output.as_bytes();

        // now we have to parse it
        let pid = output.find_iter("\n(")
            .find_map(|pos| {
                let name = name.to_bytes();
                // it looks like: (PID) NAME: ...
                let start = pos + 2;
                let end = start + output[start..].find(") ")?;
                if !output[start..end].iter().all(|x| x.is_ascii_digit()) {
                    return None
                }
                if ! output[end+2..].starts_with(name) {
                    return None
                }
                if ! output[end+2+name.len()..].starts_with(b": ") {
                    return None
                }
                Some(&output[start..end])
            });
        let Some(pid) = pid else { anyhow::bail!("could not get pid") };
        let pid = std::str::from_utf8(pid)?.parse()?;
        add_pid(pid as _);

        // tell the zpty to start
        let fd = fd as _;
        let borrowed = unsafe{ std::os::fd::BorrowedFd::borrow_raw(fd) };
        while nix::unistd::write(borrowed, b"\n")? != 1 { }

        Ok(Zpty{
            pid,
            fd,
            name,
        })

    })();

    unsafe {
        zsh_sys::endparamscope();
    }
    result
}

pub fn add_pid(pid: i32) {
    unsafe{
        let aux = 1;
        let bgtime = null_mut(); // this can be NULL if aux is 1
        let oldjob = zsh_sys::thisjob;
        zsh_sys::thisjob = *JOB;
        zsh_sys::addproc(pid, null_mut(), aux, bgtime, -1, -1);
        zsh_sys::thisjob = oldjob;
    }
}

pub fn find_process_status(pid: i32, pop_if_done: bool) -> Option<c_int> {
     unsafe{
        let job = zsh_sys::jobtab.add(*JOB as usize);
        let mut prev: *mut zsh_sys::process = null_mut();
        let mut proc = (*job).auxprocs;
        while let Some(p) = proc.as_ref() {
            // found it
            if p.pid == pid {
                let status = p.status;
                if pop_if_done && status >= 0 {
                    if prev.is_null() {
                        (*job).auxprocs = p.next;
                    } else {
                        (*prev).next = p.next;
                    }
                    zsh_sys::zfree(proc.cast(), std::mem::size_of::<zsh_sys::process>() as _);
                }
                return Some(status);
            }
            prev = proc;
            proc = p.next;
        }
    }
    None
}

pub fn get_return_code() -> c_long {
    unsafe{ zsh_sys::lastval }
}

pub fn get_prompt(prompt: Option<&MetaStr>, escaped: bool) -> Option<MetaString> {
    let mut var;
    let prompt = if let Some(prompt) = prompt {
        prompt
    } else {
        var = variables::Variable::get(meta_str!(c"PROMPT"))?;
        var.as_meta_bytes()?
    };

    // The prompt used for spelling correction.  The sequence `%R' expands to the string which presumably needs  spelling  correction,  and
    // `%r' expands to the proposed correction.  All other prompt escapes are also allowed.
    let r = null_mut();
    #[allow(non_snake_case)]
    let R = null_mut();
    let glitch = escaped.into();
    unsafe {
        let ptr = zsh_sys::promptexpand(prompt.as_ptr().cast_mut(), glitch, r, R, null_mut());
        let str = MetaStr::from_ptr(ptr).to_owned();
        zsh_sys::zsfree(ptr);
        Some(str)
    }
}

pub fn get_prompt_size(prompt: &MetaStr, term_width: Option<c_long>) -> (c_int, c_int) {
    let mut width = 0;
    let mut height = 0;
    let overflow = 0;
    let mut old_term_width = None;
    unsafe {
        if let Some(term_width) = term_width && term_width != zsh_sys::zterm_columns {
            old_term_width = Some(zsh_sys::zterm_columns);
            zsh_sys::zterm_columns = term_width;
        }

        zsh_sys::countprompt(prompt.as_ptr().cast_mut(), &raw mut width, &raw mut height, overflow);

        if let Some(old_term_width) = old_term_width {
            zsh_sys::zterm_columns = old_term_width;
        }
    }
    (width, height)
}

pub fn start_zle_scope() {
    unsafe {
        zsh_sys::startparamscope();
        bindings::makezleparams(0);
        zsh_sys::startparamscope();
    }
}

pub fn end_zle_scope() {
    unsafe {
        zsh_sys::endparamscope();
        zsh_sys::endparamscope();
    }
}

pub fn set_zle_buffer(buffer: BString, cursor: i64) {
    start_zle_scope();
    Variable::set(meta_str!(c"BUFFER"), buffer.into(), true).unwrap();
    Variable::set(meta_str!(c"CURSOR"), cursor.into(), true).unwrap();
    end_zle_scope();
}

pub fn get_zle_buffer() -> (BString, Option<i64>) {
    start_zle_scope();
    let buffer = Variable::get(meta_str!(c"BUFFER")).unwrap().as_bytes();
    let cursor = Variable::get(meta_str!(c"CURSOR")).unwrap().try_as_int();
    end_zle_scope();
    match cursor {
        Ok(Some(cursor)) => (buffer, Some(cursor)),
        _ => (buffer, None),
    }
}

pub enum ErrorVerbosity {
    Normal = 0,
    Quiet = 1,
    Ignore = 2,
}

pub fn set_error_verbosity(verbosity: ErrorVerbosity) -> ErrorVerbosity {
    unsafe {
        let old_value = zsh_sys::noerrs;
        zsh_sys::noerrs = verbosity as _;
        if old_value <= 0 {
            ErrorVerbosity::Normal
        } else if old_value >= 2 {
            ErrorVerbosity::Ignore
        } else {
            ErrorVerbosity::Quiet
        }
    }
}

pub fn capture_shout<T, F: FnOnce() -> T>(
    sink: &mut super::file_stream::Sink,
    f: F,
) -> (BString, T) {

    unsafe {
        let old_trashedzle = bindings::trashedzle;
        bindings::trashedzle = 1;
        let result = {
            sink.clear().unwrap();
            let _file = sink.override_shout();
            f()
        };
        bindings::trashedzle = old_trashedzle;

        // read the data out
        let buffer = sink.read().unwrap();

        (buffer, result)
    }
}

pub fn queue_signals() {
    unsafe {
        zsh_sys::queueing_enabled += 1;
    }
}

pub fn unqueue_signals() -> nix::Result<()> {
    const MAX_QUEUE_SIZE: i32 = 128;

    unsafe {
        zsh_sys::queueing_enabled -= 1;
        if zsh_sys::queueing_enabled == 0 {
            // run_queued_signals
            while zsh_sys::queue_front != zsh_sys::queue_rear { /* while signals in queue */
                zsh_sys::queue_front = (zsh_sys::queue_front + 1) % MAX_QUEUE_SIZE;
                let sigset = zsh_sys::signal_mask_queue[zsh_sys::queue_front as usize];
                let sigset = signal::SigSet::from_sigset_t_unchecked(std::mem::transmute::<zsh_sys::__sigset_t, nix::libc::sigset_t>(sigset));
                let mut oset = signal::SigSet::empty();
                signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&sigset), Some(&mut oset))?;
                zsh_sys::zhandler(zsh_sys::signal_queue[zsh_sys::queue_front as usize]);
                signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&oset), None)?;
            }
        }
    }
    Ok(())
}

pub fn winch_block() {
    unsafe {
        zsh_sys::signal_block(zsh_sys::signal_mask(signal::Signal::SIGWINCH as _));
    }
}

pub fn winch_unblock() {
    unsafe {
        zsh_sys::signal_unblock(zsh_sys::signal_mask(signal::Signal::SIGWINCH as _));
    }
}

pub fn zistype(x: c_char, y: c_short) -> bool {
    unsafe {
        zsh_sys::typtab[x as usize] & y > 0
    }
}

pub fn call_hook_func<'a, I: Iterator<Item=&'a MetaStr>>(name: &'a MetaStr, args: I) -> Option<c_int> {
    unsafe {
        // needs metafy
        if zsh_sys::getshfunc(name.as_ptr().cast_mut()).is_null() {
            let mut name = name.to_owned();
            name.push_str(MetaStr::from_bytes(zsh_sys::HOOK_SUFFIX));
            // check if it exists
            Variable::get(name.as_ref())?;
        }
    }

    let args = std::iter::once(name.as_ptr())
        .chain(args.map(|x| x.as_ptr()));

    // convert args to a linked list
    let args = linked_list::LinkedList::new_from_ptrs(args);

    let mut list = args.as_linkroot();
    unsafe {
        Some(zsh_sys::callhookfunc(name.as_ptr().cast_mut(), &raw mut list, 1, null_mut()))
    }
}
