use std::collections::HashMap;
use std::ffi::{CString, CStr};
use std::os::raw::{c_int, c_char};
use anyhow::Result;
use crate::c_string_array::{CStringArray, CStrArray};
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

impl From<i64> for Value {
    fn from(val: i64) -> Self {
        Value::Integer(val)
    }
}

impl From<f64> for Value {
    fn from(val: f64) -> Self {
        Value::Float(val)
    }
}

impl From<Vec<BString>> for Value {
    fn from(val: Vec<BString>) -> Self {
        Value::Array(val)
    }
}

impl From<BString> for Value {
    fn from(val: BString) -> Self {
        Value::String(val)
    }
}

impl From<&BStr> for Value {
    fn from(val: &BStr) -> Self {
        Value::String(val.to_owned())
    }
}

impl From<String> for Value {
    fn from(val: String) -> Self {
        Value::String(val.into())
    }
}

impl From<HashMap<BString, BString>> for Value {
    fn from(val: HashMap<BString, BString>) -> Self {
        Value::HashMap(val)
    }
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

    pub fn set(name: &[u8], value: Value) -> Result<()> {
        let c_name = CString::new(name).unwrap();
        let name = c_name.as_ptr() as *mut _;
        let result = match value {
            Value::HashMap(value) => {
                let value: Vec<BString> = value.into_iter().flat_map(|(k, v)| [k, v]).collect();
                let value: CStringArray = value.into();
                unsafe{ zsh_sys::setaparam(name, value.into_ptr()) }
            },
            Value::Array(value) => {
                let value: CStringArray = value.into();
                unsafe{ zsh_sys::setaparam(name, value.into_ptr()) }
            },
            Value::Float(value) => {
                let value = zsh_sys::mnumber{
                    type_: zsh_sys::MN_FLOAT as _,
                    u: zsh_sys::mnumber__bindgen_ty_1{ d: value },
                };
                unsafe{ zsh_sys::setnparam(name, value) }
            },
            Value::Integer(value) => {
                unsafe{ zsh_sys::setiparam(name, value) }
            },
            Value::String(value) => {
                // setsparam will free the value for us
                let c_value: ZString = (&value[..]).into();
                unsafe{ zsh_sys::setsparam(name, c_value.into_raw()) }
            },
        };
        if result.is_null() {
            Err(anyhow::anyhow!("failed to set var {name:?}"))
        } else {
            Ok(())
        }

    }

    pub fn unset(name: &[u8]) {
        let c_name = CString::new(name).unwrap();
        unsafe{ zsh_sys::unsetparam(c_name.as_ptr() as *mut _); }
    }

    pub fn export(&self) {
        unsafe{ zsh_sys::export_param(self.value.pm) }
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
            let array: CStrArray = unsafe{ zsh_sys::getarrvalue(&mut self.value as *mut _) }.into();
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
                let keys: CStrArray = zsh_sys::paramvalarr(param, zsh_sys::SCANPM_WANTKEYS as c_int).into();
                let values: CStrArray = zsh_sys::paramvalarr(param, zsh_sys::SCANPM_WANTVALS as c_int).into();

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
                x.into()
            } else if let Some(x) = self.try_as_array() {
                x.into()
            } else if let Some(x) = self.try_as_float()? {
                x.into()
            } else if let Some(x) = self.try_as_int()? {
                x.into()
            } else {
                self.as_bytes().into()
            }
        )
    }

}
