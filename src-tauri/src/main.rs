fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--beblog-guided-simulator")
    {
        if let Err(error) = beblog_digitizer_lib::run_guided_simulator(&arguments[1..]) {
            eprintln!("synthetic client simulator failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    beblog_digitizer_lib::run();
}
