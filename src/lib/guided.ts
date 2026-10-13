export type HardwareClientEvent =
  | { kind: "opened" }
  | { kind: "openFailed"; detail: string }
  | { kind: "holderDetected" }
  | { kind: "framePositioned"; frame: number }
  | { kind: "parameters"; dpi: number; width: number; height: number; bytesPerLine: number; depth: number; format: string; expectedBytes: number }
  | { kind: "scanStarted" }
  | { kind: "dataChunk"; chunkBytes: number; totalBytes: number }
  | { kind: "eof"; totalBytes: number }
  | { kind: "tiffWritten" }
  | { kind: "scannerClosing" }
  | { kind: "complete" }
  | { kind: "error"; detail: string };

export type GuidedState = "idle" | "openingScanner" | "awaitingHolderConfirmation" | "waitingForHolderDetection" | "positioningFrame" | "configuringFrame" | "scanning" | "transferring" | "finalizing" | "closingScanner" | "completed" | "failed";

export interface GuidedProjection {
  state: GuidedState;
  holderDetected: boolean;
  transferredBytes: number;
  expectedBytes: number | null;
  tiffWritten: boolean;
  error: string | null;
}

export const initialGuidedProjection: GuidedProjection = {
  state: "idle", holderDetected: false, transferredBytes: 0, expectedBytes: null, tiffWritten: false, error: null
};

/** The user action only sends CONTINUE; only an explicit client event marks detection. */
export function projectHardwareEvent(current: GuidedProjection, event: HardwareClientEvent): GuidedProjection {
  switch (event.kind) {
    case "opened":
      return current.state === "openingScanner" ? { ...current, state: "awaitingHolderConfirmation" } : current;
    case "openFailed":
    case "error":
      return { ...current, state: "failed", error: event.detail };
    case "holderDetected":
      return current.state === "waitingForHolderDetection" ? { ...current, holderDetected: true, state: "positioningFrame" } : current;
    case "framePositioned":
      return current.state === "positioningFrame" ? { ...current, state: "configuringFrame" } : current;
    case "parameters":
      return current.state === "configuringFrame" ? { ...current, expectedBytes: event.expectedBytes } : current;
    case "scanStarted":
      return current.state === "configuringFrame" && current.expectedBytes !== null ? { ...current, state: "transferring" } : current;
    case "dataChunk":
      return current.state === "transferring" && event.chunkBytes >= 0 && event.totalBytes === current.transferredBytes + event.chunkBytes && (current.expectedBytes === null || event.totalBytes <= current.expectedBytes)
        ? { ...current, transferredBytes: event.totalBytes, state: "scanning" } : current;
    case "eof":
      return current.state === "scanning" && event.totalBytes === current.transferredBytes && current.expectedBytes === event.totalBytes ? { ...current, state: "finalizing" } : current;
    case "tiffWritten":
      return current.state === "finalizing" ? { ...current, tiffWritten: true } : current;
    case "scannerClosing":
      return current.state === "finalizing" && current.tiffWritten ? { ...current, state: "closingScanner" } : current;
    case "complete":
      return current.state === "closingScanner" ? { ...current, state: "completed" } : current;
  }
}

export function confirmHolderInsertion(current: GuidedProjection): GuidedProjection {
  return current.state === "awaitingHolderConfirmation"
    ? { ...current, state: "waitingForHolderDetection" }
    : current;
}
