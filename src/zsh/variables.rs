use std::collections::HashMap;
use std::ffi::{CString, CStr};
use std::os::raw::{c_int, c_char};
use anyhow::Result;
use crate::c_string_array::CStringArray;
use super::ZString;
use bstr::{BStr, BString};

fn pm_type(flags: c_int) -> c_int {
    flags & (zsh_sys::PM_SCALAR | zsh_sys::PM_INTEGER | zsh_sys::PM_EFLOAT | zsh_sys::PM_FFLOAT | zsh_sys::PM_ARRAY | zsh_sys::PM_HASHED) as c_int
}

pub struct Variable{
    value: zsh_sys::value,
    name_is_digit: bool,
}

#[derive(Debug)]
pub enum Value {
    Integer(i64),
    Float(f64),
    Array(Vec<BString>),
    String(BString),
    HashMap(HashMap<BString, BString>),
}

impl Variable {
    pub fn get<S: AsRef<BStr>>(name: S) -> Option<Self> {
        let bracks = 1;
        let c_name = CString::new(name.as_ref().to_vec()).unwrap();
        let mut c_varname_ptr = c_name.as_ptr() as *mut c_char;
        let mut value = unsafe{ std::mem::MaybeUninit::<zsh_sys::value>::zeroed().assume_init() };
        let ptr = unsafe{ zsh_sys::getvalue(
            &mut value as *mut _,
            &mut c_varname_ptr as *mut _,
            bracks,
        ) };
        if ptr.is_null() {
            None
        } else {
            Some(Self{
                value,
                name_is_digit: name
                    .as_ref()
                    .utf8_chunks()
                    .flat_map(|chunk| chunk.valid().chars())
                    .all(|c| c.is_ascii_digit()),
            })
        }
    }

    pub fn set<S: AsRef<[u8]>>(name: &str, value: S) -> Result<()> {
        let c_name = CString::new(name).unwrap();
        // setsparam will free the value for us
        let c_value: ZString = value.as_ref().into();
        if unsafe{ zsh_sys::setsparam(c_name.as_ptr() as *mut _, c_value.into_raw()) }.is_null() {
            Err(anyhow::anyhow!("failed to set var {name:?}"))
        } else {
            Ok(())
        }
    }

    pub fn unset(name: &str) {
        let c_name = CString::new(name).unwrap();
        unsafe{ zsh_sys::unsetparam(c_name.as_ptr() as *mut _); }
    }

    fn param(&mut self) -> &mut zsh_sys::param {
        unsafe{ &mut *self.value.pm }
    }

    pub fn as_bytes(&mut self) -> BString {
        let str = unsafe{
            let var = zsh_sys::getstrvalue(&mut self.value as *mut _);
            if var.is_null() {
                return BString::new(vec![]);
            }
            CStr::from_ptr(var)
        };
        str.to_bytes().into()
    }

    pub fn try_as_int(&mut self) -> Result<Option<i64>> {
        Ok(
            if self.value.flags & zsh_sys::VALFLAG_INV as c_int != 0 {
                Some(self.value.start as _)
            } else if self.param().node.flags == zsh_sys::PM_INTEGER as c_int {
                Some(unsafe{ (*self.param().gsu.i).getfn.ok_or(anyhow::anyhow!("gsu.i.getfn is missing"))?(self.param()) })
            } else {
                None
            }
        )
    }

    pub fn try_as_array(&mut self) -> Option<Vec<BString>> {
        if self.value.isarr != 0 {
            let array: CStringArray = unsafe{ zsh_sys::getarrvalue(&mut self.value as *mut _) }.into();
            Some(array.to_vec())
        } else {
            None
        }
    }

    pub fn try_as_hashmap(&mut self) -> Result<Option<HashMap<BString, BString>>> {
        if pm_type(self.param().node.flags) == zsh_sys::PM_HASHED as c_int && !self.name_is_digit {

            let mut hashmap = HashMap::new();
            unsafe {
                let param = (*self.param().gsu.h).getfn.ok_or(anyhow::anyhow!("gsu.h.getfn is missing"))?(self.param());
                let keys: CStringArray = zsh_sys::paramvalarr(param, zsh_sys::SCANPM_WANTKEYS as c_int).into();
                let values: CStringArray = zsh_sys::paramvalarr(param, zsh_sys::SCANPM_WANTVALS as c_int).into();

                let keys = keys.iter().map(Some).chain(std::iter::repeat(None));
                let values = values.iter().map(Some).chain(std::iter::repeat(None));

                for (k, v) in keys.zip(values) {
                    match (k, v) {
                        (Some(k), Some(v)) => hashmap.insert(k.to_bytes().into(), v.to_bytes().into()),
                        (Some(k), None)    => hashmap.insert(k.to_bytes().into(), vec![].into()),
                        (None, Some(_))    => return Err(anyhow::anyhow!("hashmap has more values than keys")),
                        _ => break,
                    };
                }
            }
            Ok(Some(hashmap))
        } else {
            Ok(None)
        }
    }

    pub fn try_as_float(&mut self) -> Result<Option<f64>> {
        Ok(
            if self.param().node.flags & (zsh_sys::PM_EFLOAT | zsh_sys::PM_FFLOAT) as c_int != 0 {
                Some(unsafe{ (*self.param().gsu.f).getfn.ok_or(anyhow::anyhow!("gsu.f.getfn is missing"))?(self.param()) })
            } else {
                None
            }
        )
    }

    pub fn as_value(&mut self) -> Result<Value> {
        Ok(
            if let Some(x) = self.try_as_hashmap()? {
                Value::HashMap(x)
            } else if let Some(x) = self.try_as_array() {
                Value::Array(x)
            } else if let Some(x) = self.try_as_float()? {
                Value::Float(x)
            } else if let Some(x) = self.try_as_int()? {
                Value::Integer(x)
            } else {
                Value::String(self.as_bytes())
            }
        )
    }

}
