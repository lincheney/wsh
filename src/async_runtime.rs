pub fn spawn_and_log<F, T, E>(future: F) -> tokio::task::JoinHandle<()> where
    F: Future<Output = Result<T, E>> + 'static,
    E: std::fmt::Debug,
{
    tokio::task::spawn_local(async move {
        crate::log_if_err(future.await);
    })
}

pub struct Runtime {
    runtime: tokio::runtime::Runtime,
    localset: tokio::task::LocalSet,
}

impl Runtime {
    pub fn new() -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let localset = tokio::task::LocalSet::new();
        Ok(Self{
            runtime,
            localset,
        })
    }

    pub fn block_on<'a, F: 'a + Future>(&'a self, future: F) -> F::Output {
        self.localset.block_on(&self.runtime, future)
    }

    pub fn weak_block_on<'a, F: 'a + Future>(&'a self, future: F) -> F::Output {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let guard = handle.enter();
            let result = futures::executor::block_on(future);
            drop(guard);
            result
        } else {
            self.block_on(future)
        }
    }

    pub fn spawn_local<F: 'static + Future>(&self, future: F) -> tokio::task::JoinHandle<F::Output> {
        self.localset.spawn_local(future)
    }

}
