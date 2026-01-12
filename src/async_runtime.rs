pub fn init() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .on_thread_start(|| {
            // try to make the main thread handle all signals
            if let Err(err) = crate::signals::disable_all_signals() {
                // mmmm pretty bad
                log::error!("{:?}", err);
            }
        })
        .build()
}

pub fn spawn_and_log<F, T, E>(future: F) -> tokio::task::JoinHandle<()> where
    F: Future<Output = Result<T, E>> + Send + 'static,
    E: std::fmt::Debug,
{
    tokio::task::spawn(async move {
        crate::log_if_err(future.await);
    })
}
