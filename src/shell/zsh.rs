use nix::sys::signal;
use std::sync::{LazyLock};
use std::os::fd::{RawFd};
use std::io::Read;
use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::default::Default;
use std::ptr::null_mut;
use bstr::{BStr, BString, ByteSlice, ByteVec};

mod string;
mod bindings;
mod linked_list;
pub mod variables;
pub mod functions;
mod widget;
pub use widget::ZleWidget;
pub mod history;
pub mod completion;
pub mod parser;
pub use string::ZString;
pub(crate) use bindings::*;
use variables::{Variable};

pub static JOB: LazyLock<c_int> = LazyLock::new(|| unsafe{ zsh_sys::initjob() });

// pub type HandlerFunc = unsafe extern "C" fn(name: *mut c_char, argv: *mut *mut c_char, options: *mut zsh_sys::options, func: c_int) -> c_int;

pub fn shell_quote(string: &CStr) -> BString {
    unsafe {
        CStr::from_ptr(zsh_sys::quotestring(string.as_ptr(), zsh_sys::QT_SINGLE_OPTIONAL as _)).to_bytes().into()
    }
}

#[derive(Clone, Copy)]
pub struct ExecstringOpts<'a> {
    dont_change_job: bool,
    exiting: bool,
    context: Option<&'a str>,
}

impl Default for ExecstringOpts<'_> {
    fn default() -> Self {
        Self{ dont_change_job: true, exiting: false, context: None }
    }
}

pub fn execstring<S: AsRef<BStr>>(cmd: S, opts: ExecstringOpts) -> c_long {
    let cmd = cmd.as_ref().to_vec();
    let context = opts.context.map(|c| ZString::from(c).into_raw());
    unsafe{
        zsh_sys::execstring(
            metafy(&cmd),
            opts.dont_change_job.into(),
            opts.exiting.into(),
            context.unwrap_or(null_mut()),
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
    pub name: CString,
}

pub fn zpty(name: CString, cmd: &CStr, opts: ZptyOpts) -> anyhow::Result<Zpty> {
    let mut cmd = shell_quote(cmd);

    let silent = 0;
    if unsafe{ zsh_sys::require_module(c"zsh/zpty".as_ptr(), null_mut(), silent) } > 0 {
        anyhow::bail!("failed to load module zsh/zpty")
    }

    // reversed
    // add a read so that we have time to get the pid
    cmd.insert_str(0, " '\\builtin read -k1;'");
    cmd.insert_str(0, shell_quote(&name).as_bytes());
    if opts.echo_input {
        cmd.insert_str(0, "-e ");
    }
    if opts.non_blocking {
        cmd.insert_str(0, "-b ");
    }
    cmd.insert_str(0, "zpty ");

    unsafe {
        zsh_sys::startparamscope();
    }

    let code = execstring(cmd, Default::default());

    let result = (|| {
        if code > 0 {
            anyhow::bail!("zpty failed with code {code}")
        }

        // get fd from $REPLY
        let Some(mut fd) = variables::Variable::get("REPLY")
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
        execstring("zpty_output=$'\\n'\"$(zpty & wait $!)\"", Default::default());
        let Some(mut output) = variables::Variable::get("zpty_output")
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
            fd,
            pid,
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

pub fn get_return_code() -> c_long {
    unsafe{ zsh_sys::lastval }
}

pub fn pop_builtin(name: &str) -> Option<zsh_sys::Builtin> {
    let name = CString::new(name).unwrap();
    let ptr = unsafe { zsh_sys::removehashnode(zsh_sys::builtintab, name.as_ptr().cast()) };
    if ptr.is_null() { None } else { Some(ptr.cast()) }
}

pub fn add_builtin(cmd: &str, builtin: zsh_sys::Builtin) {
    let cmd: ZString = cmd.into();
    unsafe { zsh_sys::addhashnode(zsh_sys::builtintab, cmd.into_raw(), builtin.cast()) };
}

pub fn get_prompt(prompt: Option<&BStr>, escaped: bool) -> Option<CString> {
    let prompt = if let Some(prompt) = prompt {
        CString::new(prompt.to_vec()).unwrap()
    } else {
        let prompt = variables::Variable::get("PROMPT")?.as_bytes();
        CString::new(prompt).unwrap()
    };

    // The prompt used for spelling correction.  The sequence `%R' expands to the string which presumably needs  spelling  correction,  and
    // `%r' expands to the proposed correction.  All other prompt escapes are also allowed.
    let r = null_mut();
    #[allow(non_snake_case)]
    let R = null_mut();
    let glitch = escaped.into();
    unsafe {
        let ptr = zsh_sys::promptexpand(prompt.as_ptr().cast_mut(), glitch, r, R, null_mut());
        Some(CString::from_raw(ptr))
    }
}

pub fn get_prompt_size(prompt: &CStr) -> (c_int, c_int) {
    let mut width = 0;
    let mut height = 0;
    let overflow = 0;
    unsafe {
        zsh_sys::countprompt(prompt.as_ptr().cast_mut(), &raw mut width, &raw mut height, overflow);
    }
    (width, height)
}

pub fn metafy(value: &[u8]) -> *mut c_char {
    unsafe {
        if value.is_empty() {
            // make an empty string on the arena
            let ptr = zsh_sys::zhalloc(1).cast();
            *ptr = 0;
            ptr
        } else {
            // metafy will ALWAYS write a terminating null no matter what
            zsh_sys::metafy(value.as_ptr() as _, value.len() as _, zsh_sys::META_HEAPDUP as _)
        }
    }
}

pub fn unmetafy<'a>(ptr: *mut u8) -> &'a [u8] {
    // threadsafe!
    let mut len = 0i32;
    unsafe {
        zsh_sys::unmetafy(ptr.cast(), &raw mut len);
        std::slice::from_raw_parts(ptr, len as _)
    }
}

pub fn unmetafy_owned(value: &mut Vec<u8>) {
    // threadsafe!
    let mut len = 0i32;
    // MUST end with null byte
    if value.last().is_none_or(|c| *c != 0) {
        value.push(0);
    }
    unsafe {
        zsh_sys::unmetafy(value.as_mut_ptr().cast(), &raw mut len);
    }
    value.truncate(len as _);
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
    Variable::set(b"BUFFER", buffer.into(), true).unwrap();
    Variable::set(b"CURSOR", cursor.into(), true).unwrap();
    end_zle_scope();
}

pub fn get_zle_buffer() -> (BString, Option<i64>) {
    start_zle_scope();
    let buffer = Variable::get("BUFFER").unwrap().as_bytes();
    let cursor = Variable::get("CURSOR").unwrap().try_as_int();
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
    reader: &mut std::io::PipeReader,
    writer: std::ptr::NonNull<nix::libc::FILE>,
    f: F,
) -> (BString, T) {

    let result;
    unsafe {
        let old_shout = zsh_sys::shout;
        let old_trashedzle = bindings::trashedzle;
        zsh_sys::shout = writer.as_ptr().cast();
        bindings::trashedzle = 1;
        result = f();
        nix::libc::fflush(zsh_sys::shout.cast());
        bindings::trashedzle = old_trashedzle;
        zsh_sys::shout = old_shout;
    }

    let mut buffer = BString::new(vec![]);
    let mut buf = [0; 1024];
    while let Ok(n) = reader.read(&mut buf) {
        let buf = &buf[..n];
        buffer.extend(if n < buf.len() {
            // probably the end so trim it
            buf.trim_end()
        } else {
            buf
        });
    }
    (buffer, result)
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
                let sigset = signal::SigSet::from_sigset_t_unchecked(std::mem::transmute(sigset));
                let mut oset = signal::SigSet::empty();
                signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&sigset), Some(&mut oset))?;
                zsh_sys::zhandler(zsh_sys::signal_queue[zsh_sys::queue_front as usize]);
                signal::sigprocmask(signal::SigmaskHow::SIG_SETMASK, Some(&oset), None)?;
            }
        }
    }
    Ok(())
}

pub fn zistype(x: c_char, y: c_short) -> bool {
    unsafe {
        zsh_sys::typtab[x as usize] & y > 0
    }
}
