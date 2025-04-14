use std::sync::{OnceLock, Mutex, Arc};
use std::ffi::{CString};
use std::os::raw::*;
use std::ptr::null_mut;
use std::default::Default;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tokio::sync::Mutex as AsyncMutex;
use bstr::{BStr, BString};
use super::bindings;
use crate::utils::*;

pub struct WaitForChunk<'a> {
    consumer: &'a mut StreamConsumer,
}

pub struct StreamConsumer {
    index: usize,
    parent: ArcMutex<Streamer>,
}

impl StreamConsumer {
    pub async fn chunks(&mut self) -> Option<impl Iterator<Item=Arc<bindings::cmatch>> + use<'_>> {
        if (WaitForChunk{ consumer: self }).await {
            Some(std::iter::from_fn(move || {
                let parent = self.parent.lock().unwrap();
                let result = parent.matches.get(self.index).cloned();
                if result.is_some() {
                    self.index += 1;
                }
                result
            }))
        } else {
            None
        }
    }
}

impl std::future::Future for WaitForChunk<'_> {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut parent = self.consumer.parent.lock().unwrap();
        if parent.matches.len() > self.consumer.index {
            drop(parent);
            Poll::Ready(true)
        } else if parent.finished {
            Poll::Ready(false)
        } else {
            parent.wakers.push(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[derive(Debug)]
pub struct Streamer {
    buffer: BString,
    pub(crate) completion_word_len: usize,
    finished: bool,
    matches: Vec<Arc<bindings::cmatch>>,
    wakers: Vec<Waker>,
    thread: Option<nix::sys::pthread::Pthread>,
}
unsafe impl Send for Streamer {}

impl Streamer {
    fn make_consumer(parent: &ArcMutex<Self>) -> Arc<AsyncMutex<StreamConsumer>> {
        AsyncArcMutexNew!(StreamConsumer {
            index: 0,
            parent: parent.clone(),
        })
    }

    fn wake(&mut self) {
        for waker in self.wakers.drain(..) {
            waker.wake()
        }
    }

    pub fn cancel(&self) -> anyhow::Result<()> {
        if !self.finished {
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(0), nix::sys::signal::Signal::SIGINT)?;

            // if let Some(pid) = self.thread {
                // nix::sys::pthread::pthread_kill(pid, nix::sys::signal::Signal::SIGTERM)?;
            // }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CompaddState {
    original: zsh_sys::Builtin,
    streamer: Option<ArcMutex<Streamer>>,
}

static COMPFUNC: &[u8] = b"_main_complete\0";

impl Default for CompaddState {
    fn default() -> Self {
        CompaddState{
            original: null_mut(),
            streamer: None,
        }
    }
}

unsafe impl Send for CompaddState {}
static COMPADD_STATE: OnceLock<Mutex<CompaddState>> = OnceLock::new();

unsafe extern "C" fn compadd_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    // eprintln!("DEBUG(bombay)\t{}\t= {:?}\r", stringify!(nam), nam);

    let compadd = COMPADD_STATE.get().unwrap().lock().unwrap();
    let result = (*compadd.original).handlerfunc.unwrap()(nam, argv, options, func);

    let mut streamer = if let Some(streamer) = compadd.streamer.as_ref() {
        streamer.lock().unwrap()
    } else {
        return result
    };

    if !bindings::amatches.is_null() && !(*bindings::amatches).name.is_null() {
        // let g = CStr::from_ptr((*bindings::amatches).name);
        // eprintln!("DEBUG(dachas)\t{}\t= {:?}\r", stringify!(g), g);
    }

    if !bindings::matches.is_null() {
        let len = streamer.matches.len();
        let iter = super::iter_linked_list(bindings::matches)
            .filter_map(|ptr| {
                let ptr = ptr as *mut bindings::cmatch;
                if ptr.is_null() {
                    None
                } else {
                    Some(Arc::new((*ptr).clone()))
                }
            }).skip(len);
        streamer.matches.extend(iter);
        streamer.completion_word_len = (zsh_sys::we - zsh_sys::wb).max(0) as usize;
        streamer.wake();
            // eprintln!("DEBUG(pucks) \t{}\t= {:?}\r", stringify!(node), (std::ffi::CStr::from_ptr((*dat).str_), (*dat).gnum));
        // }
    }

    result
}

pub fn override_compadd() {
    super::execstring("zmodload zsh/complete", Default::default());

    if super::get_return_code() == 0 {
        let mut compadd = COMPADD_STATE.get_or_init(|| Mutex::new(Default::default())).lock().unwrap();
        compadd.original = super::pop_builtin("compadd").unwrap();

        let mut compadd = unsafe{ *compadd.original };
        compadd.handlerfunc = Some(compadd_handlerfunc);
        compadd.node = zsh_sys::hashnode{
            next: null_mut(),
            nam: CString::new("compadd").unwrap().into_raw(),
            flags: 0,
        };
        super::add_builtin("compadd", Box::into_raw(Box::new(compadd)));

    }
}

pub fn restore_compadd() {
    if let Some(compadd) = COMPADD_STATE.get() {
        let mut compadd = compadd.lock().unwrap();
        if !compadd.original.is_null() {
            super::add_builtin("compadd", compadd.original);
            compadd.original = null_mut();
        }

    }
}

// ookkkk
// zsh completion is intimately tied to zle
// so there's no "low-level" function to hook into
// the best we can do is emulate completecall()
pub fn get_completions(line: &BStr) -> anyhow::Result<(AsyncArcMutex<StreamConsumer>, ArcMutex<Streamer>)> {
    if let Some(compadd) = COMPADD_STATE.get() {
        let (producer, consumer) = {
            let mut compadd = compadd.lock().unwrap();
            // if let Some(streamer) = compadd.streamer.as_ref().filter(|s| s.lock().unwrap().buffer == line) {
                // return Ok(Streamer::make_consumer(&streamer))
            // }
            let producer = ArcMutexNew!(Streamer {
                buffer: line.to_owned(),
                completion_word_len: 0,
                finished: false,
                matches: vec![],
                wakers: vec![],
                thread: None,
            });
            let consumer = Streamer::make_consumer(&producer);
            compadd.streamer = Some(producer.clone());
            (producer, consumer)
        };

        Ok((consumer, producer))

    } else {
        Err(anyhow::anyhow!("ui is not running"))
    }
}

pub fn _get_completions(streamer: &Mutex<Streamer>) {
    streamer.lock().unwrap().thread = Some(nix::sys::pthread::pthread_self());

    {
        let line = &streamer.lock().unwrap().buffer;
        super::set_zle_buffer(line.clone(), line.len() as i64 + 1);
    }

    unsafe {
        // this is kinda what completecall() does
        let cfargs: [*mut c_char; 1] = [null_mut()];
        bindings::cfargs = cfargs.as_ptr() as _;
        bindings::compfunc = COMPFUNC.as_ptr() as *mut _;
        // zsh will switch up the pgid if monitor and interactive are set
        super::execstring("set +o monitor", Default::default());
        bindings::menucomplete(null_mut());
        // soft exit menu completion
        bindings::minfo.cur = null_mut();
        super::execstring("set -o monitor", Default::default());
    }

    let mut streamer = streamer.lock().unwrap();
    streamer.finished = true;
    streamer.wake();
}

pub fn clear_cache() {
    if let Some(compadd) = COMPADD_STATE.get() {
        compadd.lock().unwrap().streamer = None;
    }
}

pub fn insert_completion(line: &BStr, completion_word_len: usize, m: &bindings::cmatch) -> (BString, usize) {
    unsafe {
        // set the zle buffer
        super::set_zle_buffer(line.into(), line.len() as i64 + 1);

        // set start and end of word being completed
        zsh_sys::we = line.len() as i32;
        zsh_sys::wb = zsh_sys::we - completion_word_len as i32;

        bindings::metafy_line();
        bindings::do_single(m as *const _ as *mut _);
        bindings::unmetafy_line();

        zsh_sys::startparamscope();
        bindings::makezleparams(0);
        let buffer = super::Variable::get("BUFFER").unwrap().as_bytes();
        let cursor = super::Variable::get("CURSOR").unwrap().as_bytes();
        zsh_sys::endparamscope();

        let cursor = std::str::from_utf8(&cursor).ok().and_then(|s| s.parse().ok()).unwrap_or(buffer.len());
        (buffer, cursor)
    }
}
