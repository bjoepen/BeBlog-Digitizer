import { describe, expect, it } from "vitest";
import { confirmHolderInsertion, initialGuidedProjection, projectHardwareEvent } from "./guided";

describe("guided hardware event projection", () => {
  it("never prompts for insertion before a successful sane_open event", () => {
    const early = confirmHolderInsertion({ ...initialGuidedProjection, state: "openingScanner" });
    expect(early.state).toBe("openingScanner");
    const opened = projectHardwareEvent(early, { kind: "opened" });
    expect(opened.state).toBe("awaitingHolderConfirmation");
    expect(opened.holderDetected).toBe(false);
  });

  it("user confirmation does not fabricate hardware detection", () => {
    const opened = projectHardwareEvent({ ...initialGuidedProjection, state: "openingScanner" }, { kind: "opened" });
    const confirmed = confirmHolderInsertion(opened);
    expect(confirmed.state).toBe("waitingForHolderDetection");
    expect(confirmed.holderDetected).toBe(false);
    const detected = projectHardwareEvent(confirmed, { kind: "holderDetected" });
    expect(detected.holderDetected).toBe(true);
    expect(detected.state).toBe("positioningFrame");
  });

  it("accepts simulated full event sequence and rejects inconsistent bytes", () => {
    let state = projectHardwareEvent({ ...initialGuidedProjection, state: "openingScanner" }, { kind: "opened" });
    state = confirmHolderInsertion(state);
    state = projectHardwareEvent(state, { kind: "holderDetected" });
    state = projectHardwareEvent(state, { kind: "framePositioned", frame: 1 });
    state = projectHardwareEvent(state, { kind: "parameters", dpi: 2820, width: 1, height: 1, bytesPerLine: 3, depth: 8, format: "RGB", expectedBytes: 3 });
    state = projectHardwareEvent(state, { kind: "scanStarted" });
    state = projectHardwareEvent(state, { kind: "dataChunk", chunkBytes: 3, totalBytes: 3 });
    state = projectHardwareEvent(state, { kind: "eof", totalBytes: 3 });
    expect(state.state).toBe("finalizing");
    state = projectHardwareEvent(state, { kind: "tiffWritten" });
    state = projectHardwareEvent(state, { kind: "scannerClosing" });
    state = projectHardwareEvent(state, { kind: "complete" });
    expect(state.state).toBe("completed");

    const inconsistent = projectHardwareEvent(state, { kind: "dataChunk", chunkBytes: 1, totalBytes: 4 });
    expect(inconsistent.transferredBytes).toBe(3);
  });

  it("latches sane_open failure and never advances on diagnostics", () => {
    const opening = { ...initialGuidedProjection, state: "openingScanner" as const };
    const failed = projectHardwareEvent(opening, { kind: "openFailed", detail: "device unavailable" });
    expect(failed.state).toBe("failed");
    expect(failed.error).toBe("device unavailable");
  });
});
