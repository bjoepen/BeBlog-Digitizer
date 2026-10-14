# BeBlog Digitizer – D6-HW1: Real Four-Frame Batch Hardware Pass

**Test date:** 14 October 2026
**Result:** PASS for the real, GUI-guided four-frame scan sequence described below.

## Hardware and profile

- Scanner: Minolta DiMAGE Scan Dual II AF-2820U.
- Application: native macOS ARM64 BeBlog Digitizer using the D6-B3 batch orchestrator.
- SANE device identifier observed during the test: `avision:libusb:002:006`.
- Minolta client SHA-256: `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404`.
- Frames: positions 1, 2, 3 and 4, in order.
- Requested and reported profile: 2,820 dpi, RGB8, 2,664 × 3,996 pixels.
- Reported RAW transfer per frame: 31,936,032 bytes; four-frame total: 127,744,128 bytes.

The dimensions, byte counts, process lifecycle and event statements below are based on the four client logs and parameter files reviewed for this acceptance. Times are rounded from those logs.

## Observed workflow

The scanner was initialized without the film holder inserted. The first client process opened the scanner and the application waited for `OPENED`. The holder was then inserted and confirmed manually once. Frame 1 completed. The holder remained inserted for frames 2–4; those frames proceeded without another manual confirmation, while each fresh client process performed its own open and holder-detection sequence.

Each frame used a separate client process and SANE connection. The four logs report regular EOF, TIFF creation, successful `sane_close`, successful `sane_exit`, and client exit code 0. The intervals between a process completion and the next process start were consistent with the required 30-second cooldown.

| Frame | Process start (approx.) | Client completion (approx.) |
| --- | --- | --- |
| 1 | 19:14:28 | 19:16:02 |
| 2 | 19:16:34 | 19:17:47 |
| 3 | 19:18:18 | 19:19:31 |
| 4 | 19:20:03 | 19:21:15 |

Elapsed time from the first process start to the final client completion was approximately **6 minutes 47 seconds**. The rounded gaps between completion and the next start are about 32, 31 and 32 seconds.

## Evidence boundary

The four client logs and parameter files were reviewed. They support the reported client/SANE lifecycle, frame order, scan parameters, EOF, TIFF creation, and client exit results. The actual RAW/TIFF files were not independently byte-compared in this acceptance. The full D6 batch journal and export manifests were also not independently audited.

Accordingly, this PASS confirms the successful real four-frame scan sequence. It does not claim independent pixel identity verification, a complete independent archive audit, or `CaptureKind::Production` classification. That classification remains locked.

Frame 6 is available in software for manual single-frame selection, but this four-frame test does not confirm frame 6 in hardware. The historical D1 batch remains a simulation and is distinct from this D6 hardware result.

## UI follow-up (non-blocking)

Record these interface refinements for a separate RC task; they do not invalidate the successful scan sequence:

- Widen the right inspector so event identifiers do not overlap diagnostic text.
- Eliminate horizontal scrolling and improve the responsive layout for technical details.
- Reduce repeated near-identical countdown event rows while retaining the live countdown.
- Preserve live byte count, scan progress and scanner status.
- The lower “Scanarchiv” and “Bildentwicklung” placeholder sections may be omitted from the RC layout.
- Keep negative development external, for example in NegPy or GIMP; do not add image development to BeBlog Digitizer.

## Related records

- [D6-B3 implementation and hardware gates](BUILD-001-D6-B3.md)
- [D5-HW3 real single-frame hardware milestone](BUILD-001-D5-HW3-HARDWARE-PASS.md)
