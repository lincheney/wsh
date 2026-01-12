#[macro_export]
macro_rules! impl_deref_helper {
    ($arg:ident: $struct:ident $(< $($generics:tt),* >)?, $inner:expr => $type:ty) => (
        impl$(<$($generics),*>)? ::std::ops::Deref for $struct $(<$($generics),*>)? {
            type Target = $type;
            fn deref(&$arg) -> &Self::Target {
                $inner
            }
        }
    );
    (mut $arg:ident: $struct:ident $(< $($generics:tt),* >)?, $inner:expr => $type:ty) => (
        impl$(<$($generics),*>)? ::std::ops::DerefMut for $struct $(<$($generics),*>)? {
            fn deref_mut(&mut $arg) -> &mut $type {
                $inner
            }
        }
    );
}
