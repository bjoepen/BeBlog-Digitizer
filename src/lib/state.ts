import type { AppSnapshot, ScanEvent } from "./types";

export const initialSnapshot: AppSnapshot = {
  adapter: "fake",
  scannerStatus: "ready",
  holderStatus: "unknown",
  activeJobId: null,
  state: "idle",
  progress: 0
};

export function applyScanEvent(snapshot: AppSnapshot, event: ScanEvent): AppSnapshot {
  const terminal = ["completed", "cancelled", "failed", "timedOut"].includes(event.state);
  return {
    ...snapshot,
    scannerStatus: terminal ? "ready" : "busy",
    holderStatus: event.state === "waitingForHolder" ? "unknown" :
      event.progress >= 24 ? "detected" : snapshot.holderStatus,
    activeJobId: terminal ? null : event.jobId,
    state: event.state,
    progress: event.progress
  };
}
