#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use paste::paste;
use zsh_sys::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn isset(setting: usize) -> bool {
    unsafe{ zsh_sys::opts[setting] != 0 }
}

macro_rules! make_str_getter {
    ($field:ident) => (
        paste! {
            pub fn [<get_ $field>](&self) -> Option<&std::ffi::CStr> {
                Some(unsafe{ std::ffi::CStr::from_ptr(self.$field.as_ref()?) })
            }
        }
    )
}

impl cmatch {
    make_str_getter!(str_);
    make_str_getter!(orig);
    make_str_getter!(ipre);
    make_str_getter!(ripre);
    make_str_getter!(isuf);
    make_str_getter!(ppre);
    make_str_getter!(psuf);
    make_str_getter!(prpre);
    make_str_getter!(pre);
    make_str_getter!(suf);
    make_str_getter!(disp);
    make_str_getter!(autoq);
    make_str_getter!(rems);
    make_str_getter!(remf);
}

impl Clone for cmatch {
    fn clone(&self) -> Self {
        unsafe {
            let brpl: Vec<_> = (0..nbrbeg).map(|i| *self.brpl.offset(i as _)).collect();
            let brsl: Vec<_> = (0..nbrend).map(|i| *self.brsl.offset(i as _)).collect();

            Self{
                str_: ztrdup(self.str_),
                orig: ztrdup(self.orig),
                ipre: ztrdup(self.ipre),
                ripre: ztrdup(self.ripre),
                isuf: ztrdup(self.isuf),
                ppre: ztrdup(self.ppre),
                psuf: ztrdup(self.psuf),
                prpre: ztrdup(self.prpre),
                pre: ztrdup(self.pre),
                suf: ztrdup(self.suf),
                disp: ztrdup(self.disp),
                autoq: ztrdup(self.autoq),
                rems: ztrdup(self.rems),
                remf: ztrdup(self.remf),
                brpl: Box::into_raw(brpl.into_boxed_slice()) as _,
                brsl: Box::into_raw(brsl.into_boxed_slice()) as _,

                flags: self.flags,
                qipl: self.qipl,
                qisl: self.qisl,
                rnum: self.rnum,
                gnum: self.gnum,
                mode: self.mode,
                modec: self.modec,
                fmode: self.fmode,
                fmodec: self.fmodec,
            }
        }
    }
}

impl std::fmt::Debug for cmatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        use std::ffi::CStr;
        use std::ptr::NonNull;

        f.debug_struct("cmatch")
            .field("str_", &NonNull::new(self.str_).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("orig", &NonNull::new(self.orig).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("ipre", &NonNull::new(self.ipre).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("ripre", &NonNull::new(self.ripre).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("isuf", &NonNull::new(self.isuf).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("ppre", &NonNull::new(self.ppre).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("psuf", &NonNull::new(self.psuf).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("prpre", &NonNull::new(self.prpre).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("pre", &NonNull::new(self.pre).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("suf", &NonNull::new(self.suf).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("disp", &NonNull::new(self.disp).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("autoq", &NonNull::new(self.autoq).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("rems", &NonNull::new(self.rems).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("remf", &NonNull::new(self.remf).map(|s| unsafe{ CStr::from_ptr(s.as_ptr()) }))
            .field("brpl", &self.brpl)
            .field("brsl", &self.brsl)

            .field("flags", &self.flags)
            .field("qipl", &self.qipl)
            .field("qisl", &self.qisl)
            .field("rnum", &self.rnum)
            .field("gnum", &self.gnum)
            .field("mode", &self.mode)
            .field("modec", &self.modec)
            .field("fmode", &self.fmode)
            .field("fmodec", &self.fmodec)
            .finish()
    }
}

unsafe impl Send for cmatch {}
unsafe impl Sync for cmatch {}
