import { describe, expect, it } from "vitest";
import { applyScanEvent, initialSnapshot } from "./state";

describe("applyScanEvent", () => {
  it("marks the scanner busy during transfer", () => {
    const next = applyScanEvent(initialSnapshot, {
      jobId: "job-1", state: "transferring", progress: 52,
      message: "Datenübertragung", level: "info", timestampMs: 1
    });
    expect(next.scannerStatus).toBe("busy");
    expect(next.holderStatus).toBe("detected");
    expect(next.activeJobId).toBe("job-1");
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
