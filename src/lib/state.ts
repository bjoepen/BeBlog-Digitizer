import type { AppSnapshot, ScanEvent } from "./types";

export const initialSnapshot: AppSnapshot = {
  adapter: "fake",
  scannerStatus: "ready",
  holderStatus: "unknown",
  activeJobId: null,
  state: "idle",
  progress: 0,
  batch: null,
  recoveryNotices: []
};

export function applyScanEvent(snapshot: AppSnapshot, event: ScanEvent): AppSnapshot {
  const terminal = ["completed", "cancelled", "failed", "timedOut", "batchCompleted", "batchFailed", "batchCancelled"].includes(event.state);
  const waitingForConfirmation = event.state === "awaitingHolderConfirmation";
  return {
    ...snapshot,
    scannerStatus: terminal || waitingForConfirmation ? "ready" : "busy",
    holderStatus: event.holderStatus ?? snapshot.holderStatus,
    activeJobId: terminal ? null : event.jobId,
    state: event.state,
    progress: event.progress,
    batch: terminal ? null : (event.batch ?? snapshot.batch)
  };
}
