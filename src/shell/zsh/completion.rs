use std::collections::HashSet;
use std::cell::RefCell;
use std::ops::ControlFlow;
use regex::bytes::Regex;
use anyhow::Result;
use std::os::raw::*;
use std::ptr::{null_mut};
use std::default::Default;
use bstr::{BString, ByteSlice};
use super::{bindings, builtin::Builtin};
use super::MetaStr;
use crate::ui::buffer::suffix::{Suffix, RemovalTrigger};

const CMF_REMOVE: i32 =   1<< 1;	/* remove the suffix */

pub struct Match {
    inner: bindings::cmatch,
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
                inner,
                completion_word_len: (zsh_sys::we - zsh_sys::wb).max(0) as usize,
                nbrbeg: super::nbrbeg,
                nbrend: super::nbrend,
            }
        }
    }

    pub fn get_orig(&self) -> Option<&MetaStr> {
        self.inner.get_orig()
    }

    pub fn get_mode(&self) -> u32 {
        self.inner.mode
    }

    pub fn get_fmode(&self) -> u32 {
        self.inner.fmode
    }

    pub fn as_suffix(&self) -> Option<Suffix> {
        if (self.inner.flags & CMF_REMOVE) == 0 {
            return None
        }

        let suf = self.inner.get_suf()?.unmetafy();
        let byte_len = suf.len();
        let removal_trigger = if let Some(name) = self.inner.get_remf() {
            RemovalTrigger::Function{name: name.to_owned(), len: suf.graphemes().count()}
        } else if let Some(chars) = self.inner.get_rems() {
            let mut regex = format!("^[{}]", chars.to_owned().unmetafy());
            let match_empty = regex.contains("\\-");
            if match_empty {
                regex = regex.replace("\\-", "");
            }
            let regex = Regex::new(&regex).ok()?;
            RemovalTrigger::Chars{regex, match_empty}
        } else {
            RemovalTrigger::Default(suf.into_owned())
        };

        Some(Suffix {
            removal_trigger,
            byte_len,
        })
    }

}

impl Drop for Match {
    fn drop(&mut self) {
        unsafe {
            zsh_sys::zsfree(self.inner.str_);
            zsh_sys::zsfree(self.inner.orig);
            zsh_sys::zsfree(self.inner.ipre);
            zsh_sys::zsfree(self.inner.ripre);
            zsh_sys::zsfree(self.inner.isuf);
            zsh_sys::zsfree(self.inner.ppre);
            zsh_sys::zsfree(self.inner.psuf);
            zsh_sys::zsfree(self.inner.prpre);
            zsh_sys::zsfree(self.inner.pre);
            zsh_sys::zsfree(self.inner.suf);
            zsh_sys::zsfree(self.inner.disp);
            zsh_sys::zsfree(self.inner.autoq);
            zsh_sys::zsfree(self.inner.rems);
            zsh_sys::zsfree(self.inner.remf);
            if !self.inner.brpl.is_null() {
                drop(Box::from_raw(self.inner.brpl));
            }
            if !self.inner.brsl.is_null() {
                drop(Box::from_raw(self.inner.brsl));
            }
        }
    }
}

#[derive(Default)]
struct CompaddState {
    // original compadd function
    original: Option<Builtin>,
    // callback to send matches
    callback: Option<Box<dyn FnMut(Vec<Match>) -> ControlFlow<()>>>,
    // matches we have already seen
    seen: HashSet<*const bindings::cmatch>,
}

impl CompaddState {
    fn reset(&mut self) {
        self.callback.take();
        self.seen.clear();
    }
}

static COMPFUNC: &MetaStr = meta_str!(c"_main_complete");
thread_local! {
    static COMPADD_STATE: RefCell<Option<CompaddState>> = const{ RefCell::new(None) };
}

unsafe extern "C" fn compadd_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    // eprintln!("DEBUG(bombay)\t{}\t= {:?}\r", stringify!(nam), nam);

    COMPADD_STATE.with_borrow_mut(|compadd| {
        unsafe {
            let compadd = compadd.as_mut().unwrap();
            let result = compadd.original.as_ref().unwrap().handlerfunc.unwrap()(nam, argv, options, func);

            if !bindings::matches.is_null() && let Some(callback) = compadd.callback.as_mut() {
                if !bindings::amatches.is_null() && !(*bindings::amatches).name.is_null() {
                    // let g = MetaStr::from_ptr((*bindings::amatches).name);
                    // eprintln!("DEBUG(dachas)\t{}\t= {:?}\r", stringify!(g), g);
                }

                // compadd can change the list matches points to by changing the group
                // so we use a hashset to store what matches we've seen before

                let matches: Vec<_> = super::linked_list::iter_linklist(bindings::matches)
                    .filter_map(|ptr| (ptr as *const bindings::cmatch).as_ref())
                    .filter(|m| compadd.seen.insert(*m as _))
                    .map(Match::new)
                    .collect();

                if !matches.is_empty() && callback(matches).is_break() {
                    compadd.callback.take();
                }
            }

            result
        }
    })
}

pub fn override_compadd() -> Result<()> {
    let silent = 0;
    if unsafe{ zsh_sys::require_module(c"zsh/complete".as_ptr(), null_mut(), silent) } > 0 {
        anyhow::bail!("failed to load module zsh/complete")
    }

    let original = Builtin::pop(meta_str!(c"compadd")).unwrap();
    let mut compadd = original.clone();

    COMPADD_STATE.set(Some(CompaddState{
        original: Some(original),
        ..CompaddState::default()
    }));

    compadd.handlerfunc = Some(compadd_handlerfunc);
    compadd.node.flags = 0;
    compadd.add();
    Ok(())
}

pub fn restore_compadd() {
    COMPADD_STATE.with_borrow_mut(|compadd| {
        if let Some(mut compadd) = compadd.take() && let Some(original) = compadd.original.take() {
            original.add();
        }
    });
}

// ookkkk
// zsh completion is intimately tied to zle
// so there's no "low-level" function to hook into
// the best we can do is emulate completecall()

pub fn get_completions(line: BString, callback: Box<dyn FnMut(Vec<Match>) -> ControlFlow<()>>) {
    COMPADD_STATE.with_borrow_mut(|compadd| {
        if let Some(compadd) = compadd {
            compadd.callback = Some(callback);
        } else {
            panic!("ui is not running");
        }
        let len = line.len();
        super::set_zle_buffer(line, len as i64 + 1);
    });

    unsafe {
        // this is kinda what completecall() does
        let mut cfargs: [*mut c_char; 1] = [null_mut()];
        bindings::cfargs = cfargs.as_mut_ptr();
        bindings::compfunc = COMPFUNC.as_ptr().cast_mut();
        // zsh will switch up the pgid if monitor and interactive are set
        super::execstring(meta_str!(c"set +o monitor"), Default::default());
        bindings::menucomplete(null_mut());
        // prevent completion list from showing
        bindings::showinglist = 0;
        // bindings::invalidate_list();
        // soft exit menu completion
        bindings::minfo.cur = null_mut();
        super::execstring(meta_str!(c"set -o monitor"), Default::default());

        COMPADD_STATE.with_borrow_mut(|compadd| {
            if let Some(compadd) = compadd {
                compadd.reset();
            }
        });
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
        bindings::do_single(&m.inner as *const _ as *mut _);
        bindings::unmetafy_line();

        let (buffer, cursor) = super::get_zle_buffer();
        let buflen = buffer.len() as _;
        (buffer, cursor.unwrap_or(buflen) as _)
    }
}
