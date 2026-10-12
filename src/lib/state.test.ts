import { describe, expect, it } from "vitest";
import { applyScanEvent, initialSnapshot } from "./state";

describe("applyScanEvent", () => {
  it("does not infer holder detection from progress", () => {
    const next = applyScanEvent(initialSnapshot, {
      jobId: "job-1", state: "transferring", progress: 52,
      message: "Datenübertragung", level: "info", timestampMs: 1
    });
    expect(next.scannerStatus).toBe("busy");
    expect(next.holderStatus).toBe("unknown");
    expect(next.activeJobId).toBe("job-1");
  });

  it("projects explicit typed holder feedback", () => {
    const next = applyScanEvent(initialSnapshot, {
      jobId: "job-1", state: "waitingForHolder", progress: 20,
      message: "Mock holder detected", level: "info", timestampMs: 1, holderStatus: "detected"
    });
    expect(next.holderStatus).toBe("detected");
    const reset = applyScanEvent(next, { jobId: "job-2", state: "initializing", progress: 80, message: "reset", level: "info", timestampMs: 2, holderStatus: "unknown" });
    expect(reset.holderStatus).toBe("unknown");
  });

  it("releases the scanner after cancellation", () => {
    const next = applyScanEvent(initialSnapshot, {
      jobId: "job-1", state: "cancelled", progress: 31,
      message: "Abgebrochen", level: "warning", timestampMs: 1
    });
    expect(next.scannerStatus).toBe("ready");
    expect(next.activeJobId).toBeNull();
  });

  it("keeps the batch lock and countdown visible during cooldown", () => {
    const next = applyScanEvent(initialSnapshot, {
      jobId: "batch-1", state: "interFrameCooldown", progress: 100,
      message: "Bereitschaftspause", level: "info", timestampMs: 2,
      batch: { batchId: "batch-1", frames: [1, 2, 3, 4], currentFrame: 1, completedFrames: 1, totalFrames: 4, cooldownRemainingSeconds: 30, awaitingHolderConfirmation: false }
    });
    expect(next.scannerStatus).toBe("busy");
    expect(next.batch?.cooldownRemainingSeconds).toBe(30);
    expect(next.activeJobId).toBe("batch-1");
  });
});
