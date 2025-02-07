use std::sync::{OnceLock, Mutex};
use std::ffi::{CString, CStr};
use std::os::raw::*;
use std::ptr::null_mut;
use std::default::Default;
use super::bindings;

#[derive(Debug)]
struct CompaddState {
    original: zsh_sys::Builtin,
}

impl Default for CompaddState {
    fn default() -> Self {
        CompaddState{
            original: null_mut(),
        }
    }
}

unsafe impl Send for CompaddState {}
static COMPADD_STATE: OnceLock<Mutex<CompaddState>> = OnceLock::new();

unsafe extern "C" fn compadd_handlerfunc(nam: *mut c_char, argv: *mut *mut c_char, options: zsh_sys::Options, func: c_int) -> c_int {
    // eprintln!("DEBUG(bombay)\t{}\t= {:?}\r", stringify!(nam), nam);


    let compadd = COMPADD_STATE.get().unwrap().lock().unwrap();
    let result = (*compadd.original).handlerfunc.unwrap()(nam, argv, options, func);

    if !bindings::amatches.is_null() && !(*bindings::amatches).name.is_null() {
        let g = CStr::from_ptr((*bindings::amatches).name);
        eprintln!("DEBUG(dachas)\t{}\t= {:?}\r", stringify!(g), g);
    }

    if !bindings::matches.is_null() {
        let mut node = (*bindings::matches).list.first;
        while !node.is_null() {
            let dat = (*node).dat as *mut bindings::cmatch;
            eprintln!("DEBUG(pucks) \t{}\t= {:?}\r", stringify!(node), (std::ffi::CStr::from_ptr((*dat).str_), (*dat).gnum));
            node = (*node).next;
        }
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
pub fn get_completions(line: &str) {
    unsafe {
        // set the zle buffer
        zsh_sys::startparamscope();
        bindings::makezleparams(0);
        super::Variable::set("BUFFER", line);
        eprintln!("DEBUG(hinges)\t{}\t= {:?}", stringify!(line), line);
        super::Variable::set("CURSOR", &format!("{}", line.len() + 1));
        zsh_sys::endparamscope();

        // this is kinda what completecall() does
        let cfargs: [*mut c_char; 1] = [null_mut()];
        bindings::cfargs = cfargs.as_ptr() as _;
        bindings::compfunc = std::ffi::CString::new("_main_complete").unwrap().into_raw();
        bindings::menucomplete(null_mut());
    }
}
