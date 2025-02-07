use std::sync::{OnceLock, Mutex};
use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::ptr::null_mut;
use std::default::Default;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use futures::Stream;
use super::bindings;

pub struct StreamConsumer {
    waker: Option<Waker>,
    index: usize,
    parent: Rc<RefCell<Streamer>>,
}

impl Stream for StreamConsumer {
    type Item = *mut bindings::cmatch;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let cmatch = self.parent.borrow().matches.get(self.index).copied();
        if let Some(cmatch) = cmatch {
            self.index += 1;
            Poll::Ready(Some(cmatch))
        } else if self.parent.borrow().finished {
            Poll::Ready(None)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[derive(Debug)]
struct Streamer {
    buffer: String,
    finished: bool,
    matches: Vec<*mut bindings::cmatch>,
    children: Vec<Weak<RefCell<StreamConsumer>>>,
}

impl Streamer {
    fn make_consumer(parent: &Rc<RefCell<Self>>) -> Rc<RefCell<StreamConsumer>> {
        let consumer = Rc::new(RefCell::new(StreamConsumer {
            waker: None,
            index: 0,
            parent: parent.clone(),
        }));
        parent.borrow_mut().children.push(Rc::downgrade(&consumer));
        consumer
    }

    fn wake_children(&mut self) {
        self.children.retain(|c| {
            if let Some(c) = c.upgrade() {
                if let Some(waker) = c.borrow_mut().waker.take() {
                    waker.wake_by_ref();
                }
                true
            } else {
                false
            }
        });
    }
}

#[derive(Debug)]
struct CompaddState {
    original: zsh_sys::Builtin,
    streamer: Option<Rc<RefCell<Streamer>>>,
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
        streamer.borrow_mut()
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
            if node.is_null() {
                None
            } else {
                let dat = (*node).dat as *mut bindings::cmatch;
                node = (*node).next;
                Some(dat)
            }
        });
        let len = streamer.matches.len();
        streamer.matches.extend(iter.skip(len));
        streamer.wake_children();
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
pub fn get_completions(line: &str) -> anyhow::Result<Rc<RefCell<StreamConsumer>>> {
    if let Some(compadd) = COMPADD_STATE.get() {
        let consumer = {
            let mut compadd = compadd.lock().unwrap();
            if let Some(streamer) = compadd.streamer.as_ref().filter(|s| s.borrow().buffer == line) {
                return Ok(Streamer::make_consumer(&streamer))
            }
            let streamer = Rc::new(RefCell::new(Streamer {
                buffer: line.to_owned(),
                finished: false,
                matches: vec![],
                children: vec![],
            }));
            let consumer = Streamer::make_consumer(&streamer);
            compadd.streamer = Some(streamer);
            consumer
        };

        unsafe {
            // set the zle buffer
            zsh_sys::startparamscope();
            bindings::makezleparams(0);
            super::Variable::set("BUFFER", line);
            super::Variable::set("CURSOR", &format!("{}", line.len() + 1));
            zsh_sys::endparamscope();

            // this is kinda what completecall() does
            let cfargs: [*mut c_char; 1] = [null_mut()];
            bindings::cfargs = cfargs.as_ptr() as _;
            bindings::compfunc = COMPFUNC.as_ptr() as *mut _;
            bindings::menucomplete(null_mut());
        }
        consumer.borrow().parent.borrow_mut().finished = true;

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
