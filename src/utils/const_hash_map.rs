use std::collections::HashMap;
use std::hash::{BuildHasherDefault, DefaultHasher};

#[derive(Default, Debug)]
pub struct ConstHashMap<K, V> {
    inner: HashMap<K, V, BuildHasherDefault<DefaultHasher>>,
}

impl<K, V> ConstHashMap<K, V> {
    pub const fn new() -> Self {
        Self {
            inner: HashMap::with_hasher(BuildHasherDefault::new()),
        }
    }
}
crate::impl_deref_helper!(self: ConstHashMap<K, V>, &self.inner => HashMap<K, V, BuildHasherDefault<DefaultHasher>>);
crate::impl_deref_helper!(mut self: ConstHashMap<K, V>, &mut self.inner => HashMap<K, V, BuildHasherDefault<DefaultHasher>>);
