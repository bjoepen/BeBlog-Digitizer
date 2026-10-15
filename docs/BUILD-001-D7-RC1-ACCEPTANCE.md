# Build 001-D7-RC1 – UI-Abnahme und Simulatorprüfung

**Prüfdatum:** 15. Oktober 2026

**Modus:** Synchronisierung, Tests und native Abnahme; keine Featureentwicklung.

## Git-Stand

- GitHub-`main`: `03eefa40d5881beff717cbebd64dd9d12f9d522d`.
- `origin/main`: `03eefa40d5881beff717cbebd64dd9d12f9d522d`.
- Lokaler `main` nach Fast-Forward: `03eefa40d5881beff717cbebd64dd9d12f9d522d`.
- Ausgangsbranch war `build/001-d7-b-ui-ergonomics` bei `bc5f34a5084f21eb7d1586d1406c6498af9bcc3d`; anschließend wurde nur der bestätigte `main`-Fast-Forward ausgeführt.
- Arbeitsbaum enthält nach der Abnahme nur diesen ungetrackten Bericht und die geschützte ungetrackte Datei `tools/minolta-hardware-test.sha256`; Anwendungscode wurde nicht geändert.
- SHA-256 der Prüfsummendatei vor und nach Prüfung: `1a03d2d6e7d7fd38452b982e019fae140e2f3bacb7efd63f157b7a3d7ea5a8ee`.

## Verfahren und Testergebnisse

- `pnpm check`: **PASS**.
- `pnpm test`: **PASS**, 4 Testdateien / 21 Tests.
- `pnpm build`: **PASS**.
- D6-Rustregressionen (`cargo test --manifest-path src-tauri/Cargo.toml --lib d6 -- --test-threads=1`): **PASS**, 10 Tests.
- Prozess-E2E (`cargo test --manifest-path src-tauri/Cargo.toml --test guided_process_e2e -- --test-threads=1`): **PASS**, 3 Tests.
- `pnpm tauri build --bundles app`: **PASS**. App: `src-tauri/target/release/bundle/macos/BeBlog Digitizer.app`.
- App-Executable laut `file`: **Mach-O 64-bit arm64**, **PASS**.
- `git diff --check`: **PASS**.

## Native UI-Beobachtungen

Die gebaute ARM64-App wurde nativ bei drei Fensterbreiten geprüft:

- Breit, Screenshot 1049 × 768: Steuerung und Inspector nebeneinander. Hierarchie und Statuswerte sind gut unterscheidbar; Diagnosekennungen und Werte sind lesbar.
- Mittel, 987 × 751: Layout wechselt auf eine Spalte. Controls bleiben innerhalb der Fensterbreite und überlappen nicht.
- Schmal, 792 × 680: Aufgabenbereiche bleiben einspaltig und bedienbar; der Inspector ist durch vertikales Scrollen erreichbar und lesbar. Keine horizontale Seiten-Scrollleiste beobachtet.

Die vier nummerierten Aufgabenbereiche (Scanner, Parameter, Ausgabeordner, Workflow) sind in der Accessibility-Struktur vorhanden. Einzelbild und D6 werden als eigene, gleichrangige Workflowkarten angeboten. Der Inspector enthält Prozess-/Scannerstatus, Halterstatus, Frame, Profil, RAW-Fortschritt, Cooldown und Diagnoseprotokoll. Bei kleinen Höhen ist normales vertikales Scrollen erforderlich; nicht alle unteren Bedienelemente sind gleichzeitig im ersten Viewport.

Lokale Prüf-Screenshots:

- Breit: `/tmp/beblog-d7-rc1.yGbgVJ/wide.jpg`
- Mittel: `/tmp/beblog-d7-rc1.yGbgVJ/medium.jpg`
- Schmal: `/tmp/beblog-d7-rc1.yGbgVJ/narrow.jpg`
- Inspector im schmalen Fenster: `/tmp/beblog-d7-rc1.yGbgVJ/narrow-inspector.jpg`

## Simulator

Der Einzelbildsimulator wurde ausschließlich über die native App gestartet. Nach dem simulierten `OPENED` erschien die Einlegeaufforderung; der Ablauf wurde erst durch den manuellen Klick auf „Filmhalter eingesetzt bestätigen“ fortgesetzt.

- Simulator meldete Abschluss und OS-Exitcode `0`.
- Inspector zeigte `28.800 / 28.800 Bytes` und `100 %` tatsächlichen Simulatorfortschritt.
- Ereignisprotokoll enthielt 16 relevante Prozess-/Scannerereignisse. Datenchunks wurden nicht als zusätzliche sichtbare Logzeilen ausgegeben.
- Inspector und Bedienkarte zeigten den validierten Exportpfad `/private/tmp/beblog-d7-rc1.yGbgVJ/sim-export/BeBlog-Scan-1f7141e979694b66aaded4eed17a2ce7`.
- Export enthielt die vier erwarteten Dateien: `capture.raw`, `capture.tiff`, `capture.params.txt`, `capture.client.log`.
- Es wurden keine Scannererkennung, kein realer Minolta-Client, kein `sane_open` und kein Hardwarezugriff ausgeführt.

Simulator-Screenshots:

- Während der geführten Bestätigung/Diagnose: `/tmp/beblog-d7-rc1.yGbgVJ/simulator-started.jpg`
- Nach erfolgreichem Export und vollständigem Bytefortschritt: `/tmp/beblog-d7-rc1.yGbgVJ/simulator-confirmed.jpg`

### D6-GUI-Abgrenzung

Der D6-Startknopf blieb deaktiviert, weil die App eine explizit erkannte und bestätigte Scannerkennung verlangt. Diese Abnahme hat bewusst keine Scannererkennung ausgelöst. In der normalen Oberfläche ist kein separater D6-Simulator verfügbar. Der D6-Prozessablauf wurde stattdessen durch die vorhandenen automatisierten Prozess-E2E-Tests und D6-Rusttests geprüft (3 beziehungsweise 10 Tests, alle bestanden). Ein nativer GUI-D6-Lauf wurde daher nicht behauptet.

## RC-Bewertung

| Bereich | Befund | Bewertung |
|---|---|---|
| UI-Ergonomie | Aufgaben sind klar gruppiert; Einzelbild und Batch gleichwertig angeboten. | **PASS** |
| Responsives Layout | Breit zweispaltig, mittel/schmal einspaltig; vertikales Scrollen nötig, kein horizontales Überlaufen beobachtet. | **PASS** |
| Inspector | Status, Profil, tatsächliche Bytes, Fortschritt und Diagnose gut lesbar; Screenshot bestätigt 100 % des Simulators. | **PASS** |
| Einzelbildbedienung | Native geführte Simulation inklusive manueller Bestätigung bis validiertem Export erfolgreich. | **PASS** |
| Batchbedienung | UI-Vorhandensein bestätigt; nativer GUI-Batch blieb mangels bestätigtem Gerät gesperrt. D6-Unit-/Prozess-E2E bestanden. | **WARN** |
| Technische Stabilität | Frontend-, D6-, Prozess-E2E- und ARM64-Buildprüfungen bestanden. Hardwarepfad nicht ausgeführt. | **PASS** |
| Dokumentationsstand | D7-A-/D7-B-Berichte vorhanden; dieser Abnahmebericht ergänzt die tatsächlichen Prüfergebnisse. | **PASS** |

## Empfehlung

**WARN – als RC-Kandidat geeignet, aber keine automatische RC-Freigabe.** Für den geprüften UI- und Einzelbild-Simulatorumfang wurde kein Blocker gefunden. Vor einer vollständigen RC-Abnahme ist zu entscheiden, ob die vorhandenen automatisierten D6-Regressionsnachweise ausreichen oder ob zusätzlich ein gesondert freigegebener Hardware-Batchtest erforderlich ist. Diese Prüfung hat keine reale Hardwarefunktion freigegeben.
