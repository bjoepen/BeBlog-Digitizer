# Build 001-D3-B3a — Offline Artifact Import & Validation

## Executive Summary

**PASS** — Implemented a hardware-free import API for one explicitly selected `.raw`, `.tiff`, `.params.txt`, `.client.log` set. It reserves a new private staging directory, copies only fixed artifact names, validates all four inputs, and archives all original bytes with sizes and SHA-256 digests.

**PASS** — Imported captures use `captureKind: offlineImport`. Their `completed` status means the offline artifacts passed validation and were durably archived; it does not mean a scanner or process completed. The returned report explicitly says process exit/reaping and SANE cleanup were not observed.

**PASS** — Existing `CaptureManifest`/`FileMetadata`, collision-safe `create_new`, SHA-256 checks, `TransferAccumulator`, `SidecarLogParser`, and interrupted-job recovery were reused. The FakeScannerAdapter and batch path remain unchanged.

**BLOCKER for real integration** — Generic archive entry points now refuse `Production`; a future real adapter needs a separate typed process-evidence capability before production manifests may be finalized.

## Read-only Baseline Audit

- **PASS — Archive:** `archive.rs` already created capture directories exclusively, wrote originals with `create_new`, recorded byte counts/SHA-256, checked written files against the manifest, and recovered `inProgress` captures as `Interrupted`. Existing manifests without a `captureKind` remain `UnclassifiedLegacy` through a serde default.
- **WARN — Existing TIFF validator:** The production validator in `archive.rs` checked classic TIFF framing, IFD and data ranges, and strip/tile presence. It did not require RGB8 tags or reconcile TIFF dimensions/strip byte counts with the scanner profile. It was not suitable as the only validator for this import.
- **PASS — Log parser:** `SidecarLogParser` represents each `sane_read` as a single chunk, distinguishes EOF from `capture ended: total=...`, parses TIFF/client-exit markers, and preserves unknown diagnostics. `TransferAccumulator` safely sums chunks and rejects overflow, over-profile totals, and disagreement with the client total.
- **PASS — Process lifecycle boundary:** `ProcessLifecycle` checks actual process events, exit status, reaping, streams, and cleanup in the FakeScannerAdapter flow. Offline import does not synthesize or invoke that lifecycle.
- **OPEN — Existing import/staging:** No prior offline artifact import or app-owned per-import staging existed. D3-B3a adds this separately without making a process runner selectable.

## Client/Profile Evidence

**PASS** — The D3-B1 client contract documents the metadata keys and successful RGB8/600-dpi profile. The C source at `~/Minolta-Research-005-PASS/minolta-batch-frame.c` was inspected read-only for `write_metadata()` and `raw_to_tiff()`; no client, scan, or hardware command was run.

The fixed accepted profile is RGB, 8 bits/channel, 600 dpi, 564 × 850 pixels, 1,692 bytes/line, and 1,438,200 RAW bytes. Arithmetic is checked (`564 × 3 = 1,692`; `1,692 × 850 = 1,438,200`). Metadata must agree on `format`, `bytes_per_line`, `pixels_per_line`, `lines`, `depth`, `raw_bytes`, and EOF `terminal_status=5`, and include a nonempty device field. If metadata includes a frame key, it must match the requested frame. DPI and color mode are checked against the client diagnostic records and parameter snapshots.

**PASS** — The inspected C writer emits a classic little-endian, uncompressed, single-strip RGB TIFF. The strict validator checks TIFF signature/byte order, classic magic, bounded IFD and value offsets, rejects duplicate/unknown tags and multiple pages, requires the profile dimensions, RGB photometric interpretation, 8/8/8 BitsPerSample, 3 SamplesPerPixel, no compression, chunky planar layout, one complete strip whose byte count equals the RAW profile, and unsigned SampleFormat. Resolution tags are absent in this client’s TIFF output; if X/Y resolution is present, both must consistently declare 600 dpi and inch units. The log/parameters and TIFF geometry are cross-checked instead of requiring tags the client does not write.

**OPEN** — This exact parser has synthetic TIFF coverage only. A supplied real `.tiff` offline sample would be needed to confirm any implementation drift from the inspected C writer; no Research scan image was imported or used as a test fixture.

## Import, Staging, and Archive Contract

1. `import_offline_artifacts()` accepts four explicit source paths and frame 1–4 only.
2. It canonicalizes the archive root, rejects a symlink/non-directory `.staging`, and atomically reserves a UUID directory with `create_dir`.
3. Sources must be regular non-symlink files. On Unix, source/staged reads use `O_NOFOLLOW`; staged destinations use fixed names and `create_new`. Size caps bound RAW, TIFF, metadata, and client-log reads.
4. Every copied staged artifact gets a size and SHA-256 record; the staged copies are re-read and checked against those records before validation. Failure leaves the stage and marks it `failed` where persistence permits.
5. The importer validates the fixed profile, strict TIFF structure, and successful client-log claim ordering. It ignores all paths printed in the log; paths never select or open files.
6. The existing archive creates a unique `offline-<UUID>` capture directory, writes `capture.raw`, `capture.tiff`, `capture.params.txt`, and `capture.client.log` with `create_new`, records checksums, re-reads and verifies every archived file, then writes `completed` last.
7. Archive write/validation errors leave a failed manifest when it can be persisted; any partial originals remain in place and are never overwritten. On startup, incomplete staging is marked `interrupted` and is never retried/resumed automatically.

If the archive is already durably completed but the final staging-status update fails, the import report remains successful and carries a warning with both IDs. The archive manifest remains authoritative; startup may mark the stale staging ledger `interrupted`, and no retry occurs.

Successful staging copies are retained with status `archived`, and failed copies are retained for diagnosis. This intentionally duplicates data between staging and archive; a later retention/cleanup policy is **OPEN** and must not delete source originals.

## Log and Process Evidence

**PASS** — The offline log validator uses the existing parser and accumulator. It requires ordered initialization/open/holder/frame setup/scan start, RGB and 600-dpi selection, matching pre/post parameter snapshots, chunks totaling 1,438,200 bytes, one EOF, a matching final total with terminal status 5, metadata/TIFF write claims, `sane_close`, `sane_exit`, and exactly one `client exit -> 0`. Unknown diagnostics are tolerated; malformed known records and read failures are rejected. TIFF paths in log lines are never followed.

**IMPORTANT LIMIT** — `.client.log` is a client diagnostic mirror. The log’s `client exit -> 0`, `sane_close`, and `sane_exit` strings are only text claims. They do not prove OS process exit status, child reaping, stream EOF, or successful SANE cleanup. `OfflineImportReport` returns `NotObservedByOfflineImport` for both process and cleanup evidence. No `ProcessOutcome` or `ProcessLifecycle` success is manufactured.

## Changes

- `src-tauri/src/offline_import.rs`: explicit source API, bounded exclusive staging, metadata/log/TIFF profile validators, SHA-256 staging checks, report with explicit missing process evidence, and synthetic regression tests.
- `src-tauri/src/archive.rs`: `CaptureKind::OfflineImport`, exclusive offline capture creation/finalization, four-file manifest integration, staging recovery, and production-path guards.
- `src-tauri/src/lib.rs`: exports the offline-import API as Rust functionality only. No Tauri command/UI path was added, and no scanner process is startable through it.
- `src-tauri/Cargo.toml`: makes the already-used `libc` dependency available for Unix `O_NOFOLLOW` flags.

## Regression Coverage

- Valid synthetic four-file RGB8/600-dpi artifact set; offline classification; all four preserved archive originals and verified checksums.
- Missing source, wrong RAW size, contradictory metadata.
- Invalid IFD offset, invalid strip offset, non-baseline compression, and truncated pixel data.
- Symlink source rejection and malicious diagnostic output paths ignored.
- Archive filename collision preserves pre-existing bytes and leaves a failed capture manifest.
- Interrupted staging recovery changes status to `interrupted` without resuming.
- Generic archive APIs cannot create Production captures; Production finalization remains blocked until a future process-proof API exists.
- Entire prior FakeScannerAdapter, D2 process-lifecycle, parser, archive, recovery, and four-frame batch suite.

## Quality and Safety

| Check | Result |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml` | PASS — 78 passed, 12 intentionally ignored child entry points |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | PASS |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | PASS |
| `pnpm check` | PASS — TypeScript |
| `pnpm test` | PASS — 3 files, 7 tests |
| `pnpm build` | PASS — Vite production build |
| `pnpm tauri build --bundles app` | PASS — native macOS `.app` bundled |

All import fixtures are synthetic. No Minolta client or scanner process, SANE command/library, scanner hardware, `/opt/homebrew`, Research-005/009 image artifact, backend patch, or gamma configuration was changed or used for importing/testing. The existing C source was read only to verify its text/TIFF writer contract.

## Open Questions for D3-B3b

- **OPEN:** Provide a separately approved offline sample set if compatibility with TIFF variants beyond the inspected one-strip C output must be assessed. Unsupported variants are rejected today.
- **OPEN:** Decide the user-facing file-selection flow and source-path authorization. This build exposes a Rust API only; no UI command accepts arbitrary paths.
- **BLOCKER:** Define an unforgeable typed process result combining regular OS exit, reaping, stream completion, and cleanup result before enabling `CaptureKind::Production`.
- **OPEN:** Decide whether successful `.staging` copies are retained, moved, or safely pruned after durable archive finalization.
- **OPEN:** Define archive quotas and caps if future client logs/TIFFs exceed the current 16 MiB/4 MiB bounds.

## D3-B3a.1 Audit Corrections

**PASS — Pixel-level RAW/TIFF consistency:** The strict validator now returns the validated single-strip TIFF pixel range after checking the accepted uncompressed RGB8 layout and exact profile byte count. Artifact validation compares that complete range byte-for-byte with the associated RAW data. Structural TIFF validity alone can no longer accept altered pixels. Regression coverage accepts identical synthetic pixels and rejects a one-byte mutation independently in either the TIFF pixels or RAW file.

**PASS — Crash-safe staging journal temporary files:** Journal updates now create a fresh UUID-named temporary file with exclusive `create_new` semantics for each attempt. A temporary file left by an interrupted write is not reused, overwritten, or deleted; recovery ignores that uncommitted temporary and writes the interrupted status through a new unique file. A regression test leaves a PID-style partial temp file in place, runs recovery, and confirms its bytes remain unchanged while the stage becomes `interrupted`.

These changes affect only offline artifact validation and staging metadata. No production scanner activation, scanner process, SANE, Homebrew, Research artifact, or Minolta client was used or modified.

**D3-B3a.1 verification:** `cargo test --manifest-path src-tauri/Cargo.toml` PASS (79 passed, 12 intentionally ignored); `cargo fmt --manifest-path src-tauri/Cargo.toml --check` PASS; `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` PASS; `pnpm check` PASS; `pnpm test` PASS (7/7); `pnpm build` PASS; `pnpm tauri build --bundles app` PASS (native macOS app bundle). `git diff --check` PASS. Tests/build were hardware-free; no scanner-related executables were started.
