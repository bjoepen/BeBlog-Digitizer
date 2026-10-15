# BeBlog Digitizer – D7-C-RC2 UI-Abnahme

Datum: 15. Oktober 2026  
Modus: hardwarefreies Read-only-Audit; dieser Bericht ist lokal und nicht committed.

## Git-Stand

- GitHub `main`, `origin/main` und lokaler `main`: `e0bdc846fa2521243acd194d79e82a2bc72be342`.
- PR #24 ist im erwarteten Merge-Commit enthalten. Der lokale Branch ist `main` und war nach dem Fast-Forward `main...origin/main`.
- Arbeitsbaum: keine getrackten Änderungen. Ausschließlich die beiden bereits vorhandenen ungetrackten Dateien plus dieser neu erstellte Bericht sind ungetrackt.
- Geschützte Dateien blieben unverändert: `tools/minolta-hardware-test.sha256` SHA-256 `1a03d2d6e7d7fd38452b982e019fae140e2f3bacb7efd63f157b7a3d7ea5a8ee`; `docs/BUILD-001-D7-RC1-ACCEPTANCE.md` SHA-256 `55369c23c7795ea9079d9fe471983af83ff15caa4798a26cf0f682f655b62447`.

## Native App und UI

Die aktuelle App wurde mit `pnpm tauri build --bundles app` gebaut und nativ gestartet:

`/Users/bernd/BeBlog-Digitizer/src-tauri/target/release/bundle/macos/BeBlog Digitizer.app`

Das enthaltene Executable meldet `arm64`. Der sichtbare Startzustand war Filmstreifen, vier Negative, 2.820 dpi, RGB8; vier Positionen waren als „Wartet“ dargestellt. Der Moduswechsel zu Einzelbild zeigte Position 1–6 und entfernte die Batchanzahl-Auswahl. Scannererkennung blieb unberührt.

Die Oberfläche wurde bei ungefähr 1.050 px, 900 px und 760 px Fensterbreite visuell geprüft. Bei den drei Größen war keine horizontale Seiten-Scrollleiste oder Textüberlagerung erkennbar. Unterhalb der schmaleren Breite wechseln die Hauptkarten von zwei Spalten auf eine vertikale Anordnung. Der Inspector ist dadurch weiter unten im Dokument und per vertikalem Scrollen erreichbar. Auf dem schmalen Fenster wirken einzelne Scanner-/Ordner-Steuerungen noch dicht nebeneinander; sie blieben sichtbar und ohne horizontales Überlaufen.

Screenshots wurden in der UI-Sitzung betrachtet; sie wurden nicht als Repositorydateien abgelegt.

## D6-Ereignisprojektion

Quellen: `src/App.svelte`, `src/App.test.ts`, `src-tauri/src/core.rs` und `src-tauri/src/guided_process.rs`. Die UI-Projektion nutzt D6-Ereignisfelder `phase`, `currentFrame`, `completedFrames`, `totalFrames`, Cooldown und Halterbestätigung. `Fertig` wird ausschließlich aus `completedFrames` abgeleitet. Bytewerte kommen aus den Clientereignissen; einzelne Datenchunks werden nicht als Logzeilen hinzugefügt. Cooldownmeldungen werden pro Batch/Phase/Frame dedupliziert.

| Zustand | Projektion und Befund |
|---|---|
| Kein Batch | Alle konfigurierten Positionen „Wartet“, Inspector „Bereit“; Standardzustand durch Frontendtest abgedeckt. |
| Batch startet | Aktueller Frame „Wird vorbereitet“; kein Bild wird vor `completedFrames` als fertig markiert. |
| Halterbestätigung | Prompt wird über `awaitingHolderConfirmation` gesteuert. Einzelframe-Test beweist, dass die Aufforderung erst nach `OPENED` erscheint und `CONTINUE` erst nach Nutzerklick aufgerufen wird. |
| Halter erkannt / Scan läuft | Aktuelle Position wechselt bei Phase `scanning` auf „Wird gescannt“; tatsächliche Haltererkennung bleibt ein separates Clientereignis. |
| Erstes Bild fertig | Mit `completedFrames=1` ist ausschließlich Position 1 fertig. |
| Bereitschaftspause | Abgeschlossenes Bild bleibt fertig, Folgebild wartet; Countdown sichtbar, Log bleibt auf eine Zustandszeile dedupliziert. |
| Nächstes Bild läuft | Bei `currentFrame=2`, `completedFrames=1` bleibt Bild 1 fertig und Bild 2 aktiv. Durch dieselbe Projektion abgedeckt. |
| Alle vier fertig | Nur `completedFrames=totalFrames` erzeugt die Abschlussmeldung; Test bestätigt genau eine Erfolgsmeldung. |
| Späterer Framefehler | Vorherige abgeschlossene Positionen bleiben fertig; aktuelle Position wird „Fehlgeschlagen“. Regressionstest vorhanden. |
| Abbruch nach gesicherten Bildern | Rust-Core-Journal/Cancel-Tests sichern abgeschlossene Captures. UI zeigt „Batch abgebrochen“ und die abgeschlossene Anzahl; falls `currentFrame` leer ist, nicht begonnene Positionen bleiben bewusst „Wartet“. Die UI-Projektion des Cancel-Ereignisses hat keinen eigenen dedizierten Frontendtest (WARN). |

Ein vollständiger D6-Batch wurde nicht über die GUI gestartet: die Oberfläche bietet keinen hardwarefreien Batchsimulator. Die D6-Projektion wurde daher mit vorhandenen Frontendtests und der realen Rust-Testabdeckung geprüft; keine Ereignisse wurden an einen realen Scannerpfad gesendet.

## Hardwarefreier Einzelbildsimulator

Der Simulator wurde ausschließlich über die gestartete native App ausgeführt, in einen neu angelegten temporären Ausgabeordner:

`/private/tmp/beblog-d7-rc2-sim.DwStmv`

Beobachtet: eigener harmloser `BEBLOG/1`-Kindprozess; `OPENED`; danach sichtbare Aufforderung; ausdrücklicher manueller Halterbestätigungsklick; getrennte `HOLDER_DETECTED`-Meldung; Parameter 2.820 dpi, RGB8, synthetische Geometrie 120 × 80; tatsächlicher Fortschritt 28.800/28.800 Bytes; EOF, TIFF, Prozess-Exitcode 0 und Cleanup; erfolgreicher validierter Export in einen neuen Capture-Unterordner. Die App kennzeichnete die Simulation ausdrücklich als ohne Scannerverbindung. Keine Scannererkennung, SANE-Abfrage, Minolta-Clientausführung oder Hardwareaktion fand statt.

## Prüfungen

| Prüfung | Ergebnis |
|---|---|
| `pnpm check` | PASS |
| `pnpm test -- --run` | PASS — 4 Dateien, 21 Tests |
| `pnpm build` | PASS |
| `cargo fmt --check` | PASS |
| `cargo test d6 -- --test-threads=1` | PASS — 10 D6-Tests; zusätzlich 1 gefilterter D6-Prozess-E2E-Test |
| `cargo test -- --test-threads=1` | PASS — 133 bestanden, 0 fehlgeschlagen, 12 ignoriert; zusätzlich 3 Prozess-E2E-Tests bestanden |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `pnpm tauri build --bundles app` | PASS — native macOS-App erzeugt |
| App-Architektur | PASS — `arm64` |
| `git diff --check` | PASS |

## RC-Bewertung

- UI-Ergonomie und Filmstreifen-Standardzustand: **PASS**.
- Responsives Layout in den geprüften Größen: **PASS**, mit **WARN** zur dichten Anordnung einzelner Steuerungen bei schmaler Breite.
- Inspector, echte Simulator-Bytewerte und Einzelbild-Simulator: **PASS**.
- D6-Ereignisprojektion für Start, Halterbestätigung, Scan, Fortschritt, Cooldown, Erfolg und Fehler: **PASS** auf Basis vorhandener Tests und Quellvertrag.
- Cancel-Projektion und vollständiger D6-GUI-Simulationslauf: **WARN** — ersteres ohne dedizierten Frontendtest, letzteres ist über die GUI nicht verfügbar.
- Hardwarepfad: nicht getestet und nicht Teil dieser Abnahme.

**Empfehlung: CONDITIONAL PASS als UI-Release-Candidate-Kandidat.** Es gibt keinen gefundenen Blocker im geprüften, hardwarefreien Bedienpfad. Vor einer uneingeschränkten RC-Freigabe sollte mindestens die Cancel-Projektion in einem Frontendtest abgesichert werden. Diese Abnahme erteilt keine Hardwarefreigabe.
