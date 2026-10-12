export type ScanState =
  | "idle" | "initializing" | "openingScanner" | "waitingForHolder"
  | "configuringFrame" | "startingScan" | "transferring" | "finalizing"
  | "completed" | "cancelling" | "cancelled" | "failed" | "timedOut"
  | "awaitingHolderConfirmation" | "waitingForHolderDetection" | "positioningFrame"
  | "scanning" | "closingScanner" | "interFrameCooldown" | "batchCompleted" | "batchFailed" | "batchCancelled";

export type SimulationMode = "success" | "error" | "timeout" | "incomplete" | "missingOutput" | "invalidOutput";
export type HolderStatus = "unknown" | "detected";

export interface BatchSnapshot { batchId: string; frames: number[]; currentFrame: number | null; completedFrames: number; totalFrames: number; cooldownRemainingSeconds: number; awaitingHolderConfirmation: boolean; }

export interface ScanEvent {
  jobId: string;
  state: ScanState;
  progress: number;
  message: string;
  level: "info" | "warning" | "error";
  timestampMs: number;
  batch?: BatchSnapshot;
  holderStatus?: HolderStatus;
}

export interface AppSnapshot {
  adapter: "fake";
  scannerStatus: "ready" | "busy";
  holderStatus: "unknown" | "detected";
  activeJobId: string | null;
  state: ScanState;
  progress: number;
  batch: BatchSnapshot | null;
  recoveryNotices: string[];
}

export interface StartScanRequest {
  frame: number;
  simulation: SimulationMode;
}

export interface StartBatchRequest { frames: number[]; simulation: SimulationMode; failureFrame: number | null; }
