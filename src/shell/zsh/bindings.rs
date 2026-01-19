#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::unreadable_literal)]

use super::MetaStr;
use paste::paste;
use zsh_sys::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn isset(setting: usize) -> bool {
    unsafe{ zsh_sys::opts[setting] != 0 }
}

macro_rules! make_str_getter {
    ($field:ident) => (
        paste! {
            pub fn [<get_ $field>](&self) -> Option<&MetaStr> {
                Some(unsafe{ MetaStr::from_ptr(self.$field.as_ref()?) })
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

impl std::fmt::Debug for cmatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("cmatch")
            .field("str_", &self.get_str_())
            .field("orig", &self.get_orig())
            .field("ipre", &self.get_ipre())
            .field("ripre", &self.get_ripre())
            .field("isuf", &self.get_isuf())
            .field("ppre", &self.get_ppre())
            .field("psuf", &self.get_psuf())
            .field("prpre", &self.get_prpre())
            .field("pre", &self.get_pre())
            .field("suf", &self.get_suf())
            .field("disp", &self.get_disp())
            .field("autoq", &self.get_autoq())
            .field("rems", &self.get_rems())
            .field("remf", &self.get_remf())
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
