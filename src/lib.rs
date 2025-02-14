use std::sync::Arc;
use anyhow::Result;
use async_std::stream::StreamExt;
use futures::{select, future::FutureExt};
use std::ops::Deref;
use std::os::fd::AsRawFd;
use std::os::raw::{c_char, c_int};
use std::ptr::null_mut;
use std::sync::{LazyLock};
use std::ffi::CString;

mod shell;
mod zsh;
mod ui;
mod keybind;
mod completion;
mod buffer;
mod c_string_array;
mod tui;
mod promise;
mod event_stream;
#[macro_use]
mod utils;

async fn main() -> Result<()> {

    // crossterm will default to opening fd 0
    // but zsh will mangle this halfway through
    // so trick crossterm into opening a separate fd to the tty
    let devnull = std::fs::File::open("/dev/null").unwrap();
    let old_stdin = nix::unistd::dup(0)?;
    nix::unistd::dup2(devnull.as_raw_fd(), 0)?;

    let (mut events, event_locker) = event_stream::EventStream::new();

    let shell = shell::Shell::new();
    let ui = ui::Ui::new(&shell, event_locker).await?;
    ui.activate().await?;
    ui.draw(&shell, false).await?;

    drop(devnull);
    nix::unistd::dup2(old_stdin, 0)?;
    nix::unistd::close(old_stdin)?;

    events.run(|event| async {
        match event {
            Some(Ok(event)) => {
                match ui.handle_event(event, &shell).await {
                    Ok(true) => None,
                    Ok(false) => Some(Ok(())),
                    Err(e) => Some(Err(e)),
                }
            }
            Some(Err(event)) => { println!("Error: {:?}\r", event); None },
            None => Some(Ok(())),
        }
    }).await?;

    Ok(())
}


unsafe extern "C" fn handlerfunc(_nam: *mut c_char, _argv: *mut *mut c_char, _options: zsh_sys::Options, _func: c_int) -> c_int {
    async_std::task::block_on(async {
        if let Err(x) = main().await {
            eprintln!("DEBUG(legman)\t{}\t= {:?}", stringify!(x), x);
        }
    });
    0
}

#[derive(Debug)]
struct Features(zsh_sys::features);
unsafe impl Send for Features {}
unsafe impl Sync for Features {}

const DEFAULT_BUILTIN: zsh_sys::builtin = zsh_sys::builtin{
    node: zsh_sys::hashnode{ next: null_mut(), nam: null_mut(), flags: 0 },
    handlerfunc: None,
    minargs: 0,
    maxargs: 0,
    funcid: 0,
    optstr: null_mut(),
    defopts: null_mut(),
};

static MODULE_FEATURES: LazyLock<Features> = LazyLock::new(|| {
    let bn_list = Box::new([
        zsh_sys::builtin{
            node: zsh_sys::hashnode{
                next: null_mut(),
                nam: CString::new("wash").unwrap().into_raw(),
                flags: 0,
            },
            handlerfunc: Some(handlerfunc),
            ..DEFAULT_BUILTIN
        },
    ]);
    let bn_list = Box::leak(bn_list);

    Features(zsh_sys::features{
        // builtins
        bn_list: bn_list.as_mut_ptr(), bn_size: bn_list.len() as _,
        // conditions
        cd_list: null_mut(), cd_size: 0,
        // parameters
        pd_list: null_mut(), pd_size: 0,
        // math funcs
        mf_list: null_mut(), mf_size: 0,
        // abstract features
        n_abstract: 0,
    })
});


#[no_mangle]
pub extern fn setup_() -> c_int {
    0
}

#[no_mangle]
pub extern fn features_(module: zsh_sys::Module, features: *mut *mut *mut c_char) -> c_int {
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ *features = zsh_sys::featuresarray(module, module_features as *const _ as *mut _); }
    0
}

#[no_mangle]
pub extern fn enables_(module: zsh_sys::Module, enables: *mut *mut c_int) -> c_int {
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ zsh_sys::handlefeatures(module, module_features as *const _ as *mut _, enables) }
}

#[no_mangle]
pub extern fn boot_() -> c_int {
    zsh::completion::override_compadd();
    0
}

#[no_mangle]
pub extern fn cleanup_(module: zsh_sys::Module) -> c_int {
    zsh::completion::restore_compadd();
    let module_features: &zsh_sys::features = &MODULE_FEATURES.deref().0;
    unsafe{ zsh_sys::setfeatureenables(module, module_features as *const _ as *mut _, null_mut()) }
}

#[no_mangle]
pub extern fn finish_() -> c_int {
    0
}
