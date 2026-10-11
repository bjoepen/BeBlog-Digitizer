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
});
