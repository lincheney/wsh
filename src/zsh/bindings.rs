#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use zsh_sys::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

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
