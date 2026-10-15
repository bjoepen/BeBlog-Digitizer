# Build 001-D7-B – UI-Ergonomie, Typografie und Fensterlayout

## Ausgangszustand

Der Stand basiert auf `main` bei `c89962272e54e70452a0d27aeb94603e364d1e7b`. D7-A hatte bereits ein responsives Zwei-Spaltenlayout, Inspector, Live-Bytefortschritt und einen gebündelten Diagnosebereich. Die Bedienfelder waren jedoch überwiegend als fortlaufende Controls angeordnet; Labels und Hilfetexte lagen meist bei 11–12 px. Der frühere Hauptbereich war auf 1480 px begrenzt.

Die vorhandene Svelte-Projektion und alle Tauri-Commands und -Events wurden wiederverwendet. Es wurden keine Rust-, Scanner-, Batch-, Archiv-, Export- oder Prozessfunktionen geändert.

## Änderungen

- Die maximale Inhaltsbreite wurde auf 1720 px erweitert und die Desktopspalten so abgestimmt, dass der Inspector mehr nutzbare Breite erhält.
- Die linke Steuerung ist jetzt in vier sichtbare, nummerierte Aufgabenbereiche gegliedert: Scanner auswählen, Scanparameter, Ausgabeordner und Workflow wählen.
- Einzelbildscan und D6-Batch werden als gleichrangige Workflow-Karten dargestellt. Ihre bisherigen Controls, Freigabebedingungen und Eventbindungen bleiben erhalten.
- Allgemeine Beschriftungen und Diagnoseereignisse wurden auf besser lesbare Größen angehoben; Statuswerte und Überschriften haben eine klarere Hierarchie. Der helle, zurückhaltende Stil bleibt bestehen.
- Zwischen 1080 px und schmalen Ansichten liegen Steuerung und Inspector untereinander. Unter 680 px werden Parameter und Workflows einspaltig; Ordner- und Aktionscontrols füllen die verfügbare Breite. Ereigniskennungen und Diagnosewerte dürfen weiterhin umbrechen.
- Der Inspector zeigt unverändert Prozess-/Scannerstatus, Frame, Profil, echte RAW-Bytes, Fortschritt, Cooldown, Fehler und Ereignisse. Es wurden keine Ereigniszeilen pro Datenchunk oder Countdownsekunde ergänzt.
- Die geführte Reihenfolge einschließlich OPENED vor Einlegeaufforderung, manueller erster Halterbestätigung und automatischen D6-Folgeframes bleibt unverändert.

## Native Responsive-Prüfung

Die Release-App wurde unter macOS ARM64 gebaut und nativ gestartet. Die Oberfläche wurde bei breiter, mittlerer und schmaler Fensterbreite betrachtet:

- Breit: Scansteuerung und Inspector sind nebeneinander angeordnet; die freie Fläche wird durch den verbreiterten Hauptbereich besser genutzt.
- Mittel: beide Karten bleiben vollständig sichtbar, Texte und Controls umbrechen ohne Überlappung.
- Schmal: Steuerung und Inspector wechseln in eine Spalte; Scannerbuttons, Profilfelder und Workflowkarten bleiben innerhalb der Fensterbreite. Die Oberfläche scrollt vertikal, nicht horizontal.

Der GUI-Test blieb lesend: Es wurde weder Scannererkennung ausgelöst noch ein Gerät geöffnet oder ein Scan gestartet. Es wurde kein Screenshot als Repository-Artefakt gespeichert.

## Tests

- `pnpm check`: PASS.
- `pnpm test`: PASS, 4 Dateien / 21 Tests. Neu ist ein Strukturtest für die Aufgabenbereiche und gleichrangigen Einzelbild-/D6-Workflows; bestehende Freigabe-, Event-, Bytes-, Countdown- und D6-Tests bleiben grün.
- `pnpm build`: PASS.
- D6-Rustregressionen (`cargo test --manifest-path src-tauri/Cargo.toml --lib d6 -- --test-threads=1`): PASS, 10 Tests.
- Prozess-E2E (`cargo test --manifest-path src-tauri/Cargo.toml --test guided_process_e2e -- --test-threads=1`): PASS, 3 Tests.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: PASS.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: PASS.
- `pnpm tauri build --bundles app`: PASS; App unter `src-tauri/target/release/bundle/macos/BeBlog Digitizer.app` erstellt.
- App-Executable laut `file`: Mach-O 64-bit arm64, PASS.
- `git diff --check`: PASS.

## Einschränkungen

Die native Fensteransichten wurden manuell visuell kontrolliert; es gibt keine pixelgenaue Screenshotbaseline. Frontendtests bleiben DOM-/Eventtests und messen keine tatsächlichen nativen Pixelmaße. Der getestete schmale Fensterbereich ist durch die native Mindestbreite begrenzt. Die gebaute App liegt unter `src-tauri/target/release/bundle/macos/BeBlog Digitizer.app`. Keine Hardwarefunktion wurde im Rahmen dieses Builds getestet.
