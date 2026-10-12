use crate::{
    archive::{CaptureKind, CaptureProduct},
    log_parser::{LogStream, ParsedLog, SidecarLogParser, TransferAccumulator},
    model::{EventLevel, HolderStatus, ScanError, ScanState, SimulationMode, StartScanRequest},
    process::{ProcessEvent, ProcessOutcome, ProcessRequest, ProcessRunner, RecordedProcessRunner},
};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

#[cfg(test)]
use crate::process::RecordedFault;

pub struct AdapterStep {
    /// None denotes a diagnostic-only event which must not advance the lifecycle.
    pub state: Option<ScanState>,
    pub progress: u8,
    pub message: String,
    pub level: EventLevel,
    pub holder_status: Option<HolderStatus>,
}

pub trait ScannerAdapter: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn capture_kind(&self) -> CaptureKind;
    fn scan(
        &self,
        request: &StartScanRequest,
        cancelled: Arc<AtomicBool>,
        emit: &mut dyn FnMut(AdapterStep),
    ) -> Result<CaptureProduct, ScanError>;
}

pub struct FakeScannerAdapter {
    runner: Arc<dyn ProcessRunner>,
}

impl Default for FakeScannerAdapter {
    fn default() -> Self {
        Self {
            runner: Arc::new(RecordedProcessRunner::default()),
        }
    }
}

#[cfg(test)]
impl FakeScannerAdapter {
    pub(crate) fn with_recorded_fault(fault: RecordedFault) -> Self {
        Self {
            runner: Arc::new(RecordedProcessRunner::with_fault(fault)),
        }
    }
}

#[derive(Default)]
struct ProcessLifecycle {
    started: bool,
    stdout_closed: bool,
    stderr_closed: bool,
    reaped: bool,
    reaped_code: Option<i32>,
    terminal: Option<ObservedTerminal>,
    cleanup: CleanupOutcome,
}

#[derive(Debug, PartialEq, Eq)]
enum ObservedTerminal {
    StartFailed(String),
    Exited(i32),
    Cancelled,
    TimedOut,
    UnexpectedEnd(String),
}

#[derive(Default)]
enum CleanupOutcome {
    #[default]
    Pending,
    Complete,
    Failed(String),
    NotRequired,
}

impl ProcessLifecycle {
    fn observe(&mut self, event: &ProcessEvent) -> Result<(), String> {
        let before_terminal = self.terminal.is_none();
        match event {
            ProcessEvent::Started { .. } if !self.started && before_terminal => {
                self.started = true;
            }
            ProcessEvent::ForcedKill if self.started && before_terminal => {}
            ProcessEvent::StartFailed(message) if !self.started && before_terminal => {
                self.terminal = Some(ObservedTerminal::StartFailed(message.clone()));
                self.cleanup = CleanupOutcome::NotRequired;
            }
            ProcessEvent::Stdout(_) if self.started && !self.stdout_closed && before_terminal => {}
            ProcessEvent::Stderr(_) if self.started && !self.stderr_closed && before_terminal => {}
            ProcessEvent::StdoutClosed
                if self.started && !self.stdout_closed && before_terminal =>
            {
                self.stdout_closed = true;
            }
            ProcessEvent::StderrClosed
                if self.started && !self.stderr_closed && before_terminal =>
            {
                self.stderr_closed = true;
            }
            ProcessEvent::Reaped { code }
                if self.started
                    && self.stdout_closed
                    && self.stderr_closed
                    && !self.reaped
                    && before_terminal =>
            {
                self.reaped = true;
                self.reaped_code = *code;
            }
            ProcessEvent::Exited { code }
                if self.started
                    && self.stdout_closed
                    && self.stderr_closed
                    && self.reaped
                    && self.reaped_code == Some(*code)
                    && before_terminal =>
            {
                self.terminal = Some(ObservedTerminal::Exited(*code));
            }
            ProcessEvent::Cancelled
                if before_terminal
                    && (!self.started || self.stdout_closed && self.stderr_closed) =>
            {
                self.terminal = Some(ObservedTerminal::Cancelled);
                if !self.started {
                    self.cleanup = CleanupOutcome::NotRequired;
                }
            }
            ProcessEvent::TimedOut
                if self.started && self.stdout_closed && self.stderr_closed && before_terminal =>
            {
                self.terminal = Some(ObservedTerminal::TimedOut);
            }
            ProcessEvent::UnexpectedEnd(message)
                if self.started && self.stdout_closed && self.stderr_closed && before_terminal =>
            {
                self.terminal = Some(ObservedTerminal::UnexpectedEnd(message.clone()));
            }
            ProcessEvent::CleanedUp
                if self.started
                    && self.reaped
                    && !matches!(
                        self.cleanup,
                        CleanupOutcome::Complete | CleanupOutcome::Failed(_)
                    )
                    && !before_terminal =>
            {
                self.cleanup = CleanupOutcome::Complete;
            }
            ProcessEvent::CleanupFailed(message)
                if self.started
                    && !matches!(
                        self.cleanup,
                        CleanupOutcome::Complete | CleanupOutcome::Failed(_)
                    )
                    && !before_terminal =>
            {
                self.cleanup = CleanupOutcome::Failed(message.clone());
            }
            _ => return Err(format!("unerwartetes Prozessereignis: {event:?}")),
        }
        Ok(())
    }

    fn validate(&self, outcome: &ProcessOutcome) -> Result<(), ScanError> {
        let terminal_matches = matches!(
            (&self.terminal, outcome),
            (Some(ObservedTerminal::StartFailed(a)), ProcessOutcome::StartFailed(b)) if a == b
        ) || matches!(
            (&self.terminal, outcome),
            (Some(ObservedTerminal::Exited(a)), ProcessOutcome::Exited(b)) if a == b
        ) || matches!(
            (&self.terminal, outcome),
            (Some(ObservedTerminal::Cancelled), ProcessOutcome::Cancelled)
        ) || matches!(
            (&self.terminal, outcome),
            (Some(ObservedTerminal::TimedOut), ProcessOutcome::TimedOut)
        ) || matches!(
            (&self.terminal, outcome),
            (Some(ObservedTerminal::UnexpectedEnd(a)), ProcessOutcome::UnexpectedEnd(b)) if a == b
        ) || matches!(
            (&self.terminal, &self.cleanup, outcome),
            (Some(_), CleanupOutcome::Failed(a), ProcessOutcome::CleanupFailed(b)) if a == b
        );
        let cleanup_matches = match (&self.terminal, &self.cleanup) {
            (Some(ObservedTerminal::StartFailed(_)), CleanupOutcome::NotRequired) => true,
            (Some(ObservedTerminal::Cancelled), CleanupOutcome::NotRequired) if !self.started => {
                true
            }
            (
                Some(
                    ObservedTerminal::Cancelled
                    | ObservedTerminal::TimedOut
                    | ObservedTerminal::Exited(_)
                    | ObservedTerminal::UnexpectedEnd(_),
                ),
                CleanupOutcome::Complete,
            ) => true,
            (Some(_), CleanupOutcome::Failed(_)) => {
                matches!(outcome, ProcessOutcome::CleanupFailed(_))
            }
            _ => false,
        };
        if !terminal_matches || !cleanup_matches {
            return Err(ScanError::ProcessProtocol(
                "Prozessereignisse und Abschlussstatus widersprechen sich oder sind unvollständig"
                    .into(),
            ));
        }
        match outcome {
            ProcessOutcome::StartFailed(message) => {
                Err(ScanError::ProcessStartFailed(message.clone()))
            }
            ProcessOutcome::Exited(code) if *code != 0 => Err(ScanError::ProcessExit(*code)),
            ProcessOutcome::Cancelled => Err(ScanError::Cancelled),
            ProcessOutcome::TimedOut => Err(ScanError::TimedOut),
            ProcessOutcome::UnexpectedEnd(message) => {
                Err(ScanError::UnexpectedProcessEnd(message.clone()))
            }
            ProcessOutcome::CleanupFailed(message) => {
                Err(ScanError::ProcessCleanupFailed(message.clone()))
            }
            ProcessOutcome::Exited(_) => Ok(()),
        }
    }
}

impl ScannerAdapter for FakeScannerAdapter {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn capture_kind(&self) -> CaptureKind {
        CaptureKind::Simulation
    }

    fn scan(
        &self,
        request: &StartScanRequest,
        cancelled: Arc<AtomicBool>,
        emit: &mut dyn FnMut(AdapterStep),
    ) -> Result<CaptureProduct, ScanError> {
        let parser = SidecarLogParser;
        let mut lifecycle = ProcessLifecycle::default();
        let mut lifecycle_error = None;
        let mut stream_error = None;
        let mut saw_eof = false;
        let mut captured_bytes = None;
        let mut saw_tiff = false;
        let mut transferred_bytes = TransferAccumulator::new(1_438_200);
        let mut client_exit_code = None;
        let process_request = ProcessRequest {
            frame: request.frame,
            simulation: request.simulation,
            timeout: Duration::from_secs(2),
        };
        let outcome = self
            .runner
            .run(&process_request, cancelled, &mut |event| {
                if lifecycle_error.is_some() {
                    return;
                }
                if let Err(error) = lifecycle.observe(&event) {
                    lifecycle_error = Some(error);
                    return;
                }
                if stream_error.is_some() {
                    return;
                }
                match event {
                ProcessEvent::Started { program, .. } => emit(AdapterStep {
                    state: Some(ScanState::Initializing),
                    progress: 2,
                    message: format!("Mock-Prozess gestartet: {program}"),
                    level: EventLevel::Info,
                    holder_status: Some(HolderStatus::Unknown),
                }),
                ProcessEvent::StartFailed(message) => emit(AdapterStep {
                    state: None,
                    progress: 0,
                    message: format!("Prozessstart fehlgeschlagen: {message}"),
                    level: EventLevel::Error,
                    holder_status: None,
                }),
                log_event @ (ProcessEvent::Stdout(_) | ProcessEvent::Stderr(_)) => {
                    let (stream, line) = match log_event {
                        ProcessEvent::Stdout(line) => (LogStream::Stdout, line),
                        ProcessEvent::Stderr(line) => (LogStream::Stderr, line),
                        _ => unreachable!(),
                    };
                    if stream_error.is_some() {
                        return;
                    }
                    match parser.parse(stream, &line) {
                    ParsedLog::Initialized => step(
                        emit,
                        ScanState::Initializing,
                        7,
                        "Initialisierung bestätigt",
                    ),
                    ParsedLog::ScannerOpened => step(
                        emit,
                        ScanState::OpeningScanner,
                        14,
                        "Scanneröffnung bestätigt",
                    ),
                    ParsedLog::HolderDetected => {
                        let mut holder_step = make_step(
                            ScanState::WaitingForHolder,
                            20,
                            "Filmhalter erkannt",
                            EventLevel::Info,
                        );
                        holder_step.holder_status = Some(HolderStatus::Detected);
                        emit(holder_step);
                    }
                    ParsedLog::FrameConfigured => step(
                        emit,
                        ScanState::PositioningFrame,
                        26,
                        "Framepositionierung bestätigt",
                    ),
                    ParsedLog::ScanOptionConfigured => step(
                        emit,
                        ScanState::ConfiguringFrame,
                        30,
                        "Scanoption bestätigt",
                    ),
                    ParsedLog::ScanStarted => {
                        step(emit, ScanState::StartingScan, 32, "Scanstart bestätigt");
                        step(emit, ScanState::Scanning, 34, "Scan läuft");
                    }
                    ParsedLog::ReadChunk { bytes } => {
                        if saw_eof || captured_bytes.is_some() {
                            let error = "Datenmeldung nach EOF/Capture-Abschluss".to_string();
                            stream_error = Some(error.clone());
                            emit(make_diagnostic(&error, EventLevel::Error));
                            return;
                        }
                        let total = match transferred_bytes.record_chunk(bytes) {
                            Ok(total) => total,
                            Err(error) => {
                                let message = format!("Ungültige RAW-Chunkfolge: {error:?}");
                                stream_error = Some(message.clone());
                                emit(make_diagnostic(&message, EventLevel::Error));
                                return;
                            }
                        };
                        let progress = 40 + ((total.min(1_438_200) * 48 / 1_438_200) as u8);
                        emit(AdapterStep {
                            state: Some(ScanState::Transferring),
                            progress,
                            message: format!("Datenchunk: {bytes} Byte; kumuliert {total} / 1438200 Byte"),
                            level: EventLevel::Info,
                            holder_status: None,
                        });
                    }
                    ParsedLog::Eof { final_chunk_bytes } => {
                        if saw_eof || captured_bytes.is_some() {
                            let error = "EOF doppelt oder nach Capture-Abschluss empfangen".to_string();
                            stream_error = Some(error.clone());
                            emit(make_diagnostic(&error, EventLevel::Error));
                            return;
                        }
                        if final_chunk_bytes > 0 {
                            if let Err(error) = transferred_bytes.record_chunk(final_chunk_bytes) {
                                let message = format!("Ungültiger finaler EOF-Chunk: {error:?}");
                                stream_error = Some(message.clone());
                                emit(make_diagnostic(&message, EventLevel::Error));
                                return;
                            }
                        }
                        if transferred_bytes.total() != 1_438_200 {
                            let error = "EOF vor vollständiger Datenübertragung oder doppelt empfangen".to_string();
                            stream_error = Some(error.clone());
                            emit(make_diagnostic(&error, EventLevel::Error));
                            return;
                        }
                        saw_eof = true;
                        step(emit, ScanState::Transferring, 90, "EOF bestätigt");
                    }
                    ParsedLog::CaptureComplete { total_bytes, terminal_status } => {
                        if !saw_eof || terminal_status != 5 || captured_bytes.is_some() {
                            let error = format!("Capture-Endstand vor EOF, mit falschem Status oder doppelt empfangen (terminal={terminal_status})");
                            stream_error = Some(error.clone());
                            emit(make_diagnostic(&error, EventLevel::Error));
                            return;
                        }
                        if let Err(error) = transferred_bytes.verify_reported_total(total_bytes) {
                            let message = format!("Capture-Gesamtstand nicht konsistent: {error:?}");
                            stream_error = Some(message.clone());
                            emit(make_diagnostic(&message, EventLevel::Error));
                            return;
                        }
                        captured_bytes = Some(total_bytes);
                    }
                    ParsedLog::TiffWritten { path: _ } => {
                        if captured_bytes != Some(1_438_200) || saw_tiff {
                            let error = "TIFF-Meldung vor gültigem Capture-Endstand oder doppelt empfangen".to_string();
                            stream_error = Some(error.clone());
                            emit(make_diagnostic(&error, EventLevel::Error));
                            return;
                        }
                        saw_tiff = true;
                        step(
                            emit,
                            ScanState::Finalizing,
                            97,
                            "TIFF-Finalisierung bestätigt",
                        );
                    }
                    ParsedLog::ClientExit { code } => {
                        if client_exit_code.replace(code).is_some() {
                            let error = "Doppelte Client-Exitdiagnose".to_string();
                            stream_error = Some(error.clone());
                            emit(make_diagnostic(&error, EventLevel::Error));
                        }
                    }
                    ParsedLog::ReadFailed { status, detail } => {
                        let error = format!("sane_read fehlgeschlagen: Status {status} {detail}");
                        stream_error = Some(error.clone());
                        emit(make_diagnostic(&error, EventLevel::Error));
                    }
                    ParsedLog::Malformed { stream, line } => {
                        let error = format!("Fehlerhafte bekannte Logmeldung auf {stream:?}: {line}");
                        stream_error = Some(error.clone());
                        emit(make_diagnostic(&error, EventLevel::Error));
                    }
                    ParsedLog::Unknown { stream, line } => emit(AdapterStep {
                        // Preserve this as a typed diagnostic, never as a lifecycle step.
                        state: None,
                        progress: 0,
                        message: format!("Unbekannte {stream:?}-Diagnose ignoriert: {line}"),
                        level: EventLevel::Warning,
                        holder_status: None,
                    }),
                    }
                }
                ProcessEvent::StdoutClosed
                | ProcessEvent::StderrClosed
                | ProcessEvent::Reaped { .. }
                | ProcessEvent::Exited { .. }
                | ProcessEvent::Cancelled
                | ProcessEvent::TimedOut
                | ProcessEvent::ForcedKill
                | ProcessEvent::UnexpectedEnd(_) => {}
                ProcessEvent::CleanedUp => step(
                    emit,
                    ScanState::ClosingScanner,
                    98,
                    "Mock-Prozess bereinigt und Scannerverbindung geschlossen",
                ),
                ProcessEvent::CleanupFailed(message) => emit(AdapterStep {
                    state: None,
                    progress: 0,
                    message: format!("Prozessbereinigung fehlgeschlagen: {message}"),
                    level: EventLevel::Error,
                    holder_status: None,
                }),
            }});

        if let Some(error) = lifecycle_error {
            return Err(ScanError::ProcessProtocol(error));
        }
        lifecycle.validate(&outcome)?;
        if let (Some(client_code), ProcessOutcome::Exited(process_code)) =
            (client_exit_code, &outcome)
        {
            if client_code != *process_code {
                return Err(ScanError::ProcessProtocol(format!(
                    "Client-Exitdiagnose {client_code} widerspricht OS-Exitstatus {process_code}"
                )));
            }
        }
        if let Some(error) = stream_error {
            return Err(ScanError::InvalidDataStream(error));
        }
        if !saw_eof
            || transferred_bytes.total() != 1_438_200
            || captured_bytes != Some(transferred_bytes.total())
            || !saw_tiff
        {
            return Err(ScanError::IncompleteStream);
        }
        let raw = match request.simulation {
            SimulationMode::MissingOutput => Vec::new(),
            _ => b"BEBLOG-MOCK-RAW\nsource=synthetic-recorded-process\n".to_vec(),
        };
        Ok(CaptureProduct {
            expected_raw_bytes: 1_438_200,
            raw,
            tiff: match request.simulation {
                SimulationMode::InvalidOutput => b"invalid-tiff".to_vec(),
                SimulationMode::MissingOutput => Vec::new(),
                _ => b"BEBLOG-MOCK-TIFF\nnot-a-production-image\n".to_vec(),
            },
            kind: CaptureKind::Simulation,
        })
    }
}

fn make_diagnostic(message: &str, level: EventLevel) -> AdapterStep {
    AdapterStep {
        state: None,
        progress: 0,
        message: message.into(),
        level,
        holder_status: None,
    }
}

fn step(emit: &mut dyn FnMut(AdapterStep), state: ScanState, progress: u8, message: &str) {
    emit(make_step(state, progress, message, EventLevel::Info));
}

fn make_step(state: ScanState, progress: u8, message: &str, level: EventLevel) -> AdapterStep {
    AdapterStep {
        state: Some(state),
        progress,
        message: message.into(),
        level,
        holder_status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{ArchiveManager, ArchiveStatus};
    use crate::model::SimulationMode;

    fn request(simulation: SimulationMode) -> StartScanRequest {
        StartScanRequest {
            frame: 1,
            simulation,
        }
    }

    #[test]
    fn fake_adapter_reaches_eof_without_hardware() {
        let mut steps = Vec::new();
        FakeScannerAdapter::default()
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |step| steps.push(step),
            )
            .unwrap();
        assert!(steps
            .iter()
            .any(|step| step.message.contains("EOF bestätigt")));
        assert_eq!(steps.last().unwrap().state, Some(ScanState::ClosingScanner));
        assert!(steps
            .iter()
            .any(|step| step.holder_status == Some(HolderStatus::Unknown)));
        assert!(steps
            .iter()
            .any(|step| step.holder_status == Some(HolderStatus::Detected)));
        let fixture = include_str!("../tests/fixtures/research-009-success.fixture");
        assert!(fixture.contains("capture ended: total=1438200"));
        assert!(fixture.contains("sane_start (only call)"));
    }

    #[test]
    fn complete_recorded_fake_event_sequence_preserves_strict_lifecycle() {
        use crate::state_machine::ScanStateMachine;

        let mut machine = ScanStateMachine::default();
        machine
            .transition(ScanState::AwaitingHolderConfirmation)
            .unwrap();
        machine
            .transition(ScanState::WaitingForHolderDetection)
            .unwrap();
        let mut observed_unknown = false;
        FakeScannerAdapter::default()
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |step| match step.state {
                    Some(state) => machine.transition(state).unwrap(),
                    None if step.message.starts_with("Unbekannte") => {
                        observed_unknown = true;
                        assert_eq!(machine.state(), ScanState::OpeningScanner);
                    }
                    None => {}
                },
            )
            .unwrap();

        assert!(observed_unknown, "fixture diagnostic must be exercised");
        assert_eq!(machine.state(), ScanState::ClosingScanner);
        // Unknown messages do not create a duplicate Open transition, while the
        // explicit duplicate-phase rule is exercised by known progress reports.
        machine.transition(ScanState::ClosingScanner).unwrap();
        assert_eq!(machine.state(), ScanState::ClosingScanner);
        assert!(machine.transition(ScanState::OpeningScanner).is_err());
    }

    #[test]
    fn recorded_client_diagnostics_on_stderr_are_parsed_without_losing_stream_identity() {
        let adapter = FakeScannerAdapter {
            runner: Arc::new(RecordedProcessRunner::on_stream(LogStream::Stderr)),
        };
        let mut steps = Vec::new();
        adapter
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |step| steps.push(step),
            )
            .unwrap();

        assert!(steps
            .iter()
            .any(|step| step.message.contains("Datenchunk: 458752 Byte")));
        assert!(steps.iter().any(|step| step
            .message
            .contains("Unbekannte Stderr-Diagnose ignoriert")));
        assert_eq!(steps.last().unwrap().state, Some(ScanState::ClosingScanner));
    }

    #[test]
    fn fake_adapter_reports_timeout() {
        let result = FakeScannerAdapter::default().scan(
            &request(SimulationMode::Timeout),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        );
        assert_eq!(result, Err(ScanError::TimedOut));
    }

    #[test]
    fn fake_adapter_honours_cancellation() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let result = FakeScannerAdapter::default().scan(
            &request(SimulationMode::Success),
            cancelled,
            &mut |_| {},
        );
        assert_eq!(result, Err(ScanError::Cancelled));
    }

    #[test]
    fn start_failure_is_distinct_from_exit_failure() {
        let result = FakeScannerAdapter::with_recorded_fault(RecordedFault::StartFailure).scan(
            &request(SimulationMode::Success),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        );
        assert_eq!(
            result,
            Err(ScanError::ProcessStartFailed(
                "recorded process start failure".into()
            ))
        );
    }

    #[test]
    fn cleanup_failure_prevents_success() {
        let result = FakeScannerAdapter::with_recorded_fault(RecordedFault::CleanupFailure).scan(
            &request(SimulationMode::Success),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        );
        assert_eq!(
            result,
            Err(ScanError::ProcessCleanupFailed(
                "recorded process cleanup failure".into()
            ))
        );
    }

    #[test]
    fn success_outcome_without_cleanup_event_is_rejected() {
        let result = FakeScannerAdapter::with_recorded_fault(RecordedFault::MissingCleanupEvent)
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |_| {},
            );
        assert!(matches!(result, Err(ScanError::ProcessProtocol(_))));
    }

    #[test]
    fn exit_event_without_successful_reaping_evidence_is_rejected() {
        let result = FakeScannerAdapter::with_recorded_fault(RecordedFault::MissingReapedEvent)
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |_| {},
            );
        assert!(matches!(result, Err(ScanError::ProcessProtocol(_))));
    }

    #[test]
    fn incomplete_data_and_missing_tiff_are_rejected() {
        let mut steps = Vec::new();
        let incomplete = FakeScannerAdapter::with_recorded_fault(RecordedFault::IncompleteData)
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |step| steps.push(step),
            );
        assert!(matches!(incomplete, Err(ScanError::InvalidDataStream(_))));
        assert!(!steps
            .iter()
            .any(|step| step.state == Some(ScanState::ClosingScanner)));

        let missing_tiff = FakeScannerAdapter::with_recorded_fault(RecordedFault::MissingTiff)
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |_| {},
            );
        assert_eq!(missing_tiff, Err(ScanError::IncompleteStream));
    }

    #[test]
    fn missing_or_invalid_tiff_cannot_finalize_an_archive() {
        for mode in [SimulationMode::MissingOutput, SimulationMode::InvalidOutput] {
            let root = std::env::temp_dir().join(format!(
                "beblog-output-lifecycle-{}",
                ArchiveManager::next_capture_id()
            ));
            let archive = ArchiveManager::new(root.clone());
            let request = request(mode);
            let product = FakeScannerAdapter::default()
                .scan(&request, Arc::new(AtomicBool::new(false)), &mut |_| {})
                .unwrap();
            let pending = archive.begin(&request, CaptureKind::Simulation).unwrap();
            assert!(archive.finalize(&pending, product).is_err());
            let manifest: crate::archive::CaptureManifest = serde_json::from_slice(
                &std::fs::read(pending.directory.join("manifest.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest.status, ArchiveStatus::Failed);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn unexpected_end_and_output_after_exit_are_latched_protocol_errors() {
        let unexpected_end = FakeScannerAdapter::with_recorded_fault(RecordedFault::UnexpectedEnd)
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |_| {},
            );
        assert!(matches!(
            unexpected_end,
            Err(ScanError::UnexpectedProcessEnd(_))
        ));

        let late_output = FakeScannerAdapter::with_recorded_fault(RecordedFault::LateOutput).scan(
            &request(SimulationMode::Success),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        );
        assert!(matches!(late_output, Err(ScanError::ProcessProtocol(_))));
    }

    #[test]
    fn cancellation_during_frame_is_distinct_and_resources_are_cleaned() {
        let mut steps = Vec::new();
        let result = FakeScannerAdapter::with_recorded_fault(RecordedFault::CancelAfterStdout(4))
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |step| steps.push(step),
            );
        assert_eq!(result, Err(ScanError::Cancelled));
        assert!(steps
            .iter()
            .any(|step| step.message.contains("Mock-Prozess bereinigt")));
    }

    #[test]
    fn real_adapter_is_not_available_in_this_build() {
        assert_eq!(FakeScannerAdapter::default().name(), "fake");
    }

    #[test]
    fn nonzero_exit_is_an_error() {
        let result = FakeScannerAdapter::default().scan(
            &request(SimulationMode::Error),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        );
        assert_eq!(result, Err(ScanError::ProcessExit(1)));
    }

    #[test]
    fn incomplete_stream_is_rejected_even_with_exit_zero() {
        let result = FakeScannerAdapter::default().scan(
            &request(SimulationMode::Incomplete),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        );
        assert_eq!(result, Err(ScanError::IncompleteStream));
    }

    #[test]
    fn unknown_log_message_is_diagnostic_not_failure() {
        let mut steps = Vec::new();
        FakeScannerAdapter::default()
            .scan(
                &request(SimulationMode::Success),
                Arc::new(AtomicBool::new(false)),
                &mut |step| steps.push(step),
            )
            .unwrap();
        assert!(steps.iter().any(|step| step
            .message
            .contains("Unbekannte Stdout-Diagnose ignoriert")));
    }
}
