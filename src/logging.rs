pub fn init() {
    // init logging
    let log_file = Box::new(std::fs::File::create("/tmp/wish.log").expect("Can't create log file"));
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(log_file))
        .format_source_path(true)
        .format_timestamp_millis()
        .try_init().ok();
}

pub fn log_if_err<T, E: std::fmt::Debug>(result: Result<T, E>) -> Option<T> {
    match result {
        Ok(x) => Some(x),
        Err(e) => {
            log::error!("{e:?}");
            None
        },
    }
}
