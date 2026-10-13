# Build 001-D4 – Guided Hardware Workflow

**Basis:** D3-B5c `c73c9c7fb1ce0905944e83b75247a7aa6cfb0be1`
**Branch:** `build/001-d4-guided-hardware-workflow`
**Status:** partial integration foundation; real client start deliberately blocked pending independent hash approval.

## Ausgangsaudit

- **PROVEN:** `AppCore::new()` aktiviert ausschließlich `FakeScannerAdapter`; die Tauri-Kommandos registrieren keinen Minolta-Start. Der normale UI-Pfad war ein Vierer-Batch-Simulator.
- **PROVEN:** `OsProcessRunner` ist test-only und startet ausschließlich feste Test-Harness-Modi. `SidecarResolver` löst bislang nur das harmlose D3-B4-Test-Sidecar auf.
- **PROVEN:** `OfflineArtifactImporter` prüft RAW, TIFF, Parameter und Clientlog und archiviert als `OfflineImport`; ein erfolgreicher Prozessnachweis und die Produktionsfinalisierung sind nicht über eine App-API verbunden.
- **PROVEN:** Der C-Client führt `sane_init → sane_open → Halterpolling → Frame/Mode/DPI → sane_get_parameters → sane_start → sane_read bis EOF → Dateien → sane_close → sane_exit` aus. Vor dieser Änderung begann das Halterpolling unmittelbar nach `sane_open`; es gab kein UI-Fortsetzungsbarriere.
- **PROVEN:** Frames sind auf 1–4 begrenzt, DPI auf 600/2820 und RGB8-Parameter/RAW-Größe werden aus den von SANE gelieferten Parametern geprüft. Der 2.820-dpi-Modus ist weiterhin nicht hardwarebestätigt.

## Änderungen

- Der C-Client schreibt nun ein versionsmarkiertes Ereignisprotokoll `BEBLOG/1` ausschließlich nach stdout; Diagnose bleibt auf stderr und im `.client.log`.
- Nach erfolgreichem `sane_open` gibt der Client `OPENED` aus und blockiert synchron auf stdin. Erst die exakte Zeile `CONTINUE` lässt das Halterpolling beginnen; `CANCEL` oder ein ungültiger Befehl führt zum kontrollierten Fehlerpfad. Ein Benutzerklick kann daher nicht mit einer Haltererkennung verwechselt werden.
- Typisierte Clientereignisse umfassen OPENED/OPEN_FAILED, HOLDER_DETECTED, FRAME_POSITIONED, PARAMETERS, SCAN_STARTED, DATA_CHUNK, EOF, TIFF_WRITTEN, SCANNER_CLOSING, COMPLETE und ERROR.
- `guided_protocol.rs` enthält einen strikten, hardwarefreien Parser und Zustandsreducer. Er verlangt OPENED → Benutzerbestätigung → echte Haltermeldung, prüft RGB8-Geometrie und Bytekumulierung und lehnt unerwartete Übergänge fail-closed ab.
- Die Svelte-Oberfläche zeigt einen geführten Einzelbildablauf, Frames 1–4, 600/2820 dpi, RGB8, Status-/Diagnosebereich und native Ordnerauswahl via Tauri Dialog Plugin. Einsetzaufforderung und Haltererkennung sind getrennt.
- **BLOCKED (absichtlich):** Es ist kein produktiver OS-Prozessrunner an Tauri angeschlossen und die Oberfläche emittiert/empfängt noch keine echten Scannerprozessereignisse. Kein UI-/Tauri-Befehl startet den Client. Start bleibt bis unabhängiger Freigabe des neu gebauten Clienthashs gesperrt. Die gebaute Fassung und UI zeigen den Sperrstatus sichtbar; FakeScannerAdapter bleibt Backendstandard.

## ARM64-Clientnachweis

Die angepasste Arbeitskopie wurde mit Apple Clang/SDK und ausschließlich lesend verwendeten vorhandenen SANE-Headern und -Libraries für `arm64` kompiliert und ad-hoc signiert. Das Binary wurde nicht ausgeführt.

- Originalsnapshot `minolta-batch-frame.original.c`: SHA-256 weiterhin `35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae`.
- Neue Arbeitskopie enthält gegenüber D3-B5c den synchronen OPENED/CONTINUE-Handshake und typisierte Ereignisausgaben.
- Nach Signierung erzeugter ARM64-Binary-SHA-256: `6006009297dfb9eb492fb2e5cc34a43cb0a502f89c07b435dc0869d887b27721`.
- `file`, `lipo -archs`, `codesign --verify --strict` und `otool -L` bestätigten thin ARM64, gültige lokale Ad-hoc-Signatur und die bestehenden SANE-/libSystem-Referenzen.
- **Nicht freigegeben:** Dieser Hash ist nur Buildidentität. Er wurde weder in `tools/minolta-hardware-test.sha256` geschrieben noch als vertrauenswürdig festgelegt.
- Die bereits vorhandene ungetrackte `tools/minolta-hardware-test.sha256` wurde nicht geändert, gestaged oder committed.

## Hardwarefreie Regressionen

`guided_protocol`-Tests verwenden synthetische, explizit als Simulation behandelte Ereignisfolgen. Geprüft werden:

- Vor OPENED kann keine Halteraufforderung bestätigt werden.
- OPENED führt nur in den Bestätigungszustand; die Bestätigung selbst setzt `holderDetected` nicht.
- Erst HOLDER_DETECTED nach CONTINUE setzt den Halterstatus.
- Simulierter Einzelbildablauf einschließlich Parameter, kumulierter Chunks, EOF, TIFF und Scannerabschluss.
- Ungültige Reihenfolge, inkonsistente Bytezahlen und Fehlerereignisse werden abgewiesen.
- Frontendtests prüfen sichtbare Start-Sperre, fehlende vorzeitige Halteraufforderung und Ordnerauswahl.

## Prüfstatus

Die vollständige Prüfmatrix folgt nach finaler Integration. In diesem Zwischenstand wurden ausgeführt:

- Rust-Tests: 102 bestanden, 0 fehlgeschlagen, 12 bestehende ignorierte Child-Modi.
- Rust-Formatierung: PASS.
- Clippy `-D warnings`: PASS.
- TypeScript: PASS.
- Frontend: 12 Tests in 4 Dateien PASS.
- Vite-Build: PASS.
- ARM64 C-Clientbuild, Ad-hoc-Signatur und statische Inspektion: PASS; Binary nicht gestartet.
- Nativer Tauri-ARM64-App-Build: PASS; App-Binary ist `arm64`. Das Bundle enthält weiterhin nur das D3-B4-Test-Sidecar, keinen Minolta-Client.
- `git diff --check`: PASS.

## Offene D4-Abnahmepunkte

- **BLOCKER:** Hashfreigabe des neuen Client-Binaries steht aus; keine reale Prozessausführung möglich.
- **BLOCKER:** Der kontrollierte Rust OS-Runner ist noch nicht mit Client-stdin-Handshake, getrennten stdout/stderr-Ereignissen, Gesamt-Timeout, Cancellation und Reaping in den Core integriert.
- **BLOCKER:** Der Tauri-Eventlistener für `hardware-client-event` hat derzeit noch keinen Rust-Prozessproduzenten; sichtbare Scannerzustände werden daher noch nicht aus realen Prozessereignissen gespeist.
- **OPEN:** Native Geräteauswahl/Bestätigung und der reale Clientressourcenpfad sind noch nicht an Tauri angebunden.
- **OPEN:** Ausgabeordner kann nativ ausgewählt werden, aber die geprüften Originalartefakte werden noch nicht in diesen Zielordner exportiert; der Offline-Importer archiviert intern als `OfflineImport`.
- **OPEN:** Die 2.820-dpi-Geometrie und Artefakte bleiben bis zu einer später separat freigegebenen Hardwareabnahme unbestätigt.

Keine Scannerhardware, kein Scannerprozess, keine SANE-Kommandos und keine Geräteerkennung wurden ausgeführt. `/opt/homebrew`, SANE/avision, Research-009 und die Hash-Pindatei blieben unverändert.
