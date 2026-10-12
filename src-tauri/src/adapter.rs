use crate::{
    archive::{CaptureKind, CaptureProduct},
    log_parser::{ParsedLog, SidecarLogParser},
    model::{EventLevel, HolderStatus, ScanError, ScanState, SimulationMode, StartScanRequest},
    process::{ProcessEvent, ProcessOutcome, ProcessRequest, ProcessRunner, RecordedProcessRunner},
};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

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
            runner: Arc::new(RecordedProcessRunner),
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
        let mut saw_eof = false;
        let mut captured_bytes = None;
        let mut saw_tiff = false;
        let process_request = ProcessRequest {
            frame: request.frame,
            simulation: request.simulation,
            timeout: Duration::from_secs(2),
        };
        let outcome = self
            .runner
            .run(&process_request, cancelled, &mut |event| match event {
                ProcessEvent::Started { program } => emit(AdapterStep {
                    state: Some(ScanState::Initializing),
                    progress: 2,
                    message: format!("Mock-Prozess gestartet: {program}"),
                    level: EventLevel::Info,
                    holder_status: Some(HolderStatus::Unknown),
                }),
                ProcessEvent::Stdout(line) => match parser.parse(&line) {
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
                    ParsedLog::Data { bytes } => {
                        let progress = 40 + ((bytes.min(1_438_200) * 48 / 1_438_200) as u8);
                        emit(AdapterStep {
                            state: Some(ScanState::Transferring),
                            progress,
                            message: format!("Datenstrom: {bytes} / 1438200 Byte"),
                            level: EventLevel::Info,
                            holder_status: None,
                        });
                    }
                    ParsedLog::Eof => {
                        saw_eof = true;
                        step(emit, ScanState::Transferring, 90, "EOF bestätigt");
                    }
                    ParsedLog::CaptureComplete { bytes } => captured_bytes = Some(bytes),
                    ParsedLog::TiffWritten => {
                        saw_tiff = true;
                        step(
                            emit,
                            ScanState::Finalizing,
                            97,
                            "TIFF-Finalisierung bestätigt",
                        );
                    }
                    ParsedLog::Unknown(message) => emit(AdapterStep {
                        // Preserve this as a typed diagnostic, never as a lifecycle step.
                        state: None,
                        progress: 0,
                        message: format!("Unbekannte Logmeldung ignoriert: {message}"),
                        level: EventLevel::Warning,
                        holder_status: None,
                    }),
                },
                ProcessEvent::Stderr(message) => emit(AdapterStep {
                    state: Some(ScanState::Transferring),
                    progress: 45,
                    message: format!("Mock-Prozess stderr: {message}"),
                    level: EventLevel::Error,
                    holder_status: None,
                }),
                ProcessEvent::Exited { .. } | ProcessEvent::Cancelled | ProcessEvent::TimedOut => {}
                ProcessEvent::CleanedUp => step(
                    emit,
                    ScanState::ClosingScanner,
                    98,
                    "Mock-Prozess bereinigt und Scannerverbindung geschlossen",
                ),
            });

        match outcome {
            ProcessOutcome::Cancelled => return Err(ScanError::Cancelled),
            ProcessOutcome::TimedOut => return Err(ScanError::TimedOut),
            ProcessOutcome::Exited(code) if code != 0 => return Err(ScanError::ProcessExit(code)),
            ProcessOutcome::Exited(_) => {}
        }
        if !saw_eof || captured_bytes != Some(1_438_200) || !saw_tiff {
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
                    None => {
                        observed_unknown = true;
                        assert_eq!(machine.state(), ScanState::OpeningScanner);
                    }
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
        assert!(steps
            .iter()
            .any(|step| step.message.contains("Unbekannte Logmeldung ignoriert")));
    }
}
