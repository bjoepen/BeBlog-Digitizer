# Build 001-D4 – Guided Hardware Workflow

**Basis:** D3-B5c `c73c9c7fb1ce0905944e83b75247a7aa6cfb0be1`
**Branch:** `build/001-d4-guided-hardware-workflow`
**Status:** guided process/UI/artifact integration implemented and hardware-free tested. Real client start is deliberately blocked pending independent hash approval. The visual native-app check remains OPEN because the desktop inspection bridge did not attach to the built app.

## Ausgangsaudit

- **PROVEN:** `AppCore::new()` aktiviert ausschließlich `FakeScannerAdapter`; die Tauri-Kommandos registrieren keinen Minolta-Start. Der normale UI-Pfad war ein Vierer-Batch-Simulator.
- **PROVEN:** Der D2-`OsProcessRunner` bleibt test-only. D4 ergänzt einen separaten, fest verdrahteten Prozesscontroller für den Minolta-Ressourcenpfad; die Resolverfunktion verweigert den realen Start weiterhin, solange kein unabhängiger Hash freigegeben ist.
- **PROVEN:** `OfflineArtifactImporter` prüft RAW, TIFF, Parameter und Clientlog und archiviert als `OfflineImport`; ein erfolgreicher Prozessnachweis und die Produktionsfinalisierung sind nicht über eine App-API verbunden.
- **PROVEN:** Der C-Client führt `sane_init → sane_open → Halterpolling → Frame/Mode/DPI → sane_get_parameters → sane_start → sane_read bis EOF → Dateien → sane_close → sane_exit` aus. Vor dieser Änderung begann das Halterpolling unmittelbar nach `sane_open`; es gab kein UI-Fortsetzungsbarriere.
- **PROVEN:** Frames sind auf 1–4 begrenzt, DPI auf 600/2820 und RGB8-Parameter/RAW-Größe werden aus den von SANE gelieferten Parametern geprüft. Der 2.820-dpi-Modus ist weiterhin nicht hardwarebestätigt.

## Änderungen

- Der C-Client schreibt ein versionsmarkiertes Ereignisprotokoll `BEBLOG/1` ausschließlich nach stdout; Diagnose bleibt auf stderr und im `.client.log`.
- Nach erfolgreichem `sane_open` gibt der Client `OPENED` aus und blockiert synchron auf stdin. Erst die exakte Zeile `CONTINUE` lässt das Halterpolling beginnen; `CANCEL` oder ein ungültiger Befehl führt zum kontrollierten Fehlerpfad. Ein Benutzerklick kann daher nicht mit einer Haltererkennung verwechselt werden.
- Typisierte Clientereignisse umfassen OPENED/OPEN_FAILED, HOLDER_DETECTED, FRAME_POSITIONED, PARAMETERS, SCAN_STARTED, DATA_CHUNK, EOF, TIFF_WRITTEN, SCANNER_CLOSING, COMPLETE und ERROR.
- `guided_protocol.rs` enthält einen strikten, hardwarefreien Parser und Zustandsreducer. Er verlangt OPENED → Benutzerbestätigung → echte Haltermeldung, prüft RGB8-Geometrie und Bytekumulierung und lehnt unerwartete Übergänge fail-closed ab.
- Die Svelte-Oberfläche zeigt einen geführten Einzelbildablauf, Frames 1–4, 600/2820 dpi, RGB8, Status-/Diagnosebereich und native Ordnerauswahl via Tauri Dialog Plugin. Einsetzaufforderung und Haltererkennung sind getrennt.
- Der Tauri-Core besitzt nun `start_guided_scan`, `confirm_guided_holder` und `cancel_guided_scan`. Der Prozess wird über `std::process::Command` direkt, ohne Shell, nur aus dem festen App-Ressourcenpfad und mit intern aufgebauten Argumenten gestartet. stdout und stderr werden parallel gelesen; Clientereignisse werden strikt geparst und als `hardware-client-event` an Svelte geleitet.
- `OPENED` wird erst nach erfolgreichem `sane_open` akzeptiert. Der Core lässt die Einlegebestätigung nur nach diesem Prozessereignis zu und sendet dann exakt `CONTINUE\n`. Der UI-Reducer zeigt die Aufforderung nur nach `OPENED`; Bestätigung setzt selbst keine Haltererkennung.
- Gesamt-Timeout: 20 Minuten; Abbruch/Timeout senden erst den kooperativen `CANCEL`-Token (bei Abbruch), fordern SIGTERM an, warten höchstens 15 Sekunden und eskalieren nötigenfalls auf SIGKILL. Der Runner wartet anschließend auf `Child::wait()`, verlangt beide geschlossenen Streams und beendete Reader. Timeout, Abbruch, Forced-Kill, OS-Exit und Cleanup werden typisiert gemeldet. Keine Retries.
- Nach Prozessende muss BEBLOG/1 `COMPLETE` erreicht sein und der OS-Exitcode 0 sein. Danach prüft der vorhandene Importer alle vier Dateien; RAW/TIFF-Pixel müssen identisch sein. Validierte Dateien werden in ein neues, kollisionsfreies Unterverzeichnis des in der UI gewählten Ziels kopiert; Originaldateien werden nie überschrieben. Die UI meldet erst nach Export Erfolg.
- Die Oberfläche bietet Gerätebezeichner-Eingabe plus exakte Bestätigung vollständig in der App; sie fragt SANE-Geräte nicht ab und übernimmt keine alten USB-Adressen. Frame und DPI sind typisiert/begrenzt.
- Ein expliziter geführter Simulator startet eine zweite Instanz desselben App-Executables in einem isolierten `--beblog-guided-simulator`-Modus. Er schreibt ausschließlich synthetische Daten, spricht BEBLOG/1 und wartet auf `CONTINUE`. Der Capture wird `OfflineImport`, niemals `Production`, klassifiziert.
- **Gesperrt (beabsichtigt):** `resolve_approved_minolta_client()` liefert aktuell `PermissionDenied`, weil `APPROVED_MINOLTA_CLIENT_SHA256` weiterhin `None` ist; die UI hat `clientApproved = false`. Der neue Client ist als App-Ressource paketiert, aber nicht über die Anwendung ausführbar. `CaptureKind::Production` bleibt ebenfalls gesperrt; reale SANE-Cleanup-Garantien werden nicht aus Logmarkern abgeleitet.

## ARM64-Clientnachweis

Die angepasste Arbeitskopie wurde mit Apple Clang/SDK und ausschließlich lesend verwendeten vorhandenen SANE-Headern und -Libraries für `arm64` kompiliert und ad-hoc signiert. Das Binary wurde nicht ausgeführt.

- Originalsnapshot `minolta-batch-frame.original.c`: SHA-256 weiterhin `35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae`.
- Neue Arbeitskopie enthält gegenüber D3-B5c den synchronen OPENED/CONTINUE-Handshake und typisierte Ereignisausgaben.
- Im finalen D4-Arbeitsstand eingebundener thin-ARM64-Binary-SHA-256 nach ad-hoc-Signierung: `9049bb63b6a6a81ef1d686172f063d3949939e3acca7786fc3a414cb21d36aa6`. Der feste Codesign-Identifier lautet `minolta-batch-frame`; der Linker erzeugt weiterhin pro Build eine Mach-O-UUID, daher ist der Hash als konkrete Kandidatenidentität zu verstehen, nicht als allgemein reproduzierbarer Source-Hash.
- `file`, `lipo -archs`, `codesign --verify --strict` und `otool -L` bestätigten thin ARM64, gültige lokale Ad-hoc-Signatur und die bestehenden SANE-/libSystem-Referenzen.
- **Nicht freigegeben:** Dieser Hash ist nur Buildidentität. Er wurde weder in `tools/minolta-hardware-test.sha256` geschrieben noch als vertrauenswürdig festgelegt. Der Tauri-Core vertraut ihm nicht.
- Die bereits vorhandene ungetrackte `tools/minolta-hardware-test.sha256` wurde nicht geändert, gestaged oder committed.

## Hardwarefreie Regressionen

`guided_protocol`-Tests verwenden synthetische, explizit als Simulation behandelte Ereignisfolgen. Geprüft werden:

- Vor OPENED kann keine Halteraufforderung bestätigt werden.
- OPENED führt nur in den Bestätigungszustand; die Bestätigung selbst setzt `holderDetected` nicht.
- Erst HOLDER_DETECTED nach CONTINUE setzt den Halterstatus.
- Simulierter Einzelbildablauf einschließlich Parameter, kumulierter Chunks, EOF, TIFF und Scannerabschluss.
- Ungültige Reihenfolge, inkonsistente Bytezahlen und Fehlerereignisse werden abgewiesen.
- Frontendtests prüfen sichtbare Start-Sperre, fehlende vorzeitige Halteraufforderung, Gerätebestätigung und UI-Eventprojektion.
- `tests/guided_process_e2e.rs` startet das ARM64-App-Executable als echten Kindprozess im ausschließlich synthetischen Simulatormodus, liest OPENED, sendet CONTINUE, empfängt die Ereignisfolge bis COMPLETE, importiert/validiert die vier Dateien, archiviert als OfflineImport und exportiert alle Dateien in ein neues Zielverzeichnis.

## Prüfstatus

Aktuelle hardwarefreie Prüfmatrix:

- Rust-Tests: 105 Unit-Tests bestanden, 0 fehlgeschlagen, 12 bestehende ignorierte Child-Modi; zusätzlich 1 echter Kindprozess-End-to-End-Test bestanden.
- Rust-Formatierung: PASS.
- Clippy `-D warnings`: PASS.
- TypeScript: PASS.
- Frontend: 14 Tests in 4 Dateien PASS.
- Vite-Build: PASS.
- ARM64 C-Clientbuild, Ad-hoc-Signatur und statische Inspektion: PASS; Binary nicht gestartet.
- Nativer Tauri-ARM64-App-Build: PASS; App-Binary ist `arm64`. Das `.app` enthält das D3-B4-Test-Sidecar und den ARM64-Minolta-Kandidaten unter `Contents/Resources/sidecars/macos-aarch64/`; der Kandidat wurde nicht gestartet.
- `git diff --check`: PASS.
- Native visuelle App-Prüfung: **OPEN/FAIL für Abnahme**. Der Computer-Use-Accessibility-Aufruf hing und die App erschien nicht als laufender Prozess; es wurde kein Screenshot oder visueller PASS behauptet. Der GUI-Eventvertrag ist durch Svelte-Tests abgedeckt, der echte native Fensterlauf noch nicht.

## Offene Punkte / Freigabegates

- **BLOCKER für reale Hardwareausführung:** unabhängige Freigabe des exakten Clienthashs `9049…44c6`. Keine automatische Hashübernahme ist implementiert.
- **OPEN für visuellen Build-PASS:** nativer `.app`-Fensterstart und Klickpfad konnten über die verfügbare Computer-Use-Accessibility-Brücke nicht abgeschlossen werden. Der nächste Abnahmeschritt muss die native UI öffnen und den Simulator-Button bis zum sichtbaren Exportstatus durchspielen.
- **OPEN:** Geräteauswahl ist manuelle Gerätekennung plus exakte App-Bestätigung; automatische SANE-Geräteentdeckung wurde bewusst nicht hinzugefügt.
- **OPEN:** erfolgreiche reale Scans werden aktuell konservativ als `OfflineImport` archiviert; `Production` bleibt gesperrt, bis OS-Prozessnachweis, reale Artefakte und SANE-Cleanup-Vertrag gemeinsam geprüft sind.
- **OPEN:** 2.820-dpi-Geometrie/Artefakte sind weiterhin nicht hardwarebestätigt.

Kein Minolta-Client, keine Scannerhardware, keine SANE-Kommandos und keine Geräteerkennung wurden ausgeführt. Nur der harmlose App-Simulator-Kindprozess lief im automatisierten Test. `/opt/homebrew` wurde ausschließlich lesend für Header/Library und ARM64-Linkreferenzen genutzt; SANE/avision, Research-009 und die ungetrackte Hash-Pindatei blieben unverändert.
