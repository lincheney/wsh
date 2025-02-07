use std::collections::HashMap;
use std::ffi::{CString, CStr};
use std::os::raw::{c_int, c_char};
use anyhow::Result;
use crate::c_string_array::CStringArray;
use super::ZString;

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
    Array(Vec<Vec<u8>>),
    String(Vec<u8>),
    HashMap(HashMap<Vec<u8>, Vec<u8>>),
}

impl Variable {
    pub fn get(name: &str) -> Option<Self> {
        let bracks = 1;
        let c_name = CString::new(name).unwrap();
        let mut c_varname_ptr = c_name.as_ptr() as *mut c_char;
        let mut value = unsafe{ std::mem::MaybeUninit::<zsh_sys::value>::zeroed().assume_init() };
        let ptr = unsafe{ zsh_sys::getvalue(
            &mut value as *mut _,
            &mut c_varname_ptr as *mut _,
            bracks,
        ) };
        if ptr.is_null() {
            return None
        } else {
            Some(Self{
                value,
                name_is_digit: name.chars().all(|c| c.is_digit(10)),
            })
        }
    }

    pub fn set(name: &str, value: &str) -> Result<()> {
        let c_name = CString::new(name).unwrap();
        // setsparam will free the value for us
        let c_value: ZString = value.into();
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

    pub fn to_bytes(&mut self) -> Vec<u8> {
        let str = unsafe{
            let var = zsh_sys::getstrvalue(&mut self.value as *mut _);
            if var.is_null() {
                return vec![];
            }
            CStr::from_ptr(var)
        };
        str.to_bytes().to_owned()
    }

    pub fn to_value(&mut self) -> Result<Value> {
        Ok(
            if self.value.flags & zsh_sys::VALFLAG_INV as c_int != 0 {
                Value::Integer(self.value.start as _)

            } else if self.value.isarr != 0 {
                let array: CStringArray = unsafe{ zsh_sys::getarrvalue(&mut self.value as *mut _) }.into();
                Value::Array(array.to_vec())

            } else {
                let param = unsafe{ &mut *self.value.pm };

                if pm_type(param.node.flags) == zsh_sys::PM_HASHED as c_int && !self.name_is_digit {

                    let mut hashmap = HashMap::new();
                    unsafe {
                        let param = (&*param.gsu.h).getfn.ok_or(anyhow::anyhow!("gsu.h.getfn is missing"))?(param);
                        let keys: CStringArray = zsh_sys::paramvalarr(param, zsh_sys::SCANPM_WANTKEYS as c_int).into();
                        let values: CStringArray = zsh_sys::paramvalarr(param, zsh_sys::SCANPM_WANTVALS as c_int).into();

                        let keys = keys.iter().map(|v| Some(v)).chain(std::iter::repeat(None));
                        let values = values.iter().map(|v| Some(v)).chain(std::iter::repeat(None));

                        for (k, v) in keys.zip(values) {
                            match (k, v) {
                                (Some(k), Some(v)) => hashmap.insert(k.to_bytes().to_owned(), v.to_bytes().to_owned()),
                                (Some(k), None)    => hashmap.insert(k.to_bytes().to_owned(), vec![]),
                                (None, Some(_))    => return Err(anyhow::anyhow!("hashmap has more values than keys")),
                                _ => break,
                            };
                        }
                    }
                    Value::HashMap(hashmap)

                } else if pm_type(param.node.flags) == zsh_sys::PM_INTEGER as c_int {
                    Value::Integer(unsafe{
                        (&*param.gsu.i).getfn.ok_or(anyhow::anyhow!("gsu.i.getfn is missing"))?(param)
                    })

                } else if param.node.flags & (zsh_sys::PM_EFLOAT | zsh_sys::PM_FFLOAT) as c_int != 0 {
                    Value::Float(unsafe{
                        (&*param.gsu.f).getfn.ok_or(anyhow::anyhow!("gsu.f.getfn is missing"))?(param)
                    })

                } else {
                    // i guess its a string
                    let ptr = unsafe{ zsh_sys::getstrvalue(&mut self.value as *mut _) };
                    if ptr.is_null() {
                        Value::String(vec![])
                    } else {
                        let value = unsafe{ CStr::from_ptr(ptr) }.to_bytes().to_owned();
                        unsafe{ zsh_sys::zsfree(ptr); }
                        Value::String(value)
                    }
                }
            }
        )
    }
}
