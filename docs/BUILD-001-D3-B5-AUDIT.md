# Build 001-D3-B5 — Minolta Single-Frame Integration Audit

**Status: BLOCKED for scanner integration; hardware path remains disabled.**

This build audit is based on main after PR #14 (d4bd94a7c10210ec33a8aa2382d52eef685825f0) and the feature branch build/001-d3-b5-minolta-single-frame. No scanner, SANE command, Minolta client, or hardware test was run. The only change in this commit is this audit document: the current client/profile and artifact validators do not support the requested 2,820-dpi contract, so implementing or enabling a purported complete scan workflow would be unsafe and would claim unverified behavior.

## Integration overview

- **PASS — Existing scan path:** AppCore registers FakeScannerAdapter as its only adapter (src-tauri/src/core.rs). Tauri exposes the simulation commands (src-tauri/src/lib.rs); there is no command for resolving or starting a Minolta client. Existing single-scan state transitions, cancellation, archive journaling, and recovery remain available for simulation.
- **PASS — Process boundary:** ProcessRunner and RecordedProcessRunner in src-tauri/src/process.rs serve the recorded/fake flow. OsProcessRunner in src-tauri/src/os_process_runner.rs is test-only and starts only allowlisted Rust test-harness modes. SidecarResolver in src-tauri/src/sidecar.rs resolves only the D3-B4 harmless ARM64 test sidecar. It does not provide a Minolta executable or a production start API.
- **PASS — Artifact path:** OfflineArtifactImporter in src-tauri/src/offline_import.rs uses fixed artifact names, validates all four offline files and their contents, and archives them as OfflineImport; it does not prove OS process completion or SANE cleanup. The archive explicitly rejects Production finalization absent future process evidence (src-tauri/src/archive.rs).
- **BLOCKER — Requested profile mismatch:** Archive manifests and validators are fixed to RGB8/600 dpi, 564 × 850 pixels, 1,692 bytes per line, and 1,438,200 RAW bytes. The strict offline importer likewise accepts only this profile. No 2,820-dpi dimensions, byte count, or matching TIFF profile are established in this codebase.
- **BLOCKER — No production workflow:** There is no active Minolta adapter, production Tauri command, production sidecar resource, or production archive capability. Those protections must remain in place.

## Client provenance and source findings

minolta-batch-frame.c and the executable are **not tracked in this repository** (git ls-files '*minolta*' returns no client files). A local source copy exists at /Users/bernd/Minolta-Research-005-PASS/minolta-batch-frame.c (16,013 bytes; SHA-256 35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae). It was read only and not executed, compiled, or modified. Its source/version provenance is not pinned by a Git commit or checked-in source snapshot in this repository; the local path and digest identify the inspected copy, not an independently reproducible release.

The versioned client contract is docs/BUILD-001-D3-B1-CLIENT-CONTRACT.md. It documents -d DEVICE, -o PREFIX, -f FRAME, -t SECONDS, and -i SECONDS; accepted frames are 1–4. In the inspected C source, getopt() has no DPI argument and line 302 passes literal 600 to the resolution option. The only established client profile is the successful RGB8/600-dpi profile represented in existing Research-005/009 fixtures. -t limits holder polling, not total scan runtime.

### 2,820-dpi assessment

- **PROVEN (source):** This C client fixes the requested resolution at 600 dpi. The existing command line cannot request 2,820 dpi.
- **OPEN:** The inspected source does not establish that this scanner/backend accepts 2,820 dpi or what exact dimensions, line stride, raw length, or TIFF layout it returns at that setting. Such behavior cannot be inferred from the 600-dpi result.
- **BLOCKER:** A 2,820-dpi implementation requires an explicitly reviewed client change (for example, a strictly parsed, bounded resolution option or an intentionally fixed new profile) and a reproducibly identified source revision. This audit does not alter that external source or rebuild the client.
- **BLOCKER:** Before any 2,820-dpi artifact can be accepted, its parameter/metadata contract and TIFF/RAW geometry must be independently specified and validated. Do not loosen the existing 600-dpi validator or label a 600-dpi capture as 2,820 dpi.

No SANE backend change is proposed or implied. Whether the unchanged Research-009 backend can produce a valid 2,820-dpi RGB8 scan is **OPEN** and requires a separately authorized hardware acceptance test after the client and validator work is reviewed.

## Client artifact and risk notes

The documented client writes <prefix>.client.log, .raw, .params.txt, and .tiff; diag() writes diagnostics to stderr and mirrors them to the client log. Its source opens output files with "wb", so collision-safe, fresh staging is mandatory. A text client exit -> 0 is not an OS exit proof. The process lifecycle must additionally prove actual exit code, stream completion, reaping, and cleanup. The C source calls sane_cancel() on abnormal termination while a scan is active, then closes/exits SANE, but those void API calls do not provide a success result. A forced OS kill cannot prove SANE cleanup.

The existing offline importer can validate the current 600-dpi artifact set, but an offline import remains OfflineImport, not Production. The existing fake/test sidecar package must be preserved and must never be silently replaced by a Minolta binary.

## Safe integration boundary and next prerequisites

No 2,820-dpi scan workflow was wired into UI, Tauri, process execution, or archive finalization in this build. The safe sequence before considering a first hardware test is:

1. Pin and review the authoritative C-client source revision and licensing/build instructions in a controlled, versioned source reference.
2. Implement and test an explicit 2,820-dpi client/profile contract without changing SANE, avision, gamma tables, or Research-009 artifacts. Validate numeric argument parsing and all output paths.
3. Establish the expected 2,820-dpi SANE parameters, RAW size, TIFF structure, and metadata rules from an explicitly approved hardware test; do not derive them from 600 dpi by scaling assumptions.
4. Extend the importer and archive schema/profile validation to accept only the explicitly defined profile, retaining 600-dpi regression coverage and keeping Production finalization locked until process evidence and artifact evidence are joined to one capture.
5. Add a non-UI, separately gated integration boundary with fixed device/resource policy, exclusive staging, no retries/recovery, total-process timeout, cancellation/reaping checks, and user-confirmed holder insertion. Keep FakeScannerAdapter as the normal UI adapter until separately authorized.
6. Perform an independent audit and obtain explicit approval before a controlled hardware acceptance test.

**Conclusion:** The existing code provides reusable simulation, process-lifecycle test infrastructure, offline validation, archive, and recovery components. It does not provide a safe or truthful 2,820-dpi Minolta scan workflow. The production scanner start and CaptureKind::Production path must remain locked pending the prerequisites above.
