pub fn init() {
    // init logging
    let log_file = Box::new(std::fs::File::create("/tmp/wish.log").expect("Can't create log file"));
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(log_file))
        .format_source_path(true)
        .format_timestamp_millis()
        .init();
}
