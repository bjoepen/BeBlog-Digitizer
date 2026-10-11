use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanState {
    Idle,
    Initializing,
    OpeningScanner,
    WaitingForHolder,
    ConfiguringFrame,
    StartingScan,
    Transferring,
    Finalizing,
    Completed,
    Cancelling,
    Cancelled,
    Failed,
    TimedOut,
}

#[cfg(test)]
impl ScanState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SimulationMode {
    Success,
    Error,
    Timeout,
    Incomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScanRequest {
    pub frame: u8,
    pub simulation: SimulationMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEvent {
    pub job_id: String,
    pub state: ScanState,
    pub progress: u8,
    pub message: String,
    pub level: EventLevel,
    pub timestamp_ms: u128,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub adapter: &'static str,
    pub scanner_status: &'static str,
    pub holder_status: &'static str,
    pub active_job_id: Option<String>,
    pub state: ScanState,
    pub progress: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    Cancelled,
    TimedOut,
    ProcessExit(i32),
    IncompleteStream,
}
