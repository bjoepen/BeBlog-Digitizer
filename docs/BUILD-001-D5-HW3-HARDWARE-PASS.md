# BeBlog Digitizer – D5-HW3: Real Hardware Milestone, 14 October 2026

## Summary

On 14 October 2026, BeBlog Digitizer completed three real, manually initiated single-frame runs with the Minolta DiMAGE Scan Dual II AF-2820U. This milestone follows the successful D5-HW1 GUI scanner-discovery test and uses the D5 ARM64 client pinned in the application.

## Hardware and application

- Scanner: Minolta DiMAGE Scan Dual II AF-2820U.
- Application: native macOS ARM64 build using Tauri v2.
- D5-HW1 scanner discovery: successful through the application’s SANE-backed GUI action. The scanner was selected and explicitly confirmed; no scan was started during that discovery test.
- Device identifier reported for D5-HW1: `avision:libusb:002:006`.
- D5 client SHA-256: `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404`.

## Three manual single-frame runs

The runs were performed in this order:

| Run | Film position | Requested/returned profile |
| --- | ---: | --- |
| 1 | 3 | 2,820 dpi, RGB8, 2,664 × 3,996 pixels |
| 2 | 5 | 2,820 dpi, RGB8, 2,664 × 3,996 pixels |
| 3 | 4 | 2,820 dpi, RGB8, 2,664 × 3,996 pixels |

For each run, the corresponding client log and parameter file report:

- `sane_open` succeeded.
- The scan returned RGB8 parameters at 2,820 dpi, 2,664 pixels per line by 3,996 lines.
- Expected/observed RAW transfer size: 31,936,032 bytes.
- Data transfer ended with regular EOF.
- `sane_close` and `sane_exit` were reported successful.
- Client completion was reported successful.

Each run used its own client process and a new scanner connection. The recorded intervals between runs are consistent with the intended 30-second cooldown. The film-holder handshake was repeated for each run rather than assuming the holder remained inserted.

Frame 5 is the first frame-5 hardware confirmation using the D5 client. Frame 6 remains software-enabled but is not hardware-confirmed by these runs.

## Evidence boundary

The three client logs and parameter files support the client/SANE-level statements above. They are not substitutes for the operating system’s process record and do not independently prove pixel correctness. The corresponding RAW and TIFF files were not available with this evidence set; therefore this report does **not** claim:

- RAW/TIFF pixel identity or full image-content validation for these three scans;
- complete independent archive validation of these captures;
- `CaptureKind::Production` archival classification;
- hardware support for frame 6.

The client logs report successful `sane_open`, `sane_close`, `sane_exit`, EOF, and client completion. Those log reports are distinguished from independent verification of all exported image artifacts.

## Workflow confirmed

The application supports repeated manual single-frame scans. Each run uses a separate client process and scanner lifecycle. The application continues to require successful scanner open before the holder-insertion prompt, an explicit user confirmation, and a separate actual holder-detection event. No automatic retries or frame sequence are used. A 30-second cooldown is enforced between real scan attempts during the app session.

The earlier D1 four-frame batch simulation remains a separate, hardware-free workflow; it is not the D5 real manual six-position workflow.

## Related records

- [D5 implementation audit and D5-HW1/HW2 record](BUILD-001-D5-AUDIT.md)
- [D4 guided workflow audit](BUILD-001-D4-AUDIT.md)
- [Research-009 correction](RESEARCH-009-CORRECTION.md)
