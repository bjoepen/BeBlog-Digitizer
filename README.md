# BeBlog Digitizer

BeBlog Digitizer is a private macOS application for guided film scanning with the Minolta DiMAGE Scan Dual II AF-2820U. It combines a Svelte/TypeScript interface with a Rust application core and uses the local SANE/avision installation through a pinned ARM64 client.

## Current status

Build 001-D6 has passed a real, GUI-guided four-frame batch on 14 October 2026. Frames 1–4 completed at 2,820 dpi in RGB8, each reporting 2,664 × 3,996 pixels and 31,936,032 RAW bytes. The hardware report documents the observed process sequence and its evidence limits: the client logs and parameter files were reviewed, but the RAW/TIFF pixel data and complete batch journal/export manifests were not independently audited. `CaptureKind::Production` therefore remains locked.

Real GUI-guided single-frame scans were previously demonstrated at positions 3, 5 and 4. Manual single-frame selection supports positions 1–6; this does not mean every position has been hardware-tested. Automated scanner discovery, device selection and explicit confirmation are available in the app.

## Hardware and platform

- Minolta DiMAGE Scan Dual II AF-2820U.
- Native macOS on Apple Silicon (`arm64`), for private local use.
- Existing local SANE/avision installation required; the app does not install or modify it.
- Scanner discovery is user-initiated. The user selects and confirms the detected device; the app does not infer a stale USB address.
- Intel Macs, Rosetta and other scanner models are not supported.

## Scanning workflows

### Guided batch

The D6 workflow supports 2–6 contiguous frames (`1..N`), with four selected by default. The successful real D6-HW1 acceptance covered four frames, 1–4. The first process must report `OPENED` before the app asks the user to insert and confirm the holder. For each later frame, the app starts a new client process and only automatically sends `CONTINUE` after that process reports its own `OPENED`; actual holder detection is still required.

Every frame uses a separate process and scanner connection. After successful completion and cleanup, the Rust core enforces a 30-second cooldown between frames. There is no automatic retry or batch resume after interruption. Completed captures remain available if a later frame fails.

### Single-frame scans

Manual individual scans are available for positions 1–6 at 600 or 2,820 dpi in RGB8. Each scan is explicitly started by the user, uses its own client process, and follows the guided open/holder handshake. Repeated real single-frame scans have been demonstrated with a 30-second cooldown. The historical D1 four-frame simulation is separate from both the D5 manual single-frame and D6 real batch workflows.

## Captures and development

The client produces RAW, TIFF, parameter and client-log artifacts. The application workflow validates artifacts before export to the chosen folder, including external storage. The D6-HW1 logs and parameter files establish the reported scan profile and client lifecycle, but do not independently establish RAW/TIFF pixel identity or independently audited archive and export manifests. See the [D6-HW1 hardware report](docs/BUILD-001-D6-HW1-HARDWARE-PASS.md) for exact boundaries.

BeBlog Digitizer does not develop negatives. Use a separate tool such as NegPy or GIMP for negative conversion, color correction and derivative images. No integrated preview, automatic white balance or color correction is provided.

## Architecture

- Svelte and TypeScript provide the guided interface.
- Tauri v2 connects the interface to the Rust Application Core.
- Rust owns scanner exclusivity, process lifecycle, batch state, cooldown, artifact validation and journaling.
- The fixed ARM64 Minolta client is checked against an explicit SHA-256 pin and uses the existing SANE/avision installation.
- A harmless child-process simulator supports hardware-free protocol, UI and batch regression tests.

## Development

Requirements: an Apple Silicon Mac with the project’s Rust and Node/pnpm toolchains.

```sh
pnpm install
pnpm validate
pnpm tauri dev
```

Useful individual checks:

```sh
pnpm check
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build --bundles app
```

Starting the app does not itself initiate scanner discovery or a scan. Hardware operations are explicit GUI actions; do not use development commands for a hardware test unless that test is separately authorized.

## Current limitations

- Four-frame batch (positions 1–4) is the real D6-HW1 hardware-accepted sequence. Six-position batch execution is implemented but has not been established by that acceptance.
- The D6-HW1 evidence did not independently byte-compare RAW and TIFF pixels or audit the complete batch journal/export manifests.
- `CaptureKind::Production` remains locked.
- The 30-second cooldown is app-session state and does not persist across restarts.
- No automatic retry, automatic batch recovery, integrated negative development or image editing.
- No general support for other scanners, Intel Macs, Rosetta or public distribution.

## Project records

- [D6-HW1: real four-frame batch hardware pass](docs/BUILD-001-D6-HW1-HARDWARE-PASS.md)
- [D6-B3: batch orchestrator, tests and remaining hardware gates](docs/BUILD-001-D6-B3.md)
- [D5-HW3: real single-frame hardware milestones](docs/BUILD-001-D5-HW3-HARDWARE-PASS.md)
- [D5 implementation audit](docs/BUILD-001-D5-AUDIT.md)
- [D4 guided hardware workflow audit](docs/BUILD-001-D4-AUDIT.md)
- [Research-009 correction](docs/RESEARCH-009-CORRECTION.md)
- [D1 batch simulation](docs/BUILD-001-D1.md)
