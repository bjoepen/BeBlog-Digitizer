use crate::{
    archive::CaptureProduct,
    log_parser::{ParsedLog, SidecarLogParser},
    model::{EventLevel, ScanError, ScanState, StartScanRequest},
    process::{ProcessEvent, ProcessOutcome, ProcessRequest, ProcessRunner, RecordedProcessRunner},
};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

pub struct AdapterStep {
    pub state: ScanState,
    pub progress: u8,
    pub message: String,
    pub level: EventLevel,
}

pub trait ScannerAdapter: Send + Sync + 'static {
    fn name(&self) -> &'static str;
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
                    state: ScanState::Initializing,
                    progress: 2,
                    message: format!("Mock-Prozess gestartet: {program}"),
                    level: EventLevel::Info,
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
                        step(emit, ScanState::WaitingForHolder, 20, "Filmhalter erkannt");
                        step(
                            emit,
                            ScanState::ConfiguringFrame,
                            28,
                            "Frame, RGB8 und 600 dpi konfiguriert",
                        );
                    }
                    ParsedLog::FrameConfigured => {}
                    ParsedLog::ScanOptionConfigured => step(
                        emit,
                        ScanState::ConfiguringFrame,
                        30,
                        "Scanoption bestätigt",
                    ),
                    ParsedLog::ScanStarted => {
                        step(emit, ScanState::StartingScan, 34, "Scanstart bestätigt")
                    }
                    ParsedLog::Data { bytes } => {
                        let progress = 40 + ((bytes.min(1_438_200) * 48 / 1_438_200) as u8);
                        emit(AdapterStep {
                            state: ScanState::Transferring,
                            progress,
                            message: format!("Datenstrom: {bytes} / 1438200 Byte"),
                            level: EventLevel::Info,
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
                        state: current_state_for_unknown(saw_eof),
                        progress: if saw_eof { 92 } else { 16 },
                        message: format!("Unbekannte Logmeldung ignoriert: {message}"),
                        level: EventLevel::Warning,
                    }),
                },
                ProcessEvent::Stderr(message) => emit(AdapterStep {
                    state: ScanState::Transferring,
                    progress: 45,
                    message: format!("Mock-Prozess stderr: {message}"),
                    level: EventLevel::Error,
                }),
                ProcessEvent::Exited { .. }
                | ProcessEvent::Cancelled
                | ProcessEvent::TimedOut
                | ProcessEvent::CleanedUp => {}
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
        Ok(CaptureProduct {
            raw: b"BEBLOG-MOCK-RAW\nsource=research-009-recorded-log\n".to_vec(),
            tiff: b"BEBLOG-MOCK-TIFF\nnot-a-production-image\n".to_vec(),
            expected_raw_bytes: 1_438_200,
        })
    }
}

fn step(emit: &mut dyn FnMut(AdapterStep), state: ScanState, progress: u8, message: &str) {
    emit(AdapterStep {
        state,
        progress,
        message: message.into(),
        level: EventLevel::Info,
    });
}

fn current_state_for_unknown(eof: bool) -> ScanState {
    if eof {
        ScanState::Transferring
    } else {
        ScanState::OpeningScanner
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
        assert_eq!(steps.last().unwrap().state, ScanState::Finalizing);
        let fixture = include_str!("../tests/fixtures/research-009-success.fixture");
        assert!(fixture.contains("capture ended: total=1438200"));
        assert!(fixture.contains("sane_start (only call)"));
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
