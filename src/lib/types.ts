export type ScanState =
  | "idle" | "initializing" | "openingScanner" | "waitingForHolder"
  | "configuringFrame" | "startingScan" | "transferring" | "finalizing"
  | "completed" | "cancelling" | "cancelled" | "failed" | "timedOut";

export type SimulationMode = "success" | "error" | "timeout";

export interface ScanEvent {
  jobId: string;
  state: ScanState;
  progress: number;
  message: string;
  level: "info" | "warning" | "error";
  timestampMs: number;
}

export interface AppSnapshot {
  adapter: "fake";
  scannerStatus: "ready" | "busy";
  holderStatus: "unknown" | "detected";
  activeJobId: string | null;
  state: ScanState;
  progress: number;
}

export interface StartScanRequest {
  frame: number;
  simulation: SimulationMode;
}
