use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanState {
    Idle,
    Initializing,
    OpeningScanner,
    WaitingForHolder,
    AwaitingHolderConfirmation,
    WaitingForHolderDetection,
    PositioningFrame,
    ConfiguringFrame,
    StartingScan,
    Scanning,
    Transferring,
    Finalizing,
    ClosingScanner,
    InterFrameCooldown,
    BatchCompleted,
    BatchFailed,
    BatchCancelled,
    Completed,
    Cancelling,
    Cancelled,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HolderStatus {
    Unknown,
    Detected,
}

#[cfg(test)]
impl ScanState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Cancelled
                | Self::Failed
                | Self::TimedOut
                | Self::BatchCompleted
                | Self::BatchFailed
                | Self::BatchCancelled
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
    MissingOutput,
    InvalidOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScanRequest {
    pub frame: u8,
    pub simulation: SimulationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBatchRequest {
    #[serde(default = "default_frames")]
    pub frames: Vec<u8>,
    pub simulation: SimulationMode,
    #[serde(default)]
    pub failure_frame: Option<u8>,
}

fn default_frames() -> Vec<u8> {
    vec![1, 2, 3, 4]
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSnapshot {
    pub batch_id: String,
    pub frames: Vec<u8>,
    pub current_frame: Option<u8>,
    pub completed_frames: u8,
    pub total_frames: u8,
    pub cooldown_remaining_seconds: u8,
    pub awaiting_holder_confirmation: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<BatchSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder_status: Option<HolderStatus>,
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
    pub batch: Option<BatchSnapshot>,
    pub recovery_notices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    Cancelled,
    TimedOut,
    ProcessExit(i32),
    IncompleteStream,
}
