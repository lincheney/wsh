use crate::unsafe_send::UnsafeSend;
use std::collections::HashSet;
use std::ffi::CStr;
use tokio::sync::{mpsc};
use anyhow::Result;
use std::sync::{Mutex};
use std::os::raw::*;
use std::ptr::{null_mut};
use std::default::Default;
use bstr::{BString};
use super::{bindings, builtin::Builtin};

pub struct Match {
    inner: UnsafeSend<bindings::cmatch>,
    // length of word being completed
    completion_word_len: usize,
    nbrbeg: i32,
    nbrend: i32,
}

impl Match {
    pub fn new(inner: &bindings::cmatch) -> Self {
        unsafe {
            let brpl: Vec<_> = (0..bindings::nbrbeg).map(|i| *inner.brpl.offset(i as _)).collect();
            let brsl: Vec<_> = (0..bindings::nbrend).map(|i| *inner.brsl.offset(i as _)).collect();

            let inner = bindings::cmatch{
                str_: zsh_sys::ztrdup(inner.str_),
                orig: zsh_sys::ztrdup(inner.orig),
                ipre: zsh_sys::ztrdup(inner.ipre),
                ripre: zsh_sys::ztrdup(inner.ripre),
                isuf: zsh_sys::ztrdup(inner.isuf),
                ppre: zsh_sys::ztrdup(inner.ppre),
                psuf: zsh_sys::ztrdup(inner.psuf),
                prpre: zsh_sys::ztrdup(inner.prpre),
                pre: zsh_sys::ztrdup(inner.pre),
                suf: zsh_sys::ztrdup(inner.suf),
                disp: zsh_sys::ztrdup(inner.disp),
                autoq: zsh_sys::ztrdup(inner.autoq),
                rems: zsh_sys::ztrdup(inner.rems),
                remf: zsh_sys::ztrdup(inner.remf),
                brpl: if brpl.is_empty() {
                    null_mut()
                } else {
                    Box::into_raw(brpl.into_boxed_slice()).cast()
                },
                brsl: if brsl.is_empty() {
                    null_mut()
                } else {
                    Box::into_raw(brsl.into_boxed_slice()).cast()
                },
                ..*inner
            };

            Self {
                inner: UnsafeSend::new(inner),
                completion_word_len: (zsh_sys::we - zsh_sys::wb).max(0) as usize,
                nbrbeg: super::nbrbeg,
                nbrend: super::nbrend,
            }
        }
    }

    pub fn get_orig(&self) -> Option<&CStr> {
        self.inner.as_ref().get_orig()
    }
}

impl Drop for Match {
    fn drop(&mut self) {
        unsafe {
            let inner = self.inner.as_ref();
            zsh_sys::zsfree(inner.str_);
            zsh_sys::zsfree(inner.orig);
            zsh_sys::zsfree(inner.ipre);
            zsh_sys::zsfree(inner.ripre);
            zsh_sys::zsfree(inner.isuf);
            zsh_sys::zsfree(inner.ppre);
            zsh_sys::zsfree(inner.psuf);
            zsh_sys::zsfree(inner.prpre);
            zsh_sys::zsfree(inner.pre);
            zsh_sys::zsfree(inner.suf);
            zsh_sys::zsfree(inner.disp);
            zsh_sys::zsfree(inner.autoq);
            zsh_sys::zsfree(inner.rems);
            zsh_sys::zsfree(inner.remf);
            if !inner.brpl.is_null() {
                drop(Box::from_raw(inner.brpl));
            }
            if !inner.brsl.is_null() {
                drop(Box::from_raw(inner.brsl));
            }
        }
    }
}

#[derive(Default)]
struct CompaddState {
    // original compadd function
    original: UnsafeSend<Option<Builtin>>,
    // sink to send matches
    sink: Option<mpsc::UnboundedSender<Vec<Match>>>,
    // matches we have already seen
    seen: UnsafeSend<HashSet<*const bindings::cmatch>>,
}

impl CompaddState {
    fn reset(&mut self) {
        self.sink.take();
        self.seen.as_mut().clear();
    }
}

static COMPFUNC: &CStr = c"_main_complete";
static COMPADD_STATE: Mutex<Option<CompaddState>> = Mutex::new(None);

unsafe extern "C" fn compadd_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    // eprintln!("DEBUG(bombay)\t{}\t= {:?}\r", stringify!(nam), nam);

    unsafe {
        let mut compadd = COMPADD_STATE.lock().unwrap();
        let compadd = compadd.as_mut().unwrap();
        let result = compadd.original.as_ref().as_ref().unwrap().handlerfunc.unwrap()(nam, argv, options, func);

        if !bindings::matches.is_null() && let Some(sink) = compadd.sink.as_ref() {
            if !bindings::amatches.is_null() && !(*bindings::amatches).name.is_null() {
                // let g = CStr::from_ptr((*bindings::amatches).name);
                // eprintln!("DEBUG(dachas)\t{}\t= {:?}\r", stringify!(g), g);
            }

            // compadd can change the list matches points to by changing the group
            // so we use a hashset to store what matches we've seen before

            let matches: Vec<_> = super::linked_list::iter_linklist(bindings::matches)
                .filter_map(|ptr| (ptr as *const bindings::cmatch).as_ref())
                .filter(|m| compadd.seen.as_mut().insert(*m as _))
                .map(Match::new)
                .collect();

            if !matches.is_empty() && sink.send(matches).is_err() {
                compadd.sink.take();
            }
        }

        result
    }
}

pub fn override_compadd() -> Result<()> {
    let silent = 0;
    if unsafe{ zsh_sys::require_module(c"zsh/complete".as_ptr(), null_mut(), silent) } > 0 {
        anyhow::bail!("failed to load module zsh/complete")
    }

    let original = Builtin::pop(c"compadd").unwrap();
    let mut compadd = original.clone();

    *COMPADD_STATE.lock().unwrap() = Some(CompaddState{
        original: unsafe{ UnsafeSend::new(Some(original)) },
        ..CompaddState::default()
    });

    compadd.handlerfunc = Some(compadd_handlerfunc);
    compadd.node.flags = 0;
    compadd.add();
    Ok(())
}

pub fn restore_compadd() {
    if let Some(mut compadd) = COMPADD_STATE.lock().unwrap().take() && let Some(original) = compadd.original.as_mut().take() {
        original.add();
    }
}

// ookkkk
// zsh completion is intimately tied to zle
// so there's no "low-level" function to hook into
// the best we can do is emulate completecall()

pub fn get_completions(line: BString, sink: mpsc::UnboundedSender<Vec<Match>>) {
    {
        if let Some(compadd) = COMPADD_STATE.lock().unwrap().as_mut() {
            compadd.sink = Some(sink);
        } else {
            panic!("ui is not running");
        }
        let len = line.len();
        super::set_zle_buffer(line, len as i64 + 1);
    }

    unsafe {
        // this is kinda what completecall() does
        let mut cfargs: [*mut c_char; 1] = [null_mut()];
        bindings::cfargs = cfargs.as_mut_ptr();
        bindings::compfunc = COMPFUNC.as_ptr().cast_mut();
        // zsh will switch up the pgid if monitor and interactive are set
        super::execstring("set +o monitor", Default::default());
        bindings::menucomplete(null_mut());
        // prevent completion list from showing
        bindings::showinglist = 0;
        // bindings::invalidate_list();
        // soft exit menu completion
        bindings::minfo.cur = null_mut();
        super::execstring("set -o monitor", Default::default());

        let mut compadd = COMPADD_STATE.lock().unwrap();
        let compadd = compadd.as_mut().unwrap();
        compadd.reset();
    }
}

pub fn insert_completion(line: BString, m: &Match) -> (BString, usize) {
    unsafe {
        // set the zle buffer
        let len = line.len();
        super::set_zle_buffer(line, len as i64 + 1);

        // set start and end of word being completed
        zsh_sys::we = len as i32;
        zsh_sys::wb = zsh_sys::we - m.completion_word_len as i32;

        bindings::metafy_line();
        bindings::do_single(m.inner.as_ref() as *const _ as *mut _);
        bindings::unmetafy_line();

        let (buffer, cursor) = super::get_zle_buffer();
        let buflen = buffer.len() as _;
        (buffer, cursor.unwrap_or(buflen) as _)
    }
}
