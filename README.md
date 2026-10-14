# BeBlog Digitizer

BeBlog Digitizer is a private macOS application for guided, single-frame film scanning with the Minolta DiMAGE Scan Dual II AF-2820U. The native interface is built with Tauri v2 and Svelte; a pinned ARM64 Minolta client performs each real scan as a separate process.

## Current status

Build 001-D5 has passed real hardware checks for scanner discovery and three repeated manual scans. On 14 October 2026, frames 3, 5 and 4 were scanned at 2,820 dpi in RGB8. The client logs and parameter files report regular EOF, successful `sane_open`/`sane_close`/`sane_exit`, and successful client completion. See the [D5-HW3 hardware report](docs/BUILD-001-D5-HW3-HARDWARE-PASS.md) for the evidence boundary and details.

The three runs confirm frame 5 with the D5 client. Frame 6 is available in the software but has not yet been confirmed by a real scan. The supplied runs do not include RAW/TIFF files for independent pixel comparison or a claim of complete archive validation. `CaptureKind::Production` remains conservatively classified and is not unlocked by these log-level results.

## Supported hardware and platform

- Minolta DiMAGE Scan Dual II AF-2820U.
- macOS on Apple Silicon (`arm64`), for private local use. Intel Macs and Rosetta are not supported.
- The existing local SANE/avision installation is required. The application does not install or update SANE.
- Scanner discovery is initiated from the GUI. The user selects and confirms the detected device; no device address is guessed from a previous connection.

This is not a general-purpose application for arbitrary film scanners.

## Current capabilities

- Manual single-frame positions 1–6. Each scan must be started by the user; there is no automatic frame sequence or six-frame batch.
- RGB8 at 600 or 2,820 dpi. The three D5-HW3 scans used 2,820 dpi; 600 dpi remains the established reference profile.
- Repeated single-frame scans in one app session, with one new client process and a fresh scanner connection for every scan.
- A 30-second cooldown enforced by the Rust core between real scan attempts within the running app session.
- User-selected export directory, including an external SSD, and a separate collision-safe export directory per capture.
- A hardware-free simulator for automated UI, protocol, process, and artifact tests.
- The historical D1 four-frame batch simulation remains separate from the real manual single-frame workflow.

## Guided real scan

1. Connect and power on the scanner with the film holder removed. Allow it to initialize.
2. In BeBlog Digitizer, request scanner detection, select the Minolta, and explicitly confirm the selection.
3. Choose a film position, resolution, and output folder.
4. Start one real single-frame scan. The app starts the pinned ARM64 client as a separate process.
5. Wait for the client’s successful scanner-open event. Only then does the app prompt you to insert the film holder.
6. Insert the holder and confirm in the app. The client must then report actual holder detection; the button confirmation itself is not treated as detection.
7. Let the scan finish. The application waits for transfer EOF, process completion and cleanup, validates the available artifacts, then exports them.
8. Before another real scan, wait for the app’s 30-second cooldown. The next scan repeats the open and holder-confirmation sequence; continued holder insertion is not assumed.

There are no automatic retries or batch starts. A failed or interrupted scan requires a new, explicit user action.

## Files and archive

The client produces four capture artifacts:

- `.raw` – scanner pixel data.
- `.tiff` – TIFF image output.
- `.params.txt` – requested and returned scan parameters.
- `.client.log` – client diagnostics and scan lifecycle log.

The app validates the artifact set before exporting it to the chosen destination. Its internal archive and work area are separate from the user-selected export location. The real D5-HW3 evidence currently available to this report consists of client logs and parameter files only; it does not establish RAW/TIFF pixel identity for those three scans.

## Architecture

- Svelte and TypeScript provide the guided interface.
- The Rust Application Core owns scan state, serializes scanner jobs, applies the cooldown, and validates request and process outcomes.
- Tauri v2 connects the interface to the Rust core.
- The Minolta client is a fixed ARM64 app resource verified against a fixed SHA-256 trust anchor. The client uses the existing SANE/avision installation.
- A separate fake simulator keeps automated tests independent of scanner hardware.
- Scan capture and later image development are separate concerns. Integrated negative-to-positive development, automatic color correction, and automatic white balance are not implemented.

## Development

Requirements: an Apple Silicon Mac with the project’s Rust and Node/pnpm toolchains. Building or running the application does not itself start a scan; hardware operations are explicit GUI actions.

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

## Known limitations and next steps

- Frame 6 is software-enabled but not yet hardware-confirmed.
- The D5-HW3 client logs and parameter files do not include the RAW/TIFF material needed for independent pixel-level validation of those runs.
- `CaptureKind::Production` remains locked; successful client logs alone do not grant production archival classification.
- The 30-second cooldown is app-session state and does not persist across application restarts.
- No integrated negative-to-positive development, automatic color correction, or automatic white balance.
- No Intel/Rosetta support, public distribution promise, or general support for other scanners.
- The historical four-frame batch path is a simulation and is not the real six-position manual workflow.

## Project documentation

- [D5-HW3 real hardware milestone](docs/BUILD-001-D5-HW3-HARDWARE-PASS.md)
- [D5 implementation audit and D5-HW1/HW2 record](docs/BUILD-001-D5-AUDIT.md)
- [D4 guided hardware workflow audit](docs/BUILD-001-D4-AUDIT.md)
- [Research-009 correction](docs/RESEARCH-009-CORRECTION.md)
- [D3-B1 Minolta client contract](docs/BUILD-001-D3-B1-CLIENT-CONTRACT.md)
- [D1 batch simulation](docs/BUILD-001-D1.md)
