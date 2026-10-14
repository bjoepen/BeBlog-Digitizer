# Build 001-D6-B3 – Guided Batch Orchestrator

## Scope and safety

D6-B3 connects the existing D6 plan/journal, shared scanner lease, and D5 guided single-frame process. Batch execution is implemented against the existing controlled guided-process path; automated integration coverage uses only the app's harmless child-process simulator. No real scanner process, SANE discovery, or hardware was used. The pinned Minolta client, SANE/avision installation, Research-009 archive, Gamma configuration, `/opt/homebrew`, and `tools/minolta-hardware-test.sha256` are not part of this change. `CaptureKind::Production` remains unavailable.

## Runtime sequence

The core validates a 2–6-frame contiguous plan (`1..=N`, default 4), reserves the shared `ScannerAction::D6Batch` lease, and creates the D6 journal before starting work. The lease remains held across holder confirmation, every process, artifact handling, and cooldown.

For each frame, the core persists the frame/capture reservation, opens a fresh capture staging area, and invokes the same guided D5 process path. Frame 1 waits for real `OPENED`, then requires the explicit UI confirmation before sending `CONTINUE`. Later frames wait for their own actual `OPENED` event and send `CONTINUE` automatically. Every frame still requires an actual `HOLDER_DETECTED`; no timing or prior frame state substitutes for protocol events.

After process completion, stream closure, cleanup, and reaping, the four artifacts are validated together. A private `ValidatedD6Artifacts` receipt owns the exact validated file bytes and their digests. The core persists `Validated` and its capture association, finalizes the archive from those receipt-owned bytes, exports those same bytes, verifies persisted sizes/checksums, then writes `Completed`. A failure at any step stops the batch without retry. Previously completed captures remain intact. Between successful non-final frames, an injectable core clock enforces 30 one-second cooldown ticks; there is no final cooldown.

Cancellation is shared with the active process and checked during guided wait, scanning, and cooldown. The lease is released only after process cleanup is confirmed. If cleanup is uncertain, the scanner remains reserved and the UI reports that it is locked. Recovery marks in-progress work interrupted; it never resumes a batch automatically.

## UI and events

The Svelte batch panel defaults to four images and permits 2–6, with start, first-frame holder confirmation, and cancellation. Typed Tauri events expose batch ID, phase, frame, total, completed count, countdown, and message. The existing right-side scanner/process inspector remains in place for byte progress and diagnostics. D5 single-frame controls remain separate and are disabled while a batch owns the lease.

## Verification

The hardware-free D6 integration tests exercise 2-, 4-, and 6-frame plans, one fresh simulator child per frame, explicit first-frame confirmation, follow-frame continuation only after each `OPENED`, per-frame holder detection, EOF, four validated artifacts, unique capture IDs, ordered journal completion, and exports. Cooldown timing is core-owned and tested with injected clocks; cancellation during cooldown leaves the completed first capture intact and proves that the second frame remains unstarted. A separate archive test asserts the durable ordering `Validated -> finalized/exported -> Completed`. Existing D1/D5 and process tests remain in the Rust suite.

## Build and test results

- `cargo fmt -- --check`: PASS.
- `cargo test -- --test-threads=1`: PASS, 133 unit tests and 3 process E2E tests; 12 OS-runner child harness tests are intentionally ignored in the ordinary suite.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `pnpm check`: PASS.
- `pnpm test`: PASS, 4 test files / 17 tests.
- `pnpm build`: PASS.
- `pnpm tauri build --bundles app`: PASS; ARM64 app bundle produced at `src-tauri/target/release/bundle/macos/BeBlog Digitizer.app` (11.83 MiB).
- `git diff --check`: PASS.

The first frontend test run exposed that the test mock retained only the most recently registered Tauri listener; registering the new D6 event listener consequently displaced the existing guided-client listener in tests. The mock now stores handlers by event name, and the full frontend suite passes. No production event behavior was changed to accommodate the mock.

## Remaining gates

- Hardware acceptance is still required for multi-frame continuation with the holder left inserted, repeated real `OPENED`/`HOLDER_DETECTED` behavior, and physical cooldown behavior.
- The current automated integration uses the protocol simulator and synthetic artifacts; it does not prove real SANE cleanup or hardware behavior.
- Production capture classification remains locked. No real multi-frame batch is authorized by this build.
