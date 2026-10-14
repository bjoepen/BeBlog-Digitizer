# Build 001-D4 – Guided Hardware Workflow

**Basis:** D3-B5c `c73c9c7fb1ce0905944e83b75247a7aa6cfb0be1`
**Branch:** `build/001-d4-guided-hardware-workflow`
**Status:** guided process/UI/artifact integration implemented and hardware-free tested. D4-HW2 explicitly approved the exact Minolta client SHA-256 for the real GUI start path. This is software authorization only; no real scanner test has been run. `CaptureKind::Production` remains locked.

## D4a – Korrektur nach nativem GUI-Test

- **Root cause:** Nach Empfang von `OPENED` nahm der Prozessrunner `CONTINUE` entgegen und schrieb es an stdin, rief aber nicht `GuidedProtocol::confirm_insertion()` auf. Der Core blieb dadurch in `AwaitingHolderConfirmation`; das darauffolgende gültige `HOLDER_DETECTED` wurde als unzulässiger Übergang abgelehnt. Der Fehlerpfad setzte Cancellation und rief anschließend die Terminierung auf.
- **EPERM:** Die Terminierung sendete Signale an `kill(-pid, ...)` und unterstellte damit, der Kind-PID sei verlässlich die gewünschte Prozessgruppen-ID. Unter dem beobachteten App-Lauf schlug diese Annahme mit `EPERM` fehl. D4 verarbeitet einen einzelnen Clientprozess ohne erforderliche Kindprozesse; die Korrektur entfernt die nicht belegte Prozessgruppenannahme und signalisiert den von `Child` gehaltenen PID direkt. Nicht-`ESRCH`-Fehler wie `EPERM` bleiben harte Fehler. `ESRCH` wird nur nach zusätzlicher Exitbeobachtung und erfolgreichem `wait()` als bereits beendeter Prozess behandelt; Reapingfehler werden weitergereicht.
- Die Bestätigung wird nun im Worker nur bei einem tatsächlichen `Continue`-Steuerereignis durch `GuidedProtocol::confirm_insertion()` validiert. Erst nach erfolgreichem Schreiben des Handshake-Tokens meldet der Core `holderConfirmationAccepted`; die Svelte-Projektion wechselt nicht mehr optimistisch und protokolliert keine Bestätigung automatisch nach `OPENED`. Wiederholte Bestätigungen werden abgewehrt.
- **Zusätzlicher nativer UI-Befund:** Die Ereignis-Enum-Varianten waren camelCase, ihre Rust-Felder aber nicht. Dadurch kamen z. B. `expected_bytes`/`total_bytes`/`exit_code` statt der Svelte-Vertragsnamen an; die Oberfläche zeigte keinen Transferstand und `OS-Exitcode undefined`, obwohl der Archivexport gelang. `rename_all_fields = "camelCase"` und ein Serialisierungstest schließen diese Schnittstellenlücke.
- Regressionen prüfen, dass vor `OPENED` kein Token geschrieben wird, ein bestätigter Übergang denselben Protokollzustand fortschaltet, ein zweites Token abgewiesen wird, SIGTERM den direkten harmlosen Kindprozess beendet und reap-t, die Rust-Eventfelder dem camelCase-Frontendvertrag entsprechen sowie dass der Simulator am `OPENED`-Barrierpunkt ohne explizites `CONTINUE` weiterläuft.

## Ausgangsaudit

- **PROVEN:** `AppCore::new()` aktiviert ausschließlich `FakeScannerAdapter`; die Tauri-Kommandos registrieren keinen Minolta-Start. Der normale UI-Pfad war ein Vierer-Batch-Simulator.
- **PROVEN:** Der D2-`OsProcessRunner` bleibt test-only. D4 ergänzt einen separaten, fest verdrahteten Prozesscontroller für den Minolta-Ressourcenpfad; D4-HW2 pinnt den ausdrücklich freigegebenen Kandidatenhash im Quellcode und öffnet die GUI-Startsteuerung.
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
- **Freigabestatus nach D4-HW2:** `resolve_approved_minolta_client()` akzeptiert ausschließlich SHA-256 `9049bb63b6a6a81ef1d686172f063d3949939e3acca7786fc3a414cb21d36aa6`; die UI öffnet den Einzelbildstart nur mit Ausgabeordner und manuell bestätigter Gerätekennung. Das ist keine Hardwareabnahme. `CaptureKind::Production` bleibt gesperrt; reale SANE-Cleanup-Garantien werden nicht aus Logmarkern abgeleitet.

## ARM64-Clientnachweis

Die angepasste Arbeitskopie wurde mit Apple Clang/SDK und ausschließlich lesend verwendeten vorhandenen SANE-Headern und -Libraries für `arm64` kompiliert und ad-hoc signiert. Das Binary wurde nicht ausgeführt.

- Originalsnapshot `minolta-batch-frame.original.c`: SHA-256 weiterhin `35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae`.
- Neue Arbeitskopie enthält gegenüber D3-B5c den synchronen OPENED/CONTINUE-Handshake und typisierte Ereignisausgaben.
- Im finalen D4-Arbeitsstand eingebundener thin-ARM64-Binary-SHA-256 nach ad-hoc-Signierung: `9049bb63b6a6a81ef1d686172f063d3949939e3acca7786fc3a414cb21d36aa6`. Der feste Codesign-Identifier lautet `minolta-batch-frame`; der Linker erzeugt weiterhin pro Build eine Mach-O-UUID, daher ist der Hash als konkrete Kandidatenidentität zu verstehen, nicht als allgemein reproduzierbarer Source-Hash.
- `file`, `lipo -archs`, `codesign --verify --strict` und `otool -L` bestätigten thin ARM64, gültige lokale Ad-hoc-Signatur und die bestehenden SANE-/libSystem-Referenzen.
- **Softwarefreigabe D4-HW2:** Dieser exakte Hash ist der einzige vom Tauri-Resolver akzeptierte Minolta-Client. Er wurde nicht in `tools/minolta-hardware-test.sha256` geschrieben; dieser manuelle Launcher-Pin bleibt unabhängig und unangetastet.
- Die bereits vorhandene ungetrackte `tools/minolta-hardware-test.sha256` wurde nicht geändert, gestaged oder committed.

## Hardwarefreie Regressionen

`guided_protocol`-Tests verwenden synthetische, explizit als Simulation behandelte Ereignisfolgen. Geprüft werden:

- Vor OPENED kann keine Halteraufforderung bestätigt werden.
- OPENED führt nur in den Bestätigungszustand; die Bestätigung selbst setzt `holderDetected` nicht.
- Erst HOLDER_DETECTED nach CONTINUE setzt den Halterstatus.
- Simulierter Einzelbildablauf einschließlich Parameter, kumulierter Chunks, EOF, TIFF und Scannerabschluss.
- Ungültige Reihenfolge, inkonsistente Bytezahlen und Fehlerereignisse werden abgewiesen.
- Frontendtests prüfen den hashgebundenen Freigabestatus, erforderliche Geräte-/Zielbestätigung, fehlende vorzeitige Halteraufforderung und UI-Eventprojektion.
- Resolvertests akzeptieren das exakt gepinnte Minolta-Binary und weisen eine nachträglich veränderte Kopie ab; Prozessprotokolltests weisen `CONTINUE` vor `OPENED` oder ohne tatsächliche Bestätigungsaktion ab.
- `tests/guided_process_e2e.rs` startet das ARM64-App-Executable als echten Kindprozess im ausschließlich synthetischen Simulatormodus, liest OPENED, sendet CONTINUE, empfängt die Ereignisfolge bis COMPLETE, importiert/validiert die vier Dateien, archiviert als OfflineImport und exportiert alle Dateien in ein neues Zielverzeichnis.

## Prüfstatus

Aktuelle hardwarefreie Prüfmatrix:

- Rust-Tests nach D4a: 109 Unit-Tests bestanden, 0 fehlgeschlagen, 12 bestehende ignorierte Child-Modi; zusätzlich 1 echter Kindprozess-End-to-End-Test bestanden.
- Rust-Formatierung: PASS.
- Clippy `-D warnings`: PASS.
- TypeScript: PASS.
- Frontend: 15 Tests in 4 Dateien PASS.
- Vite-Build: PASS.
- ARM64 C-Clientbuild, Ad-hoc-Signatur und statische Inspektion: PASS; Binary nicht gestartet.
- Nativer Tauri-ARM64-App-Build: PASS; App-Binary ist `arm64`. Das `.app` enthält das D3-B4-Test-Sidecar und den ARM64-Minolta-Kandidaten unter `Contents/Resources/sidecars/macos-aarch64/`; der Kandidat wurde nicht gestartet.
- `git diff --check`: PASS.
- Native visuelle App-Prüfung nach D4a: **PASS für den Simulatorpfad**. In der gebauten ARM64-`.app` wurde `/Volumes/Kioxia/BeBlog Digitizer Test` ausgewählt, der Simulator gestartet, `OPENED` abgewartet und die Bestätigung manuell betätigt. Sichtbar waren `Im Simulator erkannt`, `28.800 / 28.800 Byte`, OS-Exitcode 0, `Scan abgeschlossen` und der erfolgreiche Export nach `/Volumes/Kioxia/BeBlog Digitizer Test/BeBlog-Scan-2d0e30efaa154388bd0cfe10e3c3d8bf`. Ein Screenshot wurde nicht ins Repository übernommen.

## Offene Punkte / Freigabegates

- **Software-Hashgate:** Für genau dieses Binary durch D4-HW2 freigegeben; abweichende Binary-Hashes werden abgewiesen. Es gibt keine automatische Hashübernahme.
- **OPEN:** Geräteauswahl ist manuelle Gerätekennung plus exakte App-Bestätigung; automatische SANE-Geräteentdeckung wurde bewusst nicht hinzugefügt.
- **OPEN:** erfolgreiche reale Scans werden aktuell konservativ als `OfflineImport` archiviert; `Production` bleibt gesperrt, bis OS-Prozessnachweis, reale Artefakte und SANE-Cleanup-Vertrag gemeinsam geprüft sind.
- **OPEN:** 2.820-dpi-Geometrie/Artefakte sind weiterhin nicht hardwarebestätigt.

Kein Minolta-Client, keine Scannerhardware, keine SANE-Kommandos und keine Geräteerkennung wurden ausgeführt. Nur der harmlose App-Simulator-Kindprozess lief im automatisierten Test. `/opt/homebrew` wurde ausschließlich lesend für Header/Library und ARM64-Linkreferenzen genutzt; SANE/avision, Research-009 und die ungetrackte Hash-Pindatei blieben unverändert.

## D4-HW2 – Softwarefreigabe (hardwarefrei)

- Fester Resolver-Trust-Anchor: `9049bb63b6a6a81ef1d686172f063d3949939e3acca7786fc3a414cb21d36aa6`. Der Resolver behält den festen App-Ressourcenpfad, ARM64-/Signaturprüfung und Hashvergleich bei; keine Laufzeit-Hashübernahme.
- Die GUI unterscheidet nun ausdrücklich „Realen Einzelbildscan starten“ von „Ablauf hardwarefrei simulieren“. Realstart verlangt weiterhin Gerät + exakte Bestätigung, Ausgabeordner und Idle-Zustand. Die Filmhalteraufforderung erscheint erst nach `OPENED`; `CONTINUE` erfordert den manuellen Bestätigungsklick.
- Hardwarefreie Regressionen: freigegebenes Testbinary akzeptiert, veränderte Kopie abgewiesen; vorhandene Tests sichern weiterhin den manuellen `CONTINUE`-Handshake. `CaptureKind::Production` bleibt gesperrt.
- D4-HW2-Prüfmatrix: Rust 109 bestanden / 0 fehlgeschlagen / 12 bewusst ignorierte Child-Modi; Prozess-E2E 1/1; Frontend 15/15; TypeScript, Rust-Format, Clippy `-D warnings`, Vite-Build, nativer ARM64-Tauri-App-Build und `git diff --check` PASS.
- Im neu erzeugten `.app` wurde das enthaltene Clientbinary erneut geprüft: SHA-256 stimmt mit dem Trust-Anchor überein, thin `arm64`, Codesign `adhoc`, `codesign --verify --strict` PASS. Das Binary wurde nicht ausgeführt.
- Es erfolgten keine Scannerstarts, SANE-Geräteabfragen oder Hardwaretests. Die ungetrackte `tools/minolta-hardware-test.sha256` wurde nicht geändert oder committet.
