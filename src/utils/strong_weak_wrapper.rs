#[macro_export]
macro_rules! strong_weak_wrapper {
    ($struct_vis:vis struct $name:ident { $( $vis:vis $field:ident: $type:ty,)* }) => (
        paste::paste! {

            #[derive(Clone)]
            $struct_vis struct $name {
                $($vis $field: std::sync::Arc<$type>,)*
            }

            #[derive(Clone)]
            $struct_vis struct [<Weak $name>] {
                $($vis $field: std::sync::Weak<$type>,)*
            }

            #[allow(dead_code)]
            $struct_vis trait [<Downgrade $name>] {
                fn downgrade(&self) -> [<Weak $name>];
            }

            impl [<Downgrade $name>] for $name {
                fn downgrade(&self) -> [<Weak $name>] {
                    [<Weak $name>] {
                        $($field: std::sync::Arc::downgrade(&self.$field),)*
                    }
                }
            }

            #[allow(dead_code)]
            $struct_vis trait [<Upgrade $name>] {
                fn upgrade(&self) -> Option<$name>;
            }
            impl [<Upgrade $name>] for [<Weak $name>] {
                fn upgrade(&self) -> Option<$name> {
                    Some($name {
                        $($field: std::sync::Weak::upgrade(&self.$field)?,)*
                    })
                }
            }

        }
    )
}
