#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use paste::paste;
use zsh_sys::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

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

unsafe impl Send for cmatch {}
unsafe impl Sync for cmatch {}
