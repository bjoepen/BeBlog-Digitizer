# Build 001-D7-C – Filmstrip UI

## Ausgangszustand

D7-B zeigte Scanner, Parameter, Speicherort sowie Einzelbild- und Mehrbildworkflow in einer dichten, dauerhaft geteilten Steuerkarte. Die Zustandsdaten kamen bereits aus `hardware-client-event` und `d6-batch-event`; diese Ereignisbindungen bleiben erhalten. Scannererkennung bleibt manuell, der Scannername ist normal sichtbar und die technische Kennung bleibt in den Diagnosedetails.

## Informationsarchitektur

Die Anwendung startet mit dem Modus **Filmstreifen**, vier Positionen, 2.820 dpi und RGB8. Scanner und Speicherort stehen oberhalb des gut sichtbaren Umschalters **Filmstreifen | Einzelbild**. Im Filmstreifenmodus sind ausschließlich Bildanzahl (2–6), Auflösung, RGB8, Positionsreihe und der Start der Folge zu sehen. Eine einzelne Filmposition wird dort nicht angeboten. Im Einzelbildmodus erscheinen stattdessen die Positionen 1–6, Auflösung, RGB8 sowie Einzelbildscan und die nachrangige hardwarefreie Simulation.

## Ereignis- und Zustandsprojektion

- Scannerstatus, Halterbestätigung und Einzelbild-Bytefortschritt bleiben an die bestehenden Guided-Client-Ereignisse und `projectHardwareEvent` gebunden.
- Die Einlegeaufforderung bleibt ausschließlich an `awaitingHolderConfirmation` gebunden. Bei Einzelbildern entsteht dieser Zustand erst aus `OPENED`; im D6-Batch kommt die explizite `awaitingHolderConfirmation`-Angabe aus dem Rust-Ereignis.
- Die Filmstreifenpositionen werden aus dem ausgewählten Umfang und dem vorhandenen D6-Ereignis projiziert. Nur Positionen `<= completedFrames` erscheinen als fertig. `currentFrame` und die Batchphase bestimmen den aktuellen Arbeitsstatus; Fehler- und Abbruchzustände werden nur für den betroffenen Frame dargestellt.
- Das bestehende Batchereignis hat keine separate `Finalizing`-Phase. Die UI fügt keine künstliche Prozessphase hinzu; technische Meldungen bleiben im Inspector-Protokoll.
- Cooldown wird als aktueller Status gezeigt. Die vorhandene Deduplizierung des Batchprotokolls bleibt erhalten, sodass Sekundenupdates keine Ereignisflut erzeugen.
- Abschlussmeldungen für den Filmstreifen verwenden `completedFrames` und werden nicht zusätzlich aus Einzel-Capture-Exportmeldungen dupliziert.

## Darstellung und Responsive Verhalten

Die Filmfolge wird als responsive Bildreihe mit beschrifteten Zuständen dargestellt. Bei schmaleren Ansichten bricht die Reihe um; der Inspector bleibt mit eigener vertikaler Ereignis-Scrollfläche erhalten. Keine Backend-, Scanner-, Batch-, Archiv- oder Exportlogik wurde geändert.

## Prüfungen

- Frontend: `pnpm check` PASS; Vitest: 21 Tests PASS in 4 Dateien; `pnpm build` PASS.
- Rust: `cargo fmt --check` PASS; vollständiges `cargo test -- --test-threads=1`: 133 PASS, 0 FAIL, 12 bewusst ignorierte Childprozess-Harnessfälle; darin alle 3 `guided_process_e2e` PASS.
- D6 gezielt: 10 Rusttests PASS; zusätzlicher D6-Prozess-E2E-Test PASS.
- `cargo clippy --all-targets -- -D warnings` PASS; `git diff --check` PASS.
- Nativer ARM64-Tauri-App-Build (`pnpm tauri build --bundles app`) PASS. Release-App: `src-tauri/target/release/bundle/macos/BeBlog Digitizer.app`.
- Die native App wurde in breiter Ansicht (zwei Spalten), mittlerer Ansicht (gestapelt) und schmaler Ansicht (gestapelt) visuell geöffnet. Die Bildpositionen brechen responsiv um; in den geprüften Ansichten war keine horizontale Seiten-Scrollleiste oder Überlappung erkennbar. Screenshots der Ansichten wurden lokal unter `/tmp/beblog-d7-c-shots/` abgelegt.
- Hardwarefreier Simulatorlauf in der nativen App PASS: expliziter Klick auf die Einzelbildsimulation, `OPENED`, manuelle Bestätigung, `HOLDER_DETECTED`, 28.800/28.800 Bytes, Prozess-Exitcode 0 und validierter Export in ein temporäres Verzeichnis. Es wurde keine Scannererkennung ausgelöst.

## Einschränkungen

Der UI-Fortschritt ist eine Projektion der vorhandenen Rust-/Clientereignisse, kein zusätzlicher Nachweis. Der reale Hardwarepfad wurde im Rahmen dieses Builds nicht ausgelöst. Der D6-Batch hat aktuell keine explizite Finalisierungsphase als eigenes UI-Ereignis. Die native Simulation prüfte Einzelbildablauf und Export; ein Batch wurde in der GUI nicht simuliert, sondern über die automatisierten Rust-/Prozessregressionen abgedeckt.
