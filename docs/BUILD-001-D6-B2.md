# Build 001-D6-B2 – Scanner Lease and D5 Single-Frame Reuse

## Scope and safety

D6-B2 introduces one AppCore-owned scanner reservation and names the existing D5 production path as the reusable one-frame entry point. It does not add a D6 command, UI, batch loop, automatic retry, or production archive classification. Only existing fake/simulator test paths were exercised; no scanner, SANE enumeration, Minolta client, or hardware was used.

## Shared scanner lease

`ScannerLeaseManager` permits one owner among D1 simulation, D5 single frame, D6 batch, and scanner discovery. The owning `ScannerLeaseGuard` is held in `CoreData` for scan and batch jobs; discovery holds a local guard while enumeration runs. `ensure_idle` checks the active job, batch, and guard. Thus D1 simulation and D5 are serialized even though their adapters/controllers differ. A D6 orchestrator will be able to retain one `D6Batch` guard for the batch and call the internal D5 frame entry point while keeping that reservation.

D5 completion now has a separate `cleanup_confirmed` result. The flag begins false after spawn and becomes true only after OS wait/reaping, both stream closures, and both reader joins have succeeded. Core and guided-controller reservations are released only on that confirmation. A process start failure (no child started) is safe to release; an ambiguous post-spawn cleanup failure leaves the scanner reservation held and emits a diagnostic rather than permitting another scanner operation. Successful artifact/import/export failures occur after process cleanup and can release the scanner safely.

## Reusable D5 frame contract

`GuidedProcessController::start_single_frame` is the shared internal D5 entry point. It retains the fixed client resolver/arguments, `OPENED` then explicit user `CONTINUE`, real `HOLDER_DETECTED`, RGB8 profile checks, stream/protocol lifecycle, OS exit and reaping, artifact validation, archive import, export, and existing 30-second cooldown. `AppCore::start_guided_scan` remains the UI route and uses this same entry point; no second D6 scanner implementation was added.

## D6 validation receipt boundary

The B1 journal no longer accepts a caller-provided frame/capture pair as proof of validation. `mark_d6_frame_validated` accepts only an opaque `ValidatedD6Artifacts` receipt emitted by the strict four-file validator after reading fixed RAW, TIFF, parameter, and client-log artifacts. The journal persists their sizes and SHA-256 values together with the batch's previously reserved capture ID. Mismatched IDs, incomplete receipts, or failed content checks cannot move the journal to `Validated`.

The D5 importer currently validates and finalizes its capture as one operation and does not accept a batch ID. D6-B2 does not alter that behavior or claim an integrated transaction. D6-B3 must add a D6-specific handoff that retains the exclusive work area, validates into this receipt, persists `Validated`, finalizes/exports the same files, and only then persists `Completed`; a journal failure must stop progression. Re-validation at finalization and comparison against the stored digests are required to protect against changed work files.

## Verification

Unit tests cover lease exclusivity across all action types, reservation during the OPENED/user-confirmation wait, fail-closed retention on uncertain cleanup, release after confirmed cleanup, and opaque artifact receipts. Existing D1 batch, D5 protocol/process, archive, and guided process E2E tests remain required regression gates.

Final hardware-free checks on the D6-B2 working tree:

- `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`: PASS (129 unit tests passed, 12 intentionally ignored child-process fixtures; 2 guided-process E2E tests passed).
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: PASS.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: PASS.
- `pnpm check`: PASS.
- `pnpm test`: PASS (17 tests across 4 files).
- `pnpm build`: PASS.
- `git diff --check`: PASS.

No scanner, SANE enumeration, Minolta client, or hardware was used. No PR or merge was created.
## Deferred to D6-B3

- No D6 batch execution or per-frame automatic progression.
- No batch GUI or Tauri batch-start command.
- No shared D6 batch-to-D5 completion channel yet.
- No integrated batch capture/archive/export transaction.
- No real hardware test. The holder remains inserted between frames as reported in D5 hardware observations; each new process must still perform its own OPENED/CONTINUE/HOLDER_DETECTED handshake. The cooldown remains Rust-enforced.
