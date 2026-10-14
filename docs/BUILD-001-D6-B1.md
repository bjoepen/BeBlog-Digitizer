# Build 001-D6-B1 – Batchplan and Journal Contract

## Scope

D6-B1 adds a hardware-free plan and durable journal contract for ordered batches of two through six frames. It does not invoke the scanner, start a process, expose a batch command to the UI, or sequence D5 scans. D1 and D5 runtime behavior is unchanged.

## Plan and identifiers

`D6BatchRequest` defaults to frames `[1, 2, 3, 4]`. Validation accepts only sequences `1..N` where `N` is 2–6. Empty/singleton, more than six, non-contiguous, reordered, or offset plans are rejected. A batch receives a UUID-based `d6-batch-*` ID. Each frame ID is derived from that batch ID and its position; each frame's eventual capture ID is separately recorded when the frame is reserved.

## Journal layout and compatibility

D6 journals are stored under `batches/d6/<batch-id>.json`, separate from the existing D1 `batches/<batch-id>.json` schema. Creation uses exclusive file creation. Updates write a unique temporary file and atomically replace only that journal. D1 journals keep their existing layout and recovery parser.

Batch states: `Planned`, `Running`, `Completed`, `Failed`, `Cancelled`, `Interrupted`.

Frame states: `Planned`, `Running`, `Validated`, `Completed`, `Failed`, `Cancelled`, `Interrupted`.

Frames may only be reserved in order, and only after all earlier frames are journaled `Completed`. Reservation stores the capture ID. The intended future D6 order is: reserve frame/capture association → validate all source artifacts → persist `Validated` → finalize capture archive → persist frame `Completed`. Thus archive finalization must not occur before the `Validated` association has been durably written, and a batch is complete only after every frame completion update succeeds. B1 supplies the journal barriers but does not yet connect them to D5 artifact finalization; that integration is a later build.

Recovery marks any D6 `Planned` or `Running` batch `Interrupted`; a `Running` or `Validated` frame becomes `Interrupted`. It never resumes automatically. Completed frame records remain intact.

## Explicit boundaries

- D1 remains the existing 1–4 FakeScannerAdapter simulation and is not converted to real batch scanning.
- D5 remains a manually started single-frame workflow.
- No production capture classification is enabled by this journal.
- Process execution, shared scanner lease, per-frame OPENED/CONTINUE/holder-detection, cooldown, export, and UI are not part of D6-B1.
- On a journal write failure, the caller must stop progression and must not report the frame or batch completed. A damaged journal is left intact for diagnosis.
