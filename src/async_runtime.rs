pub fn init() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

pub fn spawn_and_log<F, T, E>(future: F) -> tokio::task::JoinHandle<()> where
    F: Future<Output = Result<T, E>> + 'static,
    E: std::fmt::Debug,
{
    tokio::task::spawn_local(async move {
        crate::log_if_err(future.await);
    })
}
