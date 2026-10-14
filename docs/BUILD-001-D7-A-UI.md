# Build 001-D7-A – Responsive UI and Inspector

## Ausgangszustand

Die Scansteuerung, Scannererkennung, D5-Einzelframe-Führung und D6-Batchbedienung lagen bereits in einer Svelte-Ansicht. Es gab eine starre `760px`-Mindestbreite auf `body`, zwei Spalten mit engem rechten Protokollbereich und Ereigniszeilen für jeden Datenchunk beziehungsweise wiederholte D6-Countdownmeldungen. Die unteren Bereiche „Scanarchiv“ und „Bildentwicklung“ waren lediglich Platzhalter.

Die vorhandene `GuidedProjection`, D6-Tauri-Events und Prozess-/Scannerzustände wurden wiederverwendet. Es wurde keine zweite UI-Zustandsmaschine und keine Änderung an Rust, Scannerlogik, SANE, Batch-Orchestrator, Client oder Archivierung eingeführt.

## UI-Änderungen

- Linke Spalte für Einzelbildsteuerung, Scannererkennung und D6-Konfiguration; rechts ein verbreiterter Live-Inspector.
- Inspector enthält Scanner-/Prozesszustand, Geräteauswahlstatus, Haltererkennung, Frame, gewähltes beziehungsweise zurückgelesenes Scanprofil, RAW-Bytezähler und aktuellen Framefortschritt, Cooldown sowie Ereignisse und Fehler.
- Bytezähler und Progress-Element verwenden weiterhin `GuidedProjection`-Werte aus typisierten Clientereignissen. Bei einem neuen D6-Frame werden die framebezogenen Byte-/Parameteranzeigen zurückgesetzt; es werden keine Fortschrittswerte simuliert.
- D6-Batchereignisse aktualisieren Status und Countdown direkt. Wiederholte Ereignisse derselben Batchphase und desselben Frames erzeugen keine zusätzlichen Logzeilen; Übergänge bleiben protokolliert. Datenchunks aktualisieren den Bytezähler, nicht das Diagnoseprotokoll.
- Ereigniskennungen und Diagnosetexte dürfen umbrechen; der Ereignisbereich scrollt vertikal und verhindert horizontales Überlaufen.
- Die beiden unteren Platzhalterbereiche wurden entfernt. Bildentwicklung bleibt extern, zum Beispiel in NegPy oder GIMP.

## Responsive Layout

- Ab **1021px** verfügbarer Viewportbreite: Steuerung und Inspector nebeneinander, Inspector mit einer Mindestbreite von 390px.
- Bis einschließlich **1020px**: Spalten untereinander, Inspector vollständig weiter unten erreichbar.
- Bis einschließlich **620px**: einspaltige Steuerfelder, gestapelte Ordner-/Aktionsbuttons und einspaltige Ereignisdetails.
- `html`, `body`, Grid-Kinder, Controls und Ereignistext haben keine starre Desktop-Mindestbreite; lange IDs werden umgebrochen.

## Verifikation

- `pnpm check`: PASS.
- `pnpm test`: PASS, 4 Dateien / 20 Tests. Neue Prüfungen decken echte Eventdaten im Byteinspector, D6-Countdown ohne Logzeilenflut, Batchauswahl 2–6 mit Standard 4, Inspector-Präsenz und entfernte Platzhalter ab.
- `pnpm build`: PASS.
- `pnpm tauri build --bundles app`: PASS; native ARM64-App unter `src-tauri/target/release/bundle/macos/BeBlog Digitizer.app` erstellt.
- `cargo test --lib d6 -- --test-threads=1`: PASS, 10 D6-Journal-/Core-/Validierungsregressionen.
- `cargo test --test guided_process_e2e -- --test-threads=1`: PASS, 3 Prozess-E2E-Tests einschließlich D6 2/4/6, D5-Einzelbild und Export.
- Keine Scannerhardware, Geräteerkennung oder SANE-Operation wurde ausgeführt.

Die native App wurde bei ihrer verfügbaren Desktopfenstergröße visuell geprüft; Inspector und Scansteuerung waren ohne überlappende Meldungen sichtbar. Die native Fensterverkleinerung ließ sich in dieser Laufumgebung nicht zuverlässig ausführen. Schmale Breiten sind daher durch CSS-Breakpoints und DOM-Regressionen, nicht durch einen nativen Screenshot bei verkleinertem Fenster, abgedeckt.

## Einschränkungen

Frontendtests laufen in jsdom und bestätigen DOM-/Eventverhalten; sie messen keine tatsächlichen Pixelbreiten eines nativen macOS-Fensters. Das Layout ist durch CSS-Breakpoints und Umbruchregeln responsiv gestaltet, aber ein manueller Sichttest bei mehreren Fenstergrößen bleibt für die unabhängige UI-Abnahme sinnvoll. Live-RAW-Fortschritt und Ist-Parameter erscheinen erst, wenn der Client die entsprechenden Ereignisse geliefert hat.
