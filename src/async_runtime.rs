use std::cell::{RefCell, Ref};
use anyhow::Result;

pub fn spawn_and_log<F, T, E>(ui: &crate::ui::Ui, future: F) -> Option<tokio::task::JoinHandle<()>>
where
    F: Future<Output = Result<T, E>> + 'static,
    E: std::fmt::Debug,
{
    crate::log_if_err(ui.runtime.spawn_local(async move {
        crate::log_if_err(future.await);
    }))
}

pub struct Runtime {
    inner: RefCell<Option<RuntimeInner>>,
}

struct RuntimeInner {
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
            inner: RefCell::new(Some(RuntimeInner{
                runtime,
                localset,
            }))
        })
    }

    fn try_get_inner(&self) -> Result<Ref<'_, RuntimeInner>> {
        self.inner.try_borrow().ok()
            .and_then(|inner| Ref::filter_map(inner, |inner| inner.as_ref()).ok())
            .ok_or_else(|| anyhow::anyhow!("runtime is not active"))
    }

    pub fn block_on<'a, F: 'a + Future>(&'a self, future: F) -> Result<F::Output> {
        let inner = self.try_get_inner()?;
        Ok(inner.localset.block_on(&inner.runtime, future))
    }

    pub fn weak_block_on<'a, F: 'a + Future>(&'a self, future: F) -> Result<F::Output> {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let guard = handle.enter();
            let result = futures::executor::block_on(future);
            drop(guard);
            Ok(result)
        } else {
            self.block_on(future)
        }
    }

    pub fn spawn_local<F: 'static + Future>(&self, future: F) -> Result<tokio::task::JoinHandle<F::Output>> {
        Ok(self.try_get_inner()?.localset.spawn_local(future))
    }

    pub fn enter<T, F: FnOnce() -> T>(&self, f: F) -> Result<T> {
        let inner = self.try_get_inner()?;
        let _guard = inner.runtime.enter();
        Ok(f())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.inner.try_borrow_mut()?.take();
        Ok(())
    }

}
