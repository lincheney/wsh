use std::sync::{OnceLock, Mutex, Arc};
use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::ptr::null_mut;
use std::default::Default;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use async_std::sync::Mutex as AsyncMutex;
use futures::Stream;
use super::bindings;

pub struct StreamConsumer {
    index: usize,
    parent: Arc<Mutex<Streamer>>,
}

impl Stream for StreamConsumer {
    type Item = Arc<bindings::cmatch>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let (cmatch, finished) = {
            let parent = self.parent.lock().unwrap();
            (parent.matches.get(self.index).cloned(), parent.finished)
        };

        if let Some(cmatch) = cmatch {
            self.index += 1;
            Poll::Ready(Some(cmatch))
        } else if finished {
            Poll::Ready(None)
        } else {
            let mut parent = self.parent.lock().unwrap();
            parent.wakers.push(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[derive(Debug)]
struct Streamer {
    buffer: String,
    finished: bool,
    matches: Vec<Arc<bindings::cmatch>>,
    wakers: Vec<Waker>,
}
unsafe impl Send for Streamer {}

impl Streamer {
    fn make_consumer(parent: &Arc<Mutex<Self>>) -> Arc<AsyncMutex<StreamConsumer>> {
        let consumer = Arc::new(AsyncMutex::new(StreamConsumer {
            index: 0,
            parent: parent.clone(),
        }));
        consumer
    }

    fn wake(&mut self) {
        for waker in self.wakers.drain(..) {
            waker.wake()
        }
    }
}

#[derive(Debug)]
struct CompaddState {
    original: zsh_sys::Builtin,
    streamer: Option<Arc<Mutex<Streamer>>>,
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
        let g = CStr::from_ptr((*bindings::amatches).name);
        // eprintln!("DEBUG(dachas)\t{}\t= {:?}\r", stringify!(g), g);
    }

    if !bindings::matches.is_null() {
        let mut node = (*bindings::matches).list.first;
        let iter = std::iter::from_fn(|| {
            while !node.is_null() {
                let dat = (*node).dat as *mut bindings::cmatch;
                node = (*node).next;
                if !dat.is_null() {
                    let dat = Arc::new((*dat).clone());
                    return Some(dat)
                }
            }

            None
        });
        let len = streamer.matches.len();
        streamer.matches.extend(iter.skip(len));
        streamer.wake();
            // eprintln!("DEBUG(pucks) \t{}\t= {:?}\r", stringify!(node), (std::ffi::CStr::from_ptr((*dat).str_), (*dat).gnum));
        // }
    }

    return result
}

pub fn override_compadd() {
    super::execstring("zmodload zsh/complete", Default::default());

    if super::get_return_code() == 0 {
        let mut compadd = COMPADD_STATE.get_or_init(|| Mutex::new(Default::default())).lock().unwrap();
        compadd.original = super::pop_builtin("compadd").unwrap();

        let mut compadd = unsafe{ *compadd.original }.clone();
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
pub fn get_completions(line: &str) -> anyhow::Result<Arc<AsyncMutex<StreamConsumer>>> {
    if let Some(compadd) = COMPADD_STATE.get() {
        let consumer = {
            let mut compadd = compadd.lock().unwrap();
            if let Some(streamer) = compadd.streamer.as_ref().filter(|s| s.lock().unwrap().buffer == line) {
                return Ok(Streamer::make_consumer(&streamer))
            }
            let streamer = Arc::new(Mutex::new(Streamer {
                buffer: line.to_owned(),
                finished: false,
                matches: vec![],
                wakers: vec![],
            }));
            let consumer = Streamer::make_consumer(&streamer);
            compadd.streamer = Some(streamer);
            consumer
        };

        unsafe {
            // set the zle buffer
            zsh_sys::startparamscope();
            bindings::makezleparams(0);
            super::Variable::set("BUFFER", line).unwrap();
            super::Variable::set("CURSOR", &format!("{}", line.len() + 1)).unwrap();
            zsh_sys::endparamscope();

            // this is kinda what completecall() does
            let cfargs: [*mut c_char; 1] = [null_mut()];
            bindings::cfargs = cfargs.as_ptr() as _;
            bindings::compfunc = COMPFUNC.as_ptr() as *mut _;
            // zsh will switch up the pgid if monitor and interactive are set
            super::execstring("set +o monitor", Default::default());
            bindings::menucomplete(null_mut());
            super::execstring("set -o monitor", Default::default());
        }

        if let Some(streamer) = compadd.lock().unwrap().streamer.as_ref() {
            streamer.lock().unwrap().finished = true;
        }

        Ok(consumer)

    } else {
        Err(anyhow::anyhow!("ui is not running"))
    }
}

pub fn clear_cache() {
    if let Some(compadd) = COMPADD_STATE.get() {
        compadd.lock().unwrap().streamer = None;
    }
}
