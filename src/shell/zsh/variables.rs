use std::collections::HashMap;
use std::ptr::NonNull;
use std::os::raw::{c_int, c_long, c_char};
use anyhow::Result;
use bstr::{BStr, BString};
use super::{MetaStr, MetaString, MetaArray};

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

fn try_hashtable_to_hashmap(table: zsh_sys::HashTable) -> Result<HashMap<BString, BString>> {
    let mut hashmap = HashMap::new();
    unsafe {
        // paramvalarr allocates on the arena; do not free
        let keys = MetaArray::iter_ptr(zsh_sys::paramvalarr(table, zsh_sys::SCANPM_WANTKEYS as c_int).cast());
        let values = MetaArray::iter_ptr(zsh_sys::paramvalarr(table, zsh_sys::SCANPM_WANTVALS as c_int).cast());

        let keys = keys.map(Some).chain(std::iter::repeat(None));
        let values = values.map(Some).chain(std::iter::repeat(None));

        for (k, v) in keys.zip(values) {
            match (k, v) {
                (Some(k), Some(v)) => hashmap.insert(k.unmetafy().into_owned(), v.unmetafy().into_owned()),
                (Some(k), None) => hashmap.insert(k.unmetafy().into_owned(), vec![].into()),
                (None, Some(_))    => anyhow::bail!("hashmap has more values than keys"),
                _ => break,
            };
        }
    }
    Ok(hashmap)
}

impl Variable {
    pub(in crate::shell) fn get(name: &MetaStr) -> Option<Self> {
        let bracks = 1;
        let mut ptr = name.as_ptr().cast_mut();
        let mut value: zsh_sys::value = unsafe{ std::mem::MaybeUninit::zeroed().assume_init() };
        let ret = unsafe{ zsh_sys::getvalue(
            &raw mut value,
            &raw mut ptr,
            bracks,
        ) };
        if ret.is_null() {
            None
        } else {
            Some(Self{
                value,
                name_is_digit: BStr::new(name.to_bytes())
                    .utf8_chunks()
                    .flat_map(|chunk| chunk.valid().chars())
                    .all(|c| c.is_ascii_digit()),
            })
        }
    }

    pub(in crate::shell) fn set(name: &MetaStr, value: Value, local: bool) -> Result<()> {
        let name = name.as_ptr().cast_mut();
        let param = match value {
            Value::HashMap(value) => {
                let value: MetaArray = value.into_iter()
                    .flat_map(|(k, v)| [k, v])
                    .map(|x| x.into())
                    .collect();
                unsafe{ zsh_sys::sethparam(name, value.into_raw()) }
            },
            Value::Array(value) => {
                let value: MetaArray = value.into_iter()
                    .map(|x| x.into())
                    .collect();
                unsafe{ zsh_sys::setaparam(name, value.into_raw()) }
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
                unsafe {
                    let ptr = value.into_raw();
                    zsh_sys::setsparam(name, ptr)
                }
            },
        };

        if let Some(mut param) = NonNull::new(param) {
            if local {
                unsafe {
                    param.as_mut().level = zsh_sys::locallevel;
                }
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!("failed to set var {name:?}"))
        }

    }

    pub(in crate::shell) fn unset(name: &MetaStr) {
        unsafe{ zsh_sys::unsetparam(name.as_ptr().cast_mut()); }
    }

    pub(in crate::shell) fn export(&self) {
        unsafe{ zsh_sys::export_param(self.value.pm) }
    }

    fn param(&mut self) -> &mut zsh_sys::param {
        unsafe{ &mut *self.value.pm }
    }

    pub fn as_meta_bytes(&mut self) -> Option<&MetaStr> {
        unsafe{
            let var = zsh_sys::getstrvalue(&raw mut self.value);
            if var.is_null() {
                None
            } else {
                Some(MetaStr::from_ptr(var))
            }
        }
    }

    pub fn as_bytes(&mut self) -> BString {
        match self.as_meta_bytes() {
            Some(x) => x.unmetafy().into_owned(),
            None => BString::new(vec![]),
        }
    }

    pub fn try_as_int(&mut self) -> Result<Option<i64>> {
        Ok(
            if self.value.flags & zsh_sys::VALFLAG_INV as c_int != 0 {
                Some(self.value.start.into())
            } else if self.param().node.flags & zsh_sys::PM_INTEGER as c_int != 0 {
                Some(unsafe{ (*self.param().gsu.i).getfn.ok_or(anyhow::anyhow!("gsu.i.getfn is missing"))?(self.param()) })
            } else {
                None
            }
        )
    }

    pub fn try_as_array(&mut self) -> Option<Vec<BString>> {
        if self.value.isarr != 0 {
            let array = unsafe{ MetaArray::iter_ptr(zsh_sys::getarrvalue(&raw mut self.value) as _) };
            Some(array.map(|x| x.unmetafy().into_owned()).collect())
        } else {
            None
        }
    }

    pub fn try_as_hashmap(&mut self) -> Result<Option<HashMap<BString, BString>>> {
        if pm_type(self.param().node.flags) == zsh_sys::PM_HASHED as c_int && !self.name_is_digit {
            let table = unsafe{
                (*self.param().gsu.h).getfn.ok_or(anyhow::anyhow!("gsu.h.getfn is missing"))?(self.param())
            };
            try_hashtable_to_hashmap(table).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn try_as_float(&mut self) -> Result<Option<f64>> {
        Ok(
            if self.param().node.flags & (zsh_sys::PM_EFLOAT | zsh_sys::PM_FFLOAT) as c_int != 0 {
                Some(unsafe{ (*self.param().gsu.f).getfn.ok_or(anyhow::anyhow!("gsu.f.getfn is missing"))?(self.param()) })
            } else {
                self.try_as_int()?.map(|val| val as _)
            }
        )
    }

    pub fn as_value(&mut self) -> Result<Value> {
        Ok(
            if let Some(x) = self.try_as_hashmap()? {
                x.into()
            } else if let Some(x) = self.try_as_array() {
                x.into()
            } else if let Some(x) = self.try_as_int()? {
                x.into()
            } else if let Some(x) = self.try_as_float()? {
                x.into()
            } else {
                self.as_bytes().into()
            }
        )
    }

    pub(in crate::shell) fn create_dynamic<T: VariableGSU>(
        name: &MetaStr,
        get: Box<dyn Send + Fn() -> T>,
        set: Option<Box<dyn Send + Fn(T)>>,
        unset: Option<Box<dyn Send + Fn(bool)>>,
    ) -> Result<()> {
        let flag = T::FLAG | zsh_sys::PM_SPECIAL | zsh_sys::PM_REMOVABLE | zsh_sys::PM_LOCAL;
        let gsu = CustomGSU { get, set, unset };
        unsafe {
            let param = zsh_sys::createparam(name.as_ptr().cast_mut(), flag as _);
            if param.is_null() {
                anyhow::bail!("parameter {name} already exists");
            }
            (*param).level = zsh_sys::locallevel;
            // stuff the actual gsu into the data field
            (*param).u.data = Box::into_raw(Box::new(gsu)).cast();

            if T::FLAG == zsh_sys::PM_SCALAR {
                (*param).gsu.s = &raw const CUSTOM_SCALAR_GSU;
            } else if T::FLAG == zsh_sys::PM_INTEGER {
                (*param).gsu.i = &raw const CUSTOM_INTEGER_GSU;
            } else if T::FLAG == zsh_sys::PM_ARRAY {
                (*param).gsu.a = &raw const CUSTOM_ARRAY_GSU;
            } else if T::FLAG == zsh_sys::PM_FFLOAT {
                (*param).gsu.f = &raw const CUSTOM_FLOAT_GSU;
            } else if T::FLAG == zsh_sys::PM_HASHED {
                (*param).gsu.h = &raw const CUSTOM_HASH_GSU;
            } else {
                unreachable!();
            }
        }
        Ok(())
    }

}

pub struct CustomGSU<T> {
    get: Box<dyn Send + Fn() -> T>,
    set: Option<Box<dyn Send + Fn(T)>>,
    unset: Option<Box<dyn Send + Fn(bool)>>,
}

static CUSTOM_SCALAR_GSU: zsh_sys::gsu_scalar = zsh_sys::gsu_scalar {
    getfn: Some(custom_gsu_get::<BString>),
    setfn: Some(custom_gsu_set::<BString>),
    unsetfn: Some(custom_gsu_unset::<BString>),
};
static CUSTOM_INTEGER_GSU: zsh_sys::gsu_integer = zsh_sys::gsu_integer {
    getfn: Some(custom_gsu_get::<c_long>),
    setfn: Some(custom_gsu_set::<c_long>),
    unsetfn: Some(custom_gsu_unset::<c_long>),
};
static CUSTOM_FLOAT_GSU: zsh_sys::gsu_float = zsh_sys::gsu_float {
    getfn: Some(custom_gsu_get::<f64>),
    setfn: Some(custom_gsu_set::<f64>),
    unsetfn: Some(custom_gsu_unset::<f64>),
};
static CUSTOM_ARRAY_GSU: zsh_sys::gsu_array = zsh_sys::gsu_array {
    getfn: Some(custom_gsu_get::<Vec<BString>>),
    setfn: Some(custom_gsu_set::<Vec<BString>>),
    unsetfn: Some(custom_gsu_unset::<Vec<BString>>),
};
static CUSTOM_HASH_GSU: zsh_sys::gsu_hash = zsh_sys::gsu_hash {
    getfn: Some(custom_gsu_get::<HashMap<BString, BString>>),
    setfn: Some(custom_gsu_set::<HashMap<BString, BString>>),
    unsetfn: Some(custom_gsu_unset::<HashMap<BString, BString>>),
};

unsafe extern "C" fn custom_gsu_get<T: VariableGSU>(param: zsh_sys::Param) -> T::Type {
    unsafe {
        ((*((*param).u.data as *const CustomGSU<T>)).get)().into_raw()
    }
}
unsafe extern "C" fn custom_gsu_set<T: VariableGSU>(param: zsh_sys::Param, value: T::Type) {
    unsafe {
        if let Some(set) = &(*((*param).u.data as *const CustomGSU<T>)).set {
            set(T::from_raw(param, value));
        }
    }
}
unsafe extern "C" fn custom_gsu_unset<T: VariableGSU>(param: zsh_sys::Param, explicit: c_int) {
    unsafe {
        let ptr = (*param).u.data as *mut CustomGSU<T>;
        if let Some(unset) = &(*ptr).unset {
            unset(explicit > 0);
            drop(Box::from_raw(ptr));
        }
    }
}

pub trait VariableGSU {
    const FLAG: u32;
    type Type;

    fn from_raw(param: zsh_sys::Param, value: Self::Type) -> Self;
    fn into_raw(self) -> Self::Type;
}

impl VariableGSU for BString {
    const FLAG: u32 = zsh_sys::PM_SCALAR;
    type Type = *mut c_char;

    fn from_raw(param: zsh_sys::Param, ptr: Self::Type) -> Self {
        unsafe {
            let string = MetaString::from_raw(ptr).unmetafy();
            // zsh may try to strlen afterwards but we will have already freed the pointer
            (*param).width = string.len() as _;
            string
        }
    }

    fn into_raw(self) -> Self::Type {
        MetaString::from(self).into_raw()
    }
}

impl VariableGSU for c_long {
    const FLAG: u32 = zsh_sys::PM_INTEGER;
    type Type = c_long;

    fn from_raw(_param: zsh_sys::Param, value: Self::Type) -> Self {
        value
    }
    fn into_raw(self) -> Self::Type {
        self
    }
}

impl VariableGSU for f64 {
    const FLAG: u32 = zsh_sys::PM_FFLOAT;
    type Type = f64;

    fn from_raw(_param: zsh_sys::Param, value: Self::Type) -> Self {
        value
    }
    fn into_raw(self) -> Self::Type {
        self
    }
}

impl VariableGSU for Vec<BString> {
    const FLAG: u32 = zsh_sys::PM_ARRAY;
    type Type = *mut *mut c_char;

    fn from_raw(_param: zsh_sys::Param, ptr: Self::Type) -> Self {
        unsafe{
            let result = MetaArray::iter_ptr(ptr.cast()).map(|x| x.unmetafy().into_owned()).collect();
            zsh_sys::freearray(ptr);
            result
        }
    }

    fn into_raw(self) -> Self::Type {
        self.into_iter()
            .map(|x| x.into())
            .collect::<MetaArray>()
            .into_raw()
    }
}

impl VariableGSU for HashMap<BString, BString> {
    const FLAG: u32 = zsh_sys::PM_HASHED;
    type Type = zsh_sys::HashTable;

    fn from_raw(_param: zsh_sys::Param, ptr: Self::Type) -> Self {
        let map = try_hashtable_to_hashmap(ptr);
        unsafe {
            zsh_sys::deleteparamtable(ptr);
        }
        map.unwrap()
    }
    fn into_raw(self) -> Self::Type {
        unsafe {
            // why 17???
            let table = zsh_sys::newparamtable(17, std::ptr::null_mut());
            let old_paramtab = zsh_sys::paramtab;
            zsh_sys::paramtab = table;

            let mut value: zsh_sys::value = std::mem::MaybeUninit::zeroed().assume_init();
            value.end = -1;

            for (k, v) in self {
                let k = k.into_raw();
                value.pm = zsh_sys::createparam(k, (zsh_sys::PM_SCALAR | zsh_sys::PM_UNSET) as _);
                if value.pm.is_null() {
                    value.pm = ((*zsh_sys::paramtab).getnode).unwrap()(zsh_sys::paramtab, k).cast();
                }
                zsh_sys::assignstrvalue(&raw mut value, v.into_raw(), 0);
            }

            zsh_sys::paramtab = old_paramtab;
            table
        }
    }
}
