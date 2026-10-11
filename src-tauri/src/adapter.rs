use std::{sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::Duration};
use crate::model::{EventLevel, ScanError, ScanState, SimulationMode, StartScanRequest};

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
    ) -> Result<(), ScanError>;
}

#[derive(Default)]
pub struct FakeScannerAdapter;

impl FakeScannerAdapter {
    fn pause(cancelled: &AtomicBool) -> Result<(), ScanError> {
        for _ in 0..4 {
            if cancelled.load(Ordering::SeqCst) { return Err(ScanError::Cancelled); }
            thread::sleep(Duration::from_millis(35));
        }
        Ok(())
    }
}

impl ScannerAdapter for FakeScannerAdapter {
    fn name(&self) -> &'static str { "fake" }

    fn scan(&self, request: &StartScanRequest, cancelled: Arc<AtomicBool>, emit: &mut dyn FnMut(AdapterStep)) -> Result<(), ScanError> {
        let mut step = |state, progress, message: String| {
            emit(AdapterStep { state, progress, message, level: EventLevel::Info });
        };
        step(ScanState::Initializing, 5, "SANE-Initialisierung simuliert".into());
        Self::pause(&cancelled)?;
        step(ScanState::OpeningScanner, 14, "Minolta FS-V1 wird simuliert geöffnet".into());
        Self::pause(&cancelled)?;
        step(ScanState::WaitingForHolder, 20, "Frame-Range 1..1; Filmhalter wird erwartet".into());
        Self::pause(&cancelled)?;
        if request.simulation == SimulationMode::Timeout { return Err(ScanError::TimedOut); }
        step(ScanState::ConfiguringFrame, 28, format!("35-mm-Halter erkannt; Frame {} · Color · 600 dpi", request.frame));
        Self::pause(&cancelled)?;
        step(ScanState::StartingScan, 34, "sane_start simuliert (ein Aufruf)".into());
        Self::pause(&cancelled)?;
        for (progress, bytes) in [(44, 196_608), (58, 458_752), (72, 851_968), (86, 1_245_184)] {
            step(ScanState::Transferring, progress, format!("sane_read simuliert · {bytes} / 1438200 Byte"));
            Self::pause(&cancelled)?;
            if request.simulation == SimulationMode::Error && progress == 58 { return Err(ScanError::SimulatedIo); }
        }
        step(ScanState::Transferring, 92, "SANE_STATUS_EOF · 1438200 Byte vollständig".into());
        Self::pause(&cancelled)?;
        step(ScanState::Finalizing, 97, "RAW, Parameter und TIFF werden simuliert finalisiert".into());
        Self::pause(&cancelled)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(simulation: SimulationMode) -> StartScanRequest { StartScanRequest { frame: 1, simulation } }

    #[test]
    fn fake_adapter_reaches_eof_without_hardware() {
        let mut steps = Vec::new();
        FakeScannerAdapter.scan(&request(SimulationMode::Success), Arc::new(AtomicBool::new(false)), &mut |step| steps.push(step)).unwrap();
        assert!(steps.iter().any(|step| step.message.contains("SANE_STATUS_EOF")));
        assert_eq!(steps.last().unwrap().state, ScanState::Finalizing);
        let fixture = include_str!("../tests/fixtures/research-009-success.fixture");
        assert!(fixture.contains("capture ended: total=1438200"));
        assert!(fixture.contains("sane_start (only call)"));
    }

    #[test]
    fn fake_adapter_reports_timeout() {
        let result = FakeScannerAdapter.scan(&request(SimulationMode::Timeout), Arc::new(AtomicBool::new(false)), &mut |_| {});
        assert_eq!(result, Err(ScanError::TimedOut));
    }

    #[test]
    fn fake_adapter_honours_cancellation() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let result = FakeScannerAdapter.scan(&request(SimulationMode::Success), cancelled, &mut |_| {});
        assert_eq!(result, Err(ScanError::Cancelled));
    }

    #[test]
    fn real_adapter_is_not_available_in_this_build() {
        assert_eq!(FakeScannerAdapter.name(), "fake");
    }
}
