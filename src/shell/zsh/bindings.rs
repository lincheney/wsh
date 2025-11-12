#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::unreadable_literal)]

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
                brpl: Box::into_raw(brpl.into_boxed_slice()).cast(),
                brsl: Box::into_raw(brsl.into_boxed_slice()).cast(),

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

#[derive(num_derive::FromPrimitive, Debug, Copy, Clone, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum lextok {
    NULLTOK,		/* 0  */
    SEPER,
    NEWLIN,
    SEMI,
    DSEMI,
    AMPER,		/* 5  */
    INPAR,
    OUTPAR,
    DBAR,
    DAMPER,
    OUTANG,		/* 10 */
    OUTANGBANG,
    DOUTANG,
    DOUTANGBANG,
    INANG,
    INOUTANG,		/* 15 */
    DINANG,
    DINANGDASH,
    INANGAMP,
    OUTANGAMP,
    AMPOUTANG,		/* 20 */
    OUTANGAMPBANG,
    DOUTANGAMP,
    DOUTANGAMPBANG,
    TRINANG,
    BAR,		/* 25 */
    BARAMP,
    INOUTPAR,
    DINPAR,
    DOUTPAR,
    AMPERBANG,		/* 30 */
    SEMIAMP,
    SEMIBAR,
    DOUTBRACK,
    STRING,
    ENVSTRING,		/* 35 */
    ENVARRAY,
    ENDINPUT,
    LEXERR,

    /* Tokens for reserved words */
    BANG,	/* !         */
    DINBRACK,	/* [[        */	/* 40 */
    INBRACE,    /* {         */
    OUTBRACE,   /* }         */
    CASE,	/* case      */
    COPROC,	/* coproc    */
    DOLOOP,	/* do        */ /* 45 */
    DONE,	/* done      */
    ELIF,	/* elif      */
    ELSE,	/* else      */
    ZEND,	/* end       */
    ESAC,	/* esac      */ /* 50 */
    FI,		/* fi        */
    FOR,	/* for       */
    FOREACH,	/* foreach   */
    FUNC,	/* function  */
    IF,		/* if        */ /* 55 */
    NOCORRECT,	/* nocorrect */
    REPEAT,	/* repeat    */
    SELECT,	/* select    */
    THEN,	/* then      */
    TIME,	/* time      */ /* 60 */
    UNTIL,	/* until     */
    WHILE,	/* while     */
    TYPESET     /* typeset or similar */
}

#[derive(num_derive::FromPrimitive, Debug, Copy, Clone, PartialEq)]
pub enum token {
    Pound      = 0x84,
    String     = 0x85,
    Hat        = 0x86,
    Star       = 0x87,
    Inpar      = 0x88,
    Inparmath  = 0x89,
    Outpar     = 0x8a,
    Outparmath = 0x8b,
    Qstring    = 0x8c,
    Equals     = 0x8d,
    Bar        = 0x8e,
    Inbrace    = 0x8f,
    Outbrace   = 0x90,
    Inbrack    = 0x91,
    Outbrack   = 0x92,
    Tick       = 0x93,
    Inang      = 0x94,
    Outang     = 0x95,
    OutangProc = 0x96,
    Quest      = 0x97,
    Tilde      = 0x98,
    Qtick      = 0x99,
    Comma      = 0x9a,
    Dash       = 0x9b, /* Only in patterns */
    Bang       = 0x9c, /* Only in patterns */

    Snull      = 0x9d,
    Dnull      = 0x9e,
    Bnull      = 0x9f,

    Bnullkeep  = 0xa0,

    Nularg     = 0xa1,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum WidgetFlag {
    WIDGET_INT = 1 << 0,
}
