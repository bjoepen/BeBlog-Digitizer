//! Hardware-free OS process runner used only by this crate's tests.
//!
//! The executable is always `current_exe()` and arguments/environment are
//! fixed here. There is no public path, shell, or arbitrary argument surface.

use super::{ProcessEvent, ProcessOutcome, ProcessRequest, ProcessRunner};
use crate::{log_parser::LogStream, sidecar::SidecarResolver};
use std::{
    io::{Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, SyncSender},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_LINE_BYTES: usize = 16 * 1024;
const READER_QUEUE_CAPACITY: usize = 32;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERMINATION_GRACE: Duration = Duration::from_millis(250);

static PROCESS_EXCLUSIVE: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
enum SafeTestProgram {
    SpawnFailure,
    Streams,
    Flood,
    Silent,
    CloseStreams,
    InvalidUtf8,
    NoFinalNewline,
    LongLine,
    ExitFailure,
    SelfSignal,
    WaitForTermination,
    IgnoreTermination,
    CloseStreamsIgnoreTermination,
}

impl SafeTestProgram {
    fn name(self) -> &'static str {
        match self {
            Self::SpawnFailure => "beblog-safe-test:spawn-failure",
            Self::Streams => "beblog-safe-test:streams",
            Self::Flood => "beblog-safe-test:flood",
            Self::Silent => "beblog-safe-test:silent",
            Self::CloseStreams => "beblog-safe-test:close-streams",
            Self::InvalidUtf8 => "beblog-safe-test:invalid-utf8",
            Self::NoFinalNewline => "beblog-safe-test:no-final-newline",
            Self::LongLine => "beblog-safe-test:long-line",
            Self::ExitFailure => "beblog-safe-test:exit-failure",
            Self::SelfSignal => "beblog-safe-test:self-signal",
            Self::WaitForTermination => "beblog-safe-test:wait-for-termination",
            Self::IgnoreTermination => "beblog-safe-test:ignore-termination",
            Self::CloseStreamsIgnoreTermination => {
                "beblog-safe-test:close-streams-ignore-termination"
            }
        }
    }

    fn test_filter(self) -> Option<&'static str> {
        match self {
            Self::SpawnFailure => None,
            Self::Streams => Some("process::os_process_runner::tests::child_streams"),
            Self::Flood => Some("process::os_process_runner::tests::child_flood"),
            Self::Silent => Some("process::os_process_runner::tests::child_silent"),
            Self::CloseStreams => Some("process::os_process_runner::tests::child_close_streams"),
            Self::InvalidUtf8 => Some("process::os_process_runner::tests::child_invalid_utf8"),
            Self::NoFinalNewline => {
                Some("process::os_process_runner::tests::child_no_final_newline")
            }
            Self::LongLine => Some("process::os_process_runner::tests::child_long_line"),
            Self::ExitFailure => Some("process::os_process_runner::tests::child_exit_failure"),
            Self::SelfSignal => Some("process::os_process_runner::tests::child_self_signal"),
            Self::WaitForTermination => {
                Some("process::os_process_runner::tests::child_wait_for_termination")
            }
            Self::IgnoreTermination => {
                Some("process::os_process_runner::tests::child_ignore_termination")
            }
            Self::CloseStreamsIgnoreTermination => {
                Some("process::os_process_runner::tests::child_close_streams_ignore_termination")
            }
        }
    }
}

/// This type has no public constructor or caller-provided executable path.
pub(super) struct OsProcessRunner {
    program: SafeTestProgram,
}

impl OsProcessRunner {
    fn new(program: SafeTestProgram) -> Self {
        Self { program }
    }
}

enum ReaderMessage {
    Line(LogStream, String),
    Closed(LogStream),
    Failed(LogStream, String),
}

#[derive(Default)]
struct StreamState {
    stdout_closed: bool,
    stderr_closed: bool,
}

impl StreamState {
    fn closed_mut(&mut self, stream: LogStream) -> &mut bool {
        match stream {
            LogStream::Stdout => &mut self.stdout_closed,
            LogStream::Stderr => &mut self.stderr_closed,
        }
    }

    fn all_closed(&self) -> bool {
        self.stdout_closed && self.stderr_closed
    }
}

impl ProcessRunner for OsProcessRunner {
    fn run(
        &self,
        request: &ProcessRequest,
        cancelled: Arc<AtomicBool>,
        emit: &mut dyn FnMut(ProcessEvent),
    ) -> ProcessOutcome {
        if cancelled.load(Ordering::SeqCst) {
            emit(ProcessEvent::Cancelled);
            return ProcessOutcome::Cancelled;
        }

        // Blocking acquisition serializes runs. It is held until wait/reap and both
        // stream-reader threads have finished, including all error paths below.
        let exclusive = PROCESS_EXCLUSIVE.get_or_init(|| Mutex::new(()));
        let _exclusive_guard = match exclusive.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if cancelled.load(Ordering::SeqCst) {
            emit(ProcessEvent::Cancelled);
            return ProcessOutcome::Cancelled;
        }

        let executable = if matches!(self.program, SafeTestProgram::SpawnFailure) {
            // This reserved path is used only to exercise OS spawn failure. Never
            // start it if the sentinel unexpectedly exists.
            let path = std::path::PathBuf::from("/__beblog_digitizer_d3_b2_missing_test_program__");
            if path.exists() {
                let message = "reservierter Spawnfehler-Testpfad existiert unerwartet".to_string();
                emit(ProcessEvent::StartFailed(message.clone()));
                return ProcessOutcome::StartFailed(message);
            }
            path
        } else {
            match SidecarResolver::resolve_test_harness() {
                Ok(resolved) => resolved.path().to_path_buf(),
                Err(error) => {
                    let message = format!("sicheres Testprogramm nicht verfügbar: {error}");
                    emit(ProcessEvent::StartFailed(message.clone()));
                    return ProcessOutcome::StartFailed(message);
                }
            }
        };
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(test_filter) = self.program.test_filter() {
            command
                .arg("--ignored")
                .arg("--exact")
                .arg(test_filter)
                .arg("--quiet");
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        // `request` controls only the timeout. Frame and simulation are deliberately
        // not translated into executable arguments or environment variables.
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let message = format!("sicheres Testprogramm konnte nicht starten: {error}");
                emit(ProcessEvent::StartFailed(message.clone()));
                return ProcessOutcome::StartFailed(message);
            }
        };
        let pid = child.id();
        emit(ProcessEvent::Started {
            program: self.program.name(),
            pid: Some(pid),
        });

        let (sender, receiver) = sync_channel(READER_QUEUE_CAPACITY);
        let stdout_thread = match child.stdout.take() {
            Some(stdout) => spawn_reader(stdout, LogStream::Stdout, sender.clone()),
            None => Err("stdout pipe was not created".to_string()),
        };
        let stderr_thread = match child.stderr.take() {
            Some(stderr) => spawn_reader(stderr, LogStream::Stderr, sender),
            None => Err("stderr pipe was not created".to_string()),
        };
        let mut reader_threads = Vec::new();
        let mut cleanup_errors = Vec::new();
        for thread in [stdout_thread, stderr_thread] {
            match thread {
                Ok(handle) => reader_threads.push(handle),
                Err(error) => cleanup_errors.push(error),
            }
        }
        if !cleanup_errors.is_empty() {
            terminate_immediately(&mut child, &mut cleanup_errors);
        }

        let started_at = Instant::now();
        let deadline = started_at
            .checked_add(request.timeout)
            .unwrap_or(started_at);
        let mut streams = StreamState::default();
        let mut observed_status: Option<ExitStatus> = None;
        let mut terminal_reason: Option<ProcessOutcome> = None;
        let mut termination_started: Option<Instant> = None;
        let mut hard_kill_sent = false;
        let mut forced_kill = false;
        let mut reader_failure = None;

        while observed_status.is_none() || !streams.all_closed() {
            if observed_status.is_none() {
                match child.try_wait() {
                    Ok(Some(status)) => observed_status = Some(status),
                    Ok(None) => {}
                    Err(error) => {
                        reader_failure
                            .get_or_insert_with(|| format!("Prozessstatus nicht lesbar: {error}"));
                    }
                }
            }

            if terminal_reason.is_none() && (observed_status.is_none() || !streams.all_closed()) {
                if cancelled.load(Ordering::SeqCst) {
                    terminal_reason = Some(ProcessOutcome::Cancelled);
                } else if Instant::now() >= deadline {
                    terminal_reason = Some(ProcessOutcome::TimedOut);
                }
                if terminal_reason.is_some() {
                    termination_started = Some(Instant::now());
                    if let Err(error) = request_graceful_termination(pid, &mut child) {
                        cleanup_errors.push(error);
                    }
                }
            }

            if let Some(reason) = &terminal_reason {
                let grace_elapsed =
                    termination_started.is_some_and(|when| when.elapsed() >= TERMINATION_GRACE);
                // Pipe EOF is independent of process lifetime: a child can close
                // both descriptors and continue running. Escalate until OS exit
                // has actually been observed, even when both readers are done.
                if observed_status.is_none() && grace_elapsed && !hard_kill_sent {
                    hard_kill_sent = true;
                    forced_kill = true;
                    emit(ProcessEvent::ForcedKill);
                    if let Err(error) = force_kill_process_group(pid, &mut child) {
                        cleanup_errors.push(format!(
                            "erzwungener Prozessabbruch fehlgeschlagen: {error}"
                        ));
                    }
                }
                let _ = reason;
            }

            match receiver.recv_timeout(POLL_INTERVAL) {
                Ok(ReaderMessage::Line(LogStream::Stdout, line)) => {
                    emit(ProcessEvent::Stdout(line));
                }
                Ok(ReaderMessage::Line(LogStream::Stderr, line)) => {
                    emit(ProcessEvent::Stderr(line));
                }
                Ok(ReaderMessage::Closed(stream)) => {
                    let closed = streams.closed_mut(stream);
                    if !*closed {
                        *closed = true;
                        emit(match stream {
                            LogStream::Stdout => ProcessEvent::StdoutClosed,
                            LogStream::Stderr => ProcessEvent::StderrClosed,
                        });
                    }
                }
                Ok(ReaderMessage::Failed(stream, error)) => {
                    reader_failure
                        .get_or_insert_with(|| format!("{stream:?} lesen fehlgeschlagen: {error}"));
                    let closed = streams.closed_mut(stream);
                    if !*closed {
                        *closed = true;
                        emit(match stream {
                            LogStream::Stdout => ProcessEvent::StdoutClosed,
                            LogStream::Stderr => ProcessEvent::StderrClosed,
                        });
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // A reader exited without sending its closure/error message. Do not
                    // block forever in Child::wait(): turn it into a protocol failure,
                    // terminate the process group, and continue polling/reaping.
                    if !streams.all_closed() {
                        let message = "Stream-Reader endete ohne Abschlussmeldung".to_string();
                        reader_failure.get_or_insert_with(|| message.clone());
                        if terminal_reason.is_none() {
                            terminal_reason = Some(ProcessOutcome::UnexpectedEnd(message));
                            termination_started = Some(Instant::now());
                            if let Err(error) = request_graceful_termination(pid, &mut child) {
                                cleanup_errors.push(error);
                            }
                        }
                        for stream in [LogStream::Stdout, LogStream::Stderr] {
                            let closed = streams.closed_mut(stream);
                            if !*closed {
                                *closed = true;
                                emit(match stream {
                                    LogStream::Stdout => ProcessEvent::StdoutClosed,
                                    LogStream::Stderr => ProcessEvent::StderrClosed,
                                });
                            }
                        }
                    } else {
                        thread::sleep(POLL_INTERVAL);
                    }
                }
            }

            if reader_failure.is_some() && terminal_reason.is_none() {
                terminal_reason = Some(ProcessOutcome::UnexpectedEnd(
                    reader_failure.clone().unwrap_or_default(),
                ));
                termination_started = Some(Instant::now());
                if let Err(error) = request_graceful_termination(pid, &mut child) {
                    cleanup_errors.push(error);
                }
            }
        }

        // Explicit wait is the final reap attempt even when try_wait already observed
        // the exit status. Rust caches a status returned by try_wait for this purpose.
        let waited_status = match child.wait() {
            Ok(status) => Some(status),
            Err(error) => {
                cleanup_errors.push(format!("Childprozess konnte nicht gereapt werden: {error}"));
                None
            }
        };
        for handle in reader_threads {
            if handle.join().is_err() {
                cleanup_errors.push("Stream-Reader-Thread ist abgestürzt".into());
            }
        }

        // A panic or failed reader startup can leave one stream without a close event.
        if !streams.stdout_closed {
            emit(ProcessEvent::StdoutClosed);
        }
        if !streams.stderr_closed {
            emit(ProcessEvent::StderrClosed);
        }

        // On Unix (including macOS), Rust documents that try_wait returning
        // Some(status) has already reaped the PID. Keep that OS-level proof for
        // the event even if the subsequent explicit wait reports an error; the
        // wait error above still makes cleanup fail and can never emit CleanedUp.
        #[cfg(unix)]
        let status = waited_status.or(observed_status);
        #[cfg(not(unix))]
        let status = waited_status;
        if let Some(status) = status.as_ref() {
            emit(ProcessEvent::Reaped {
                code: status.code(),
            });
        }
        let outcome = if let Some(reason) = terminal_reason {
            reason
        } else if let Some(error) = reader_failure {
            ProcessOutcome::UnexpectedEnd(error)
        } else if let Some(status) = status {
            process_outcome(status)
        } else {
            ProcessOutcome::UnexpectedEnd("kein Prozessabschlussstatus beobachtet".into())
        };

        if forced_kill
            && !matches!(
                outcome,
                ProcessOutcome::Cancelled | ProcessOutcome::TimedOut
            )
        {
            cleanup_errors
                .push("Prozess wurde außerhalb eines Cancel-/Timeoutpfads hart beendet".into());
        }

        match &outcome {
            ProcessOutcome::Exited(code) => emit(ProcessEvent::Exited { code: *code }),
            ProcessOutcome::Cancelled => emit(ProcessEvent::Cancelled),
            ProcessOutcome::TimedOut => emit(ProcessEvent::TimedOut),
            ProcessOutcome::UnexpectedEnd(message) => {
                emit(ProcessEvent::UnexpectedEnd(message.clone()))
            }
            ProcessOutcome::StartFailed(_) | ProcessOutcome::CleanupFailed(_) => {}
        }

        if cleanup_errors.is_empty() {
            emit(ProcessEvent::CleanedUp);
            outcome
        } else {
            let message = cleanup_errors.join("; ");
            emit(ProcessEvent::CleanupFailed(message.clone()));
            ProcessOutcome::CleanupFailed(message)
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: LogStream,
    sender: SyncSender<ReaderMessage>,
) -> Result<thread::JoinHandle<()>, String> {
    thread::Builder::new()
        .name(format!("beblog-{stream:?}-reader"))
        .spawn(move || {
            let mut line = Vec::with_capacity(MAX_LINE_BYTES);
            let mut truncated = false;
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        if !line.is_empty() || truncated {
                            send_line(&sender, stream, &line, truncated);
                        }
                        let _ = sender.send(ReaderMessage::Closed(stream));
                        break;
                    }
                    Ok(count) => {
                        for byte in &buffer[..count] {
                            if *byte == b'\n' {
                                send_line(&sender, stream, &line, truncated);
                                line.clear();
                                truncated = false;
                            } else if line.len() < MAX_LINE_BYTES {
                                line.push(*byte);
                            } else {
                                truncated = true;
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = sender.send(ReaderMessage::Failed(stream, error.to_string()));
                        break;
                    }
                }
            }
        })
        .map_err(|error| format!("Stream-Reader konnte nicht gestartet werden: {error}"))
}

fn send_line(sender: &SyncSender<ReaderMessage>, stream: LogStream, bytes: &[u8], truncated: bool) {
    let mut line = String::from_utf8_lossy(bytes).into_owned();
    if truncated {
        line.push_str(" … [diagnostic truncated at 16384 bytes]");
    }
    // The allowlisted executable is a Rust test harness. Hide only its fixed
    // reporter records, not bytes written by the selected child mode.
    if line == "running 1 test"
        || line.starts_with("test process::os_process_runner::tests::child_")
        || line.starts_with("test result: ok. 1 passed")
    {
        return;
    }
    let _ = sender.send(ReaderMessage::Line(stream, line));
}

fn process_outcome(status: ExitStatus) -> ProcessOutcome {
    if let Some(code) = status.code() {
        return ProcessOutcome::Exited(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ProcessOutcome::UnexpectedEnd(format!(
            "Prozess durch Betriebssystemsignal beendet: {:?}",
            status.signal()
        ))
    }
    #[cfg(not(unix))]
    {
        ProcessOutcome::UnexpectedEnd("Prozess endete ohne numerischen Exitstatus".to_string())
    }
}

fn terminate_immediately(child: &mut Child, errors: &mut Vec<String>) {
    if let Err(error) = child.kill() {
        errors.push(format!(
            "Prozess nach Reader-Startfehler nicht beendbar: {error}"
        ));
    }
    if let Err(error) = child.wait() {
        errors.push(format!(
            "Prozess nach Reader-Startfehler nicht reapbar: {error}"
        ));
    }
}

#[cfg(unix)]
fn request_graceful_termination(pid: u32, _child: &mut Child) -> Result<(), String> {
    // SAFETY: `pid` is returned by the Child just spawned by this runner, and the
    // child was placed into its own group. SIGTERM is limited to that group; no
    // command string or caller-provided PID is used.
    let result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(format!("SIGTERM an Testprozess fehlgeschlagen: {error}"))
        }
    }
}

#[cfg(unix)]
fn force_kill_process_group(pid: u32, child: &mut Child) -> Result<(), String> {
    // SAFETY: this is the dedicated process group created for this runner's child.
    let result = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        let group_error = std::io::Error::last_os_error();
        child.kill().map_err(|child_error| {
            format!("SIGKILL der Testprozessgruppe fehlgeschlagen ({group_error}); Child::kill fehlgeschlagen: {child_error}")
        })
    }
}

#[cfg(not(unix))]
fn request_graceful_termination(_pid: u32, child: &mut Child) -> Result<(), String> {
    // std has no portable graceful-signal API; on non-Unix test hosts termination
    // is immediate and is reported as forced cleanup.
    child
        .kill()
        .map_err(|error| format!("Testprozess konnte nicht beendet werden: {error}"))
}

#[cfg(not(unix))]
fn force_kill_process_group(_pid: u32, child: &mut Child) -> Result<(), String> {
    child
        .kill()
        .map_err(|error| format!("Testprozess konnte nicht hart beendet werden: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };

    static TERMINATE_REQUESTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_sigterm(_: libc::c_int) {
        TERMINATE_REQUESTED.store(true, Ordering::SeqCst);
    }

    fn run_child_mode(mode: &str, expected_filter: &str) {
        let args = std::env::args().collect::<Vec<_>>();
        let explicitly_allowlisted = args.iter().any(|arg| arg == "--ignored")
            && args
                .windows(2)
                .any(|pair| pair[0] == "--exact" && pair[1] == expected_filter);
        if !explicitly_allowlisted {
            // Ignored child entries are inert when cargo runs the normal test suite,
            // including broad `--include-ignored` invocations.
            return;
        }
        match mode {
            "streams" => {
                std::io::stdout().write_all(b"stdout diagnostic\n").unwrap();
                std::io::stderr().write_all(b"stderr diagnostic\n").unwrap();
            }
            "flood" => {
                let block = vec![b'x'; 4096];
                let mut stdout = std::io::stdout().lock();
                let mut stderr = std::io::stderr().lock();
                for _ in 0..128 {
                    stdout.write_all(&block).unwrap();
                    stdout.write_all(b"\n").unwrap();
                    stderr.write_all(&block).unwrap();
                    stderr.write_all(b"\n").unwrap();
                }
                stdout.flush().unwrap();
                stderr.flush().unwrap();
            }
            "silent" => thread::sleep(Duration::from_millis(80)),
            "close-streams" => {
                // SAFETY: this runs only inside the allowlisted test child process.
                unsafe {
                    libc::close(libc::STDOUT_FILENO);
                    libc::close(libc::STDERR_FILENO);
                }
                thread::sleep(Duration::from_millis(50));
            }
            "invalid-utf8" => {
                std::io::stdout()
                    .write_all(&[0xf0, 0x28, 0x8c, 0x28, b'\n'])
                    .unwrap();
            }
            "no-final-newline" => {
                std::io::stdout()
                    .write_all(b"last line without newline")
                    .unwrap();
            }
            "long-line" => {
                std::io::stdout()
                    .write_all(&vec![b'z'; MAX_LINE_BYTES + 2048])
                    .unwrap();
                std::io::stdout().write_all(b"\n").unwrap();
            }
            "exit-failure" => std::process::exit(7),
            "self-signal" => {
                // SAFETY: this harmless test process signals itself only.
                unsafe {
                    libc::raise(libc::SIGTERM);
                }
                std::process::exit(8);
            }
            "wait-for-termination" => {
                #[cfg(unix)]
                unsafe {
                    libc::signal(libc::SIGTERM, on_sigterm as *const () as libc::sighandler_t);
                }
                std::io::stdout().write_all(b"READY\n").unwrap();
                while !TERMINATE_REQUESTED.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(5));
                }
            }
            "ignore-termination" => {
                #[cfg(unix)]
                unsafe {
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                }
                loop {
                    thread::sleep(Duration::from_secs(1));
                }
            }
            "close-streams-ignore-termination" => {
                #[cfg(unix)]
                unsafe {
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                    libc::close(libc::STDOUT_FILENO);
                    libc::close(libc::STDERR_FILENO);
                }
                loop {
                    thread::sleep(Duration::from_secs(1));
                }
            }
            _ => panic!("unrecognized test child mode"),
        }
        // Bypass libtest's own summary output so every captured byte comes from
        // the explicitly selected safe test mode.
        std::process::exit(0);
    }

    macro_rules! child_mode_test {
        ($name:ident, $mode:literal) => {
            #[test]
            #[ignore = "launched only by the D3-B2 allowlisted OS process runner"]
            fn $name() {
                run_child_mode(
                    $mode,
                    concat!("process::os_process_runner::tests::", stringify!($name)),
                );
            }
        };
    }

    child_mode_test!(child_streams, "streams");
    child_mode_test!(child_flood, "flood");
    child_mode_test!(child_silent, "silent");
    child_mode_test!(child_close_streams, "close-streams");
    child_mode_test!(child_invalid_utf8, "invalid-utf8");
    child_mode_test!(child_no_final_newline, "no-final-newline");
    child_mode_test!(child_long_line, "long-line");
    child_mode_test!(child_exit_failure, "exit-failure");
    child_mode_test!(child_self_signal, "self-signal");
    child_mode_test!(child_wait_for_termination, "wait-for-termination");
    child_mode_test!(child_ignore_termination, "ignore-termination");
    child_mode_test!(
        child_close_streams_ignore_termination,
        "close-streams-ignore-termination"
    );

    fn run(
        program: SafeTestProgram,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    ) -> (ProcessOutcome, Vec<ProcessEvent>) {
        let request = ProcessRequest {
            frame: 1,
            simulation: crate::model::SimulationMode::Success,
            timeout,
        };
        let mut events = Vec::new();
        let outcome =
            OsProcessRunner::new(program).run(&request, cancel, &mut |event| events.push(event));
        (outcome, events)
    }

    #[test]
    fn captures_both_streams_pid_exit_and_closure_before_cleanup() {
        let (outcome, events) = run(
            SafeTestProgram::Streams,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(outcome, ProcessOutcome::Exited(0));
        assert!(events.iter().any(
            |event| matches!(event, ProcessEvent::Started { pid: Some(pid), .. } if *pid > 0)
        ));
        assert!(events.iter().any(|event| matches!(event, ProcessEvent::Stdout(line) if line.contains("stdout diagnostic"))));
        assert!(events.iter().any(|event| matches!(event, ProcessEvent::Stderr(line) if line.contains("stderr diagnostic"))));
        let stdout = events
            .iter()
            .position(|event| *event == ProcessEvent::StdoutClosed)
            .unwrap();
        let stderr = events
            .iter()
            .position(|event| *event == ProcessEvent::StderrClosed)
            .unwrap();
        let exit = events
            .iter()
            .position(|event| matches!(event, ProcessEvent::Exited { code: 0 }))
            .unwrap();
        let reaped = events
            .iter()
            .position(|event| matches!(event, ProcessEvent::Reaped { code: Some(0) }))
            .unwrap();
        let cleanup = events
            .iter()
            .position(|event| *event == ProcessEvent::CleanedUp)
            .unwrap();
        assert!(stdout < reaped && stderr < reaped && reaped < exit && exit < cleanup);
    }

    #[test]
    fn handles_large_simultaneous_output_without_deadlock_or_unbounded_lines() {
        let (outcome, events) = run(
            SafeTestProgram::Flood,
            Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(outcome, ProcessOutcome::Exited(0));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProcessEvent::Stdout(line) if !line.is_empty()))
                .count(),
            128,
            "unexpected stdout event: {events:#?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProcessEvent::Stderr(line) if !line.is_empty()))
                .count(),
            128
        );
    }

    #[test]
    fn preserves_invalid_utf8_as_replacement_and_final_unterminated_line() {
        let (_, invalid) = run(
            SafeTestProgram::InvalidUtf8,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(invalid
            .iter()
            .any(|event| matches!(event, ProcessEvent::Stdout(line) if line.contains('\u{fffd}'))));
        let (_, no_newline) = run(
            SafeTestProgram::NoFinalNewline,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(no_newline.iter().any(|event| matches!(event, ProcessEvent::Stdout(line) if line == "last line without newline")));
    }

    #[test]
    fn bounds_long_diagnostics_and_marks_truncation() {
        let (_, events) = run(
            SafeTestProgram::LongLine,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(events.iter().any(|event| matches!(event, ProcessEvent::Stdout(line) if line.contains("diagnostic truncated"))));
        assert!(events
            .iter()
            .filter_map(|event| match event {
                ProcessEvent::Stdout(line) => Some(line.len()),
                _ => None,
            })
            .all(|length| length < MAX_LINE_BYTES * 4));
    }

    #[test]
    fn reports_nonzero_exit_and_no_output_process() {
        let (start_failure, events) = run(
            SafeTestProgram::SpawnFailure,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(matches!(start_failure, ProcessOutcome::StartFailed(_)));
        assert!(matches!(events.as_slice(), [ProcessEvent::StartFailed(_)]));
        let (failure, _failure_events) = run(
            SafeTestProgram::ExitFailure,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(failure, ProcessOutcome::Exited(7));
        let (silent, events) = run(
            SafeTestProgram::Silent,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(silent, ProcessOutcome::Exited(0));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ProcessEvent::Stdout(line) | ProcessEvent::Stderr(line) if !line.is_empty())),
            "unexpected no-output event: {events:#?}"
        );

        let (closed, closed_events) = run(
            SafeTestProgram::CloseStreams,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(closed, ProcessOutcome::Exited(0));
        let stdout = closed_events
            .iter()
            .position(|event| *event == ProcessEvent::StdoutClosed)
            .unwrap();
        let stderr = closed_events
            .iter()
            .position(|event| *event == ProcessEvent::StderrClosed)
            .unwrap();
        let exit = closed_events
            .iter()
            .position(|event| matches!(event, ProcessEvent::Exited { code: 0 }))
            .unwrap();
        assert!(stdout < exit && stderr < exit);
    }

    #[test]
    fn unexpected_signal_end_is_not_misreported_as_regular_exit() {
        let (outcome, events) = run(
            SafeTestProgram::SelfSignal,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(
            matches!(outcome, ProcessOutcome::UnexpectedEnd(message) if message.contains("signal"))
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, ProcessEvent::UnexpectedEnd(message) if message.contains("signal"))));
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
    }

    #[test]
    fn cancellation_before_start_does_not_spawn_and_graceful_cancel_reaps() {
        let (before, events) = run(
            SafeTestProgram::WaitForTermination,
            Duration::from_secs(2),
            Arc::new(AtomicBool::new(true)),
        );
        assert_eq!(before, ProcessOutcome::Cancelled);
        assert!(!events
            .iter()
            .any(|event| matches!(event, ProcessEvent::Started { .. })));

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_from_test = cancel.clone();
        let child_ready = Arc::new(AtomicBool::new(false));
        let child_ready_from_test = child_ready.clone();
        let handle = thread::spawn(move || {
            let request = ProcessRequest {
                frame: 1,
                simulation: crate::model::SimulationMode::Success,
                timeout: Duration::from_secs(5),
            };
            let mut events = Vec::new();
            let outcome = OsProcessRunner::new(SafeTestProgram::WaitForTermination).run(
                &request,
                cancel_from_test,
                &mut |event| {
                    if matches!(&event, ProcessEvent::Stdout(line) if line == "READY") {
                        child_ready_from_test.store(true, Ordering::SeqCst);
                    }
                    events.push(event);
                },
            );
            (outcome, events)
        });
        let start_deadline = Instant::now() + Duration::from_secs(3);
        while !child_ready.load(Ordering::SeqCst) && Instant::now() < start_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            child_ready.load(Ordering::SeqCst),
            "child process did not install its termination handler"
        );
        cancel.store(true, Ordering::SeqCst);
        let (outcome, events) = handle.join().unwrap();
        assert_eq!(outcome, ProcessOutcome::Cancelled);
        assert!(!events.contains(&ProcessEvent::ForcedKill));
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
    }

    #[test]
    fn timeout_and_ignored_signal_force_kill_then_reap() {
        let (outcome, events) = run(
            SafeTestProgram::IgnoreTermination,
            Duration::from_millis(100),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(outcome, ProcessOutcome::TimedOut);
        assert!(events.contains(&ProcessEvent::ForcedKill));
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
        let pid = events
            .iter()
            .find_map(|event| match event {
                ProcessEvent::Started { pid, .. } => *pid,
                _ => None,
            })
            .unwrap();
        // A successfully reaped PID no longer exists in the OS process table.
        // SAFETY: PID came from this runner's child; signal 0 performs no delivery.
        assert_ne!(unsafe { libc::kill(pid as libc::pid_t, 0) }, 0);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_escalates_after_streams_close_if_child_keeps_running() {
        let started = Instant::now();
        let (outcome, events) = run(
            SafeTestProgram::CloseStreamsIgnoreTermination,
            Duration::from_millis(100),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(outcome, ProcessOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3), "runner hung");
        assert!(events.contains(&ProcessEvent::StdoutClosed));
        assert!(events.contains(&ProcessEvent::StderrClosed));
        let kill = events
            .iter()
            .position(|event| *event == ProcessEvent::ForcedKill)
            .expect("SIGKILL escalation should be reported");
        let reaped = events
            .iter()
            .position(|event| matches!(event, ProcessEvent::Reaped { .. }))
            .expect("killed child should be reaped");
        let cleanup = events
            .iter()
            .position(|event| *event == ProcessEvent::CleanedUp)
            .expect("readers and process resources should be cleaned up");
        assert!(kill < reaped && reaped < cleanup);
    }

    #[test]
    fn serialized_runner_does_not_start_second_process_concurrently() {
        // The global mutex serializes entry; both children complete and are reaped.
        let first = thread::spawn(|| {
            run(
                SafeTestProgram::Silent,
                Duration::from_secs(2),
                Arc::new(AtomicBool::new(false)),
            )
        });
        let second = thread::spawn(|| {
            run(
                SafeTestProgram::Streams,
                Duration::from_secs(2),
                Arc::new(AtomicBool::new(false)),
            )
        });
        assert_eq!(first.join().unwrap().0, ProcessOutcome::Exited(0));
        assert_eq!(second.join().unwrap().0, ProcessOutcome::Exited(0));
    }
}
