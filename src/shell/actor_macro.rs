#[macro_export]
macro_rules! TokioActor {
    (impl $name:ident { $(
        pub $(async)? fn $fn:ident(&$self:ident $(, $arg:ident: $argtype:ty)*) $(-> $rettype:ty)? $body:block
    )* }) => (

        paste::paste! {

            type X<T=()> = T;

            #[allow(non_camel_case_types)]
            #[allow(dead_code)]
            pub enum [<$name Msg>] { $(
                $fn{$( $arg: $argtype, )* returnvalue: ::tokio::sync::oneshot::Sender<X$(<$rettype>)?>},
            )*}

            impl $name {
                $(
                pub fn $fn(&$self, $($arg: $argtype),*) $(-> $rettype)? $body
                )*

                pub fn handle_one_message(&self, msg: [<$name Msg>]) {
                    match msg { $(
                        [<$name Msg>]::$fn{$( $arg, )* returnvalue} => {
                            let _ = returnvalue.send(self.$fn($( $arg, )*));
                        },
                    )* }
                }

            }

            pub struct [<$name Client>] {
                queue: ::std::sync::mpsc::Sender<[<$name Msg>]>,
                inner: $name,
            }

            #[allow(dead_code)]
            impl [<$name Client>] {

                pub async fn do_run<T: 'static + Send, F: 'static + Sync + Send + Fn(&$name) -> T>(&self, func: F) -> T {
                    *self.run(Box::new(move |shell| Box::new(func(shell)))).await.downcast().unwrap()
                }

                $(
                pub async fn $fn(&self, $($arg: $argtype),*) $(-> $rettype)? {
                    let thread = ::std::thread::current().id();
                    if thread == self.inner.main_thread {
                        tokio::task::block_in_place(|| self.inner.$fn($($arg),*))
                    } else {
                        let (sender, receiver) = ::tokio::sync::oneshot::channel();
                        self.queue.send([<$name Msg>]::$fn{$( $arg, )* returnvalue: sender}).unwrap();
                        receiver.await.unwrap()
                    }
                }
                )*
            }

        }

    );
}
