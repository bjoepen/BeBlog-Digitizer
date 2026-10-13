//! Controlled, fixed-resource Minolta client runner for the guided single-frame UI.
//!
//! The implementation is present, but the resolver intentionally fails closed
//! until the candidate client's digest is independently approved. No executable
//! path, shell fragment, or argument vector is accepted from the frontend.

use crate::{
    archive::ArchiveManager,
    guided_protocol::{parse_client_line, ClientEvent, GuidedProtocol},
    offline_import::{export_validated_artifacts, import_offline_artifacts, OfflineArtifactPaths},
    sidecar::resolve_approved_minolta_client,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const TERMINATION_GRACE: Duration = Duration::from_secs(15);
const HOLDER_TIMEOUT_SECONDS: u32 = 120;
const HOLDER_POLL_SECONDS: u32 = 2;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuidedScanRequest {
    pub frame: u8,
    pub dpi: u16,
    pub device: String,
    pub confirmed_device: String,
    pub output_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum GuidedUiEvent {
    Opened,
    OpenFailed {
        detail: String,
    },
    HolderDetected,
    FramePositioned {
        frame: u8,
    },
    Parameters {
        dpi: u16,
        width: u32,
        height: u32,
        bytes_per_line: u32,
        depth: u8,
        format: String,
        expected_bytes: u64,
    },
    ScanStarted,
    DataChunk {
        chunk_bytes: u32,
        total_bytes: u64,
    },
    Eof {
        total_bytes: u64,
    },
    TiffWritten,
    ScannerClosing,
    Error {
        detail: String,
    },
    Diagnostic {
        stream: &'static str,
        message: String,
    },
    ProcessExited {
        exit_code: i32,
    },
    ProcessCleaned,
    ProcessCancelled,
    ProcessTimedOut,
    ProcessForcedKill,
    ArtifactsExported {
        directory: String,
        capture_id: String,
    },
    ProcessFailed {
        detail: String,
    },
}

#[derive(Clone, Default)]
pub(crate) struct GuidedProcessController {
    active: Arc<Mutex<Option<ActiveSession>>>,
}

#[derive(Clone)]
struct ActiveSession {
    run_id: String,
    opened: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    control: Sender<ControlMessage>,
}

enum ControlMessage {
    Continue,
    Cancel,
}

impl GuidedProcessController {
    #[allow(clippy::too_many_arguments)] // Explicit dependencies keep the fixed process contract auditable.
    pub(crate) fn start(
        &self,
        app: AppHandle,
        resource_dir: PathBuf,
        work_root: PathBuf,
        archive: ArchiveManager,
        request: GuidedScanRequest,
        cancelled: Arc<AtomicBool>,
        on_finished: impl FnOnce(bool) + Send + 'static,
    ) -> Result<String, String> {
        validate_request(&request)?;
        let client =
            resolve_approved_minolta_client(&resource_dir).map_err(|error| error.to_string())?;
        let work_directory =
            reserve_work_directory(&work_root).map_err(|error| error.to_string())?;
        let prefix = work_directory.join("capture");
        let arguments = client_arguments(&request, &prefix);
        self.launch(
            app,
            client.path().to_path_buf(),
            arguments,
            work_directory,
            request,
            archive,
            cancelled,
            on_finished,
        )
    }

    pub(crate) fn start_simulation(
        &self,
        app: AppHandle,
        work_root: PathBuf,
        archive: ArchiveManager,
        request: GuidedScanRequest,
        cancelled: Arc<AtomicBool>,
        on_finished: impl FnOnce(bool) + Send + 'static,
    ) -> Result<String, String> {
        validate_request(&request)?;
        if request.device != "simulator:hardware-free" {
            return Err(
                "Interner Simulator akzeptiert ausschließlich die feste Gerätekennung".into(),
            );
        }
        let work_directory =
            reserve_work_directory(&work_root).map_err(|error| error.to_string())?;
        let arguments = vec![
            OsString::from("--beblog-guided-simulator"),
            OsString::from("--prefix"),
            work_directory.join("capture").into_os_string(),
            OsString::from("--frame"),
            OsString::from(request.frame.to_string()),
            OsString::from("--dpi"),
            OsString::from(request.dpi.to_string()),
        ];
        let executable = std::env::current_exe()
            .map_err(|error| format!("App-Prozesspfad nicht verfügbar: {error}"))?;
        self.launch(
            app,
            executable,
            arguments,
            work_directory,
            request,
            archive,
            cancelled,
            on_finished,
        )
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the bounded worker inputs, none originate as executable/UI paths.
    fn launch(
        &self,
        app: AppHandle,
        executable: PathBuf,
        arguments: Vec<OsString>,
        work_directory: PathBuf,
        request: GuidedScanRequest,
        archive: ArchiveManager,
        cancelled: Arc<AtomicBool>,
        on_finished: impl FnOnce(bool) + Send + 'static,
    ) -> Result<String, String> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let opened = Arc::new(AtomicBool::new(false));
        let (control_tx, control_rx) = mpsc::channel();
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "Interner Prozessstatusfehler")?;
            if active.is_some() {
                return Err("Ein Scannerprozess ist bereits aktiv".into());
            }
            *active = Some(ActiveSession {
                run_id: run_id.clone(),
                opened: opened.clone(),
                cancelled: cancelled.clone(),
                control: control_tx,
            });
        }
        let active = self.active.clone();
        let worker_run_id = run_id.clone();
        thread::spawn(move || {
            let result = run_client(
                app.clone(),
                executable,
                arguments,
                work_directory.clone(),
                request,
                archive,
                opened,
                cancelled,
                control_rx,
            );
            let succeeded = result.is_ok();
            if let Err(error) = result {
                emit(&app, GuidedUiEvent::ProcessFailed { detail: error });
            }
            on_finished(succeeded);
            if let Ok(mut active) = active.lock() {
                if active
                    .as_ref()
                    .is_some_and(|session| session.run_id == worker_run_id)
                {
                    *active = None;
                }
            }
        });
        Ok(run_id)
    }

    pub(crate) fn confirm_holder(&self) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "Interner Prozessstatusfehler")?;
        let session = active.as_ref().ok_or("Kein aktiver Scannerprozess")?;
        if !session.opened.load(Ordering::SeqCst) {
            return Err(
                "Filmhalter kann erst nach dem echten OPENED-Ereignis bestätigt werden".into(),
            );
        }
        session
            .control
            .send(ControlMessage::Continue)
            .map_err(|_| "Scannerprozess nimmt keine Fortsetzung mehr an".into())
    }

    pub(crate) fn cancel(&self) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "Interner Prozessstatusfehler")?;
        let session = active.as_ref().ok_or("Kein aktiver Scannerprozess")?;
        session.cancelled.store(true, Ordering::SeqCst);
        let _ = session.control.send(ControlMessage::Cancel);
        Ok(())
    }
}

fn validate_request(request: &GuidedScanRequest) -> Result<(), String> {
    if !(1..=4).contains(&request.frame) {
        return Err("D4 unterstützt zunächst nur Frames 1–4".into());
    }
    if !matches!(request.dpi, 600 | 2820) {
        return Err("Auflösung muss 600 oder 2.820 dpi sein".into());
    }
    if request.device.is_empty()
        || request.device.trim() != request.device
        || request.device.len() > 1024
        || request
            .device
            .chars()
            .any(|c| matches!(c, '\0' | '\n' | '\r'))
        || request.device != request.confirmed_device
    {
        return Err("Gerätebezeichner fehlt oder wurde nicht exakt bestätigt".into());
    }
    let metadata = fs::symlink_metadata(&request.output_directory)
        .map_err(|error| format!("Ausgabeordner ist nicht verfügbar: {error}"))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err("Ausgabeziel muss ein echtes Verzeichnis sein".into());
    }
    Ok(())
}

fn client_arguments(request: &GuidedScanRequest, prefix: &Path) -> Vec<OsString> {
    vec![
        "-d".into(),
        request.device.clone().into(),
        "-o".into(),
        prefix.as_os_str().to_owned(),
        "-f".into(),
        request.frame.to_string().into(),
        "-r".into(),
        request.dpi.to_string().into(),
        "-t".into(),
        HOLDER_TIMEOUT_SECONDS.to_string().into(),
        "-i".into(),
        HOLDER_POLL_SECONDS.to_string().into(),
    ]
}

fn reserve_work_directory(root: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Arbeitswurzel ist kein echtes Verzeichnis",
        ));
    }
    let root = fs::canonicalize(root)?;
    for _ in 0..16 {
        let directory = root.join(format!("guided-{}", uuid::Uuid::new_v4().simple()));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Kein exklusives Arbeitsverzeichnis verfügbar",
    ))
}

enum ReaderMessage {
    Line(bool, String), // true = stdout
    Closed(bool),
    Error(bool, String),
}

#[allow(clippy::too_many_arguments)] // Keeps AppHandle event delivery and each owned lifecycle resource explicit.
fn run_client(
    app: AppHandle,
    executable: PathBuf,
    arguments: Vec<OsString>,
    work_directory: PathBuf,
    request: GuidedScanRequest,
    archive: ArchiveManager,
    opened: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    controls: Receiver<ControlMessage>,
) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        emit(&app, GuidedUiEvent::ProcessCancelled);
        return Err("Scan wurde vor dem Prozessstart abgebrochen".into());
    }
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(&work_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Minolta-Client konnte nicht gestartet werden: {error}"))?;
    let pid = child.id();
    let mut stdin = child.stdin.take().ok_or("stdin-Handshakekanal fehlt")?;
    let stdout = child.stdout.take().ok_or("stdout-Ereigniskanal fehlt")?;
    let stderr = child.stderr.take().ok_or("stderr-Diagnosekanal fehlt")?;
    let (reader_tx, reader_rx) = mpsc::channel();
    let stdout_reader = spawn_reader(stdout, true, reader_tx.clone());
    let stderr_reader = spawn_reader(stderr, false, reader_tx);
    let writer_opened = opened.clone();
    let writer_cancelled = cancelled.clone();
    let (writer_tx, writer_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        while let Ok(message) = writer_rx.recv() {
            match message {
                ControlMessage::Continue if writer_opened.load(Ordering::SeqCst) => {
                    if stdin
                        .write_all(b"CONTINUE\n")
                        .and_then(|_| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
                ControlMessage::Cancel => {
                    let _ = stdin.write_all(b"CANCEL\n");
                    let _ = stdin.flush();
                    break;
                }
                ControlMessage::Continue => {}
            }
        }
        drop(stdin);
        writer_cancelled.load(Ordering::SeqCst)
    });
    emit(
        &app,
        GuidedUiEvent::Diagnostic {
            stream: "process",
            message: format!("Clientprozess gestartet (PID {pid})"),
        },
    );
    let mut protocol = GuidedProtocol::default();
    let mut stdout_closed = false;
    let mut stderr_closed = false;
    let started = Instant::now();
    let mut status: Option<ExitStatus> = None;
    let mut terminal_error = None;
    while status.is_none() {
        while let Ok(message) = controls.try_recv() {
            if writer_tx.send(message).is_err() {
                terminal_error = Some("stdin-Handshakekanal wurde geschlossen".into());
                cancelled.store(true, Ordering::SeqCst);
                break;
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(25));
            let termination = terminate_and_reap(&mut child)
                .map_err(|error| format!("Abbruch-Cleanup fehlgeschlagen: {error}"))?;
            if termination.forced_kill {
                emit(&app, GuidedUiEvent::ProcessForcedKill);
            }
            status = Some(termination.status);
            terminal_error = Some("Scan wurde abgebrochen".to_string());
            emit(&app, GuidedUiEvent::ProcessCancelled);
            break;
        }
        if started.elapsed() >= PROCESS_TIMEOUT {
            let termination = terminate_and_reap(&mut child)
                .map_err(|error| format!("Timeout-Cleanup fehlgeschlagen: {error}"))?;
            if termination.forced_kill {
                emit(&app, GuidedUiEvent::ProcessForcedKill);
            }
            status = Some(termination.status);
            terminal_error = Some("Gesamt-Timeout von 20 Minuten überschritten".to_string());
            emit(&app, GuidedUiEvent::ProcessTimedOut);
            break;
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
            }
            Ok(None) => {}
            Err(error) => {
                if let Ok(termination) = terminate_and_reap(&mut child) {
                    if termination.forced_kill {
                        emit(&app, GuidedUiEvent::ProcessForcedKill);
                    }
                }
                return Err(format!(
                    "Prozessstatus konnte nicht festgestellt werden: {error}"
                ));
            }
        }
        match reader_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(ReaderMessage::Line(is_stdout, line)) => {
                if is_stdout {
                    match parse_client_line(&line) {
                        Ok(ClientEvent::Diagnostic) => emit(
                            &app,
                            GuidedUiEvent::Diagnostic {
                                stream: "stdout",
                                message: line,
                            },
                        ),
                        Ok(event) => {
                            if let ClientEvent::Opened = event {
                                opened.store(true, Ordering::SeqCst);
                            }
                            if let Err(error) = protocol.observe(event.clone()) {
                                terminal_error =
                                    Some(format!("Ungültige Clientereignisfolge: {error:?}"));
                                cancelled.store(true, Ordering::SeqCst);
                            } else {
                                emit_client_event(&app, event);
                            }
                        }
                        Err(error) => {
                            terminal_error =
                                Some(format!("Ungültige Clientprotokollzeile: {error:?}"));
                            cancelled.store(true, Ordering::SeqCst);
                        }
                    }
                } else {
                    emit(
                        &app,
                        GuidedUiEvent::Diagnostic {
                            stream: "stderr",
                            message: line,
                        },
                    );
                }
            }
            Ok(ReaderMessage::Closed(is_stdout)) => {
                if is_stdout {
                    stdout_closed = true;
                } else {
                    stderr_closed = true;
                }
            }
            Ok(ReaderMessage::Error(is_stdout, detail)) => {
                terminal_error = Some(format!(
                    "{} konnte nicht gelesen werden: {detail}",
                    if is_stdout { "stdout" } else { "stderr" }
                ));
                cancelled.store(true, Ordering::SeqCst);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stdout_closed = true;
                stderr_closed = true;
            }
        }
    }
    // Close control plumbing before final stream drain; the child has exited or was reaped.
    drop(writer_tx);
    if writer.join().is_err() {
        return Err("stdin-Writer konnte nicht erfolgreich beendet werden".into());
    }
    let observed_status = status;
    let waited_status = child.wait().map_err(|error| {
        format!("Kindprozess konnte nicht nachweislich gereapt werden: {error}")
    })?;
    if observed_status.is_some_and(|observed| observed != waited_status) {
        return Err("try_wait-Status und abschließendes wait-Ergebnis widersprechen sich".into());
    }
    status = Some(waited_status);
    let closed_deadline = Instant::now() + Duration::from_secs(5);
    while !(stdout_closed && stderr_closed) && Instant::now() < closed_deadline {
        match reader_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(ReaderMessage::Line(true, line)) => {
                let event = parse_client_line(&line)
                    .map_err(|error| format!("Protokollfehler nach Exit: {error:?}"))?;
                protocol
                    .observe(event.clone())
                    .map_err(|error| format!("Zustandsfehler nach Exit: {error:?}"))?;
                emit_client_event(&app, event);
            }
            Ok(ReaderMessage::Line(false, line)) => emit(
                &app,
                GuidedUiEvent::Diagnostic {
                    stream: "stderr",
                    message: line,
                },
            ),
            Ok(ReaderMessage::Closed(true)) => stdout_closed = true,
            Ok(ReaderMessage::Closed(false)) => stderr_closed = true,
            Ok(ReaderMessage::Error(_, detail)) => terminal_error = Some(detail),
            Err(_) => {}
        }
    }
    if stdout_reader.join().is_err() || stderr_reader.join().is_err() {
        return Err("stdout/stderr-Reader konnten nicht erfolgreich beendet werden".into());
    }
    let exit = status.expect("child wait status is required after cleanup");
    emit(
        &app,
        GuidedUiEvent::ProcessExited {
            exit_code: exit.code().unwrap_or(-1),
        },
    );
    if !stdout_closed || !stderr_closed {
        return Err("stdout/stderr wurden nicht vollständig geschlossen".into());
    }
    emit(&app, GuidedUiEvent::ProcessCleaned);
    if let Some(error) = terminal_error {
        return Err(error);
    }
    if !exit.success() {
        return Err(format!("Clientprozess endete mit {exit}"));
    }
    if protocol.state() != crate::model::ScanState::Completed {
        return Err("Clientprozess endete ohne vollständige BEBLOG/1-Completionsequenz".into());
    }
    let paths = OfflineArtifactPaths::from_fixed_output_directory(&work_directory);
    let report = import_offline_artifacts(&archive, request.frame, &paths)
        .map_err(|error| format!("Artefaktvalidierung/Archivierung fehlgeschlagen: {error}"))?;
    let export = export_validated_artifacts(request.frame, &paths, &request.output_directory)
        .map_err(|error| format!("Validierte Dateien konnten nicht exportiert werden: {error}"))?;
    emit(
        &app,
        GuidedUiEvent::ArtifactsExported {
            directory: export.display().to_string(),
            capture_id: report.manifest.capture_id,
        },
    );
    Ok(())
}

fn spawn_reader<R: io::Read + Send + 'static>(
    reader: R,
    is_stdout: bool,
    sender: Sender<ReaderMessage>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => {
                    let _ = sender.send(ReaderMessage::Closed(is_stdout));
                    break;
                }
                Ok(_) => {
                    if bytes.len() > 64 * 1024 {
                        let _ = sender.send(ReaderMessage::Error(
                            is_stdout,
                            "Logzeile überschreitet 64 KiB".into(),
                        ));
                        break;
                    }
                    let line = String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_string();
                    if sender.send(ReaderMessage::Line(is_stdout, line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ReaderMessage::Error(is_stdout, error.to_string()));
                    break;
                }
            }
        }
    })
}

struct TerminationResult {
    status: ExitStatus,
    forced_kill: bool,
}

fn terminate_and_reap(child: &mut Child) -> io::Result<TerminationResult> {
    let pid = child.id() as i32;
    #[cfg(unix)]
    {
        if unsafe { libc::kill(-pid, libc::SIGTERM) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                if let Some(status) = child.try_wait()? {
                    return Ok(TerminationResult {
                        status: child.wait().unwrap_or(status),
                        forced_kill: false,
                    });
                }
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            } else {
                return Err(error);
            }
        }
    }
    let until = Instant::now() + TERMINATION_GRACE;
    while Instant::now() < until {
        if let Some(status) = child.try_wait()? {
            let waited = child.wait().unwrap_or(status);
            return Ok(TerminationResult {
                status: waited,
                forced_kill: false,
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
    #[cfg(unix)]
    {
        if unsafe { libc::kill(-pid, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                child.kill()?;
            }
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
        }
    }
    let status = child.wait()?;
    Ok(TerminationResult {
        status,
        forced_kill: true,
    })
}

fn emit_client_event(app: &AppHandle, event: ClientEvent) {
    let value = match event {
        ClientEvent::Opened => GuidedUiEvent::Opened,
        ClientEvent::OpenFailed => GuidedUiEvent::OpenFailed {
            detail: "sane_open fehlgeschlagen".into(),
        },
        ClientEvent::HolderDetected => GuidedUiEvent::HolderDetected,
        ClientEvent::FramePositioned(frame) => GuidedUiEvent::FramePositioned { frame },
        ClientEvent::Parameters {
            dpi,
            width,
            height,
            bytes_per_line,
            depth,
            format,
            expected_bytes,
        } => GuidedUiEvent::Parameters {
            dpi,
            width,
            height,
            bytes_per_line,
            depth,
            format,
            expected_bytes,
        },
        ClientEvent::ScanStarted => GuidedUiEvent::ScanStarted,
        ClientEvent::DataChunk { chunk, total } => GuidedUiEvent::DataChunk {
            chunk_bytes: chunk,
            total_bytes: total,
        },
        ClientEvent::Eof { total } => GuidedUiEvent::Eof { total_bytes: total },
        ClientEvent::TiffWritten => GuidedUiEvent::TiffWritten,
        ClientEvent::ScannerClosing => GuidedUiEvent::ScannerClosing,
        // COMPLETE is a client-side marker only. The frontend reaches Completed
        // only after OS completion, validation, archival, and export succeed.
        ClientEvent::Complete => return,
        ClientEvent::Error { status } => GuidedUiEvent::Error {
            detail: format!("Client meldet SANE-Status {status}"),
        },
        ClientEvent::ProtocolError => GuidedUiEvent::Error {
            detail: "Clientprotokollfehler".into(),
        },
        ClientEvent::Diagnostic => return,
    };
    emit(app, value);
}

fn emit(app: &AppHandle, event: GuidedUiEvent) {
    let _ = app.emit("hardware-client-event", event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_arguments_are_fixed_and_device_confirmation_is_required() {
        let root = std::env::temp_dir().join(format!("beblog-guided-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let request = GuidedScanRequest {
            frame: 2,
            dpi: 2820,
            device: "scanner:explicit".into(),
            confirmed_device: "scanner:explicit".into(),
            output_directory: root.clone(),
        };
        validate_request(&request).unwrap();
        let args = client_arguments(&request, &root.join("capture"));
        let args = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "-d",
                "scanner:explicit",
                "-o",
                root.join("capture").to_string_lossy().as_ref(),
                "-f",
                "2",
                "-r",
                "2820",
                "-t",
                "120",
                "-i",
                "2"
            ]
        );
        let mut invalid = request;
        invalid.confirmed_device = "other".into();
        assert!(validate_request(&invalid).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
