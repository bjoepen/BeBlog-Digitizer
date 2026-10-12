# Build 001-D1 – Simulierte Vierer-Batchsteuerung

## Batch-Ablauf

Der Rust-Core reserviert eine Batch-ID und die geordnete Frame-Liste (standardmäßig 1–4). Vor der Filmhalterbestätigung läuft kein Adapterjob. Nach der Bestätigung verarbeitet derselbe `AppCore` jeden Frame sequenziell über `FakeScannerAdapter` und `RecordedProcessRunner`, validiert und archiviert jedes Capture separat und hält zwischen erfolgreichen Frames eine 30-Sekunden-Sperre. Nach Frame 4 gibt es keine weitere Pause.

**Umfangsgrenze:** Frames 1–4 sind ausschließlich der festgelegte Simulationsumfang von Build 001-D1. Das reale Backend unterstützt Filmpositionen 1–6; diese Korrektur erweitert weder UI noch Batch-API auf sechs Frames. Eine spätere Sechser-Erweiterung erfordert einen separaten Build und Hardwarefreigabe.

Während Bestätigung, Scan und Pause bleibt der Batch exklusiv reserviert; parallele Starts werden abgewiesen. Abbruch oder Fehler starten keinen Retry. Bereits abgeschlossene Capture-Manifeste und Dateien bleiben erhalten. Bei Anwendungsneustart werden zurückgelassene `inProgress`-Capture-Manifeste als `interrupted` markiert; Batches werden nicht automatisch fortgesetzt.

Die Sperre liegt vollständig im Rust-Core. `BatchClock` ist injizierbar; die Anwendung nutzt eine monotone, abbrechbare Ein-Sekunden-Wartefunktion. Das Frontend zeigt den vom Core gemeldeten Countdown und steuert keine Zeitlogik.

## Zustands- und Eventmodell

Die bestehende `ScanStateMachine` akzeptiert weiter den Einzel-Scan-Pfad und außerdem `awaitingHolderConfirmation`, `waitingForHolderDetection`, `positioningFrame`, `scanning`, `closingScanner`, `interFrameCooldown` und die Batch-Terminalzustände. `ScanEvent.batch` enthält Batch-ID, Frame-Reihenfolge, aktuellen Frame, abgeschlossene Frames, Gesamtzahl und Countdown. Der Prozess bleibt eine Recorded-/Fake-Simulation; es gibt weder einen echten Prozessstarter noch einen Real-Adapter.

## Simulation und Recovery

Das UI bietet Erfolg, Prozessfehler, Timeout, unvollständigen Stream sowie fehlende oder ungültige Mock-Ausgabe. Fehler können für Frame 2, 3 oder 4 festgelegt werden. Die Fake-Ausgaben sind Simulationen, keine Filmscans. Bei einem Fehler bleiben frühere erfolgreich archivierte Frames bestehen. Ein Neustart markiert unvollständige einzelne Capture-Manifeste als unterbrochen; es gibt keine Batch-Wiederaufnahme.

## Visueller Abnahmetest

1. `pnpm tauri dev` starten und die Kennzeichnung „Fake-Adapter“ prüfen.
2. Vierer-Batch vorbereiten. Vor der Bestätigung müssen Status „Filmhalter jetzt einsetzen“ und der Bestätigungsknopf sichtbar sein; kein Scanfortschritt darf beginnen.
3. Bestätigen und den Ereignisverlauf bis Frame 1, Countdown, Frames 2–4 und „Batch abgeschlossen“ beobachten.
4. Während eines Countdowns abbrechen und bestätigen, dass kein nächster Frame startet.
5. Einen Fehlerfall auf Frame 2/3/4 wählen und prüfen, dass frühere Captures erhalten und spätere Frames nicht gestartet werden.

Alle obigen Abläufe bleiben hardwarefrei. Der visuelle Lauf ist eine manuelle Abnahme; automatisierte UI-Tests prüfen Rendering und State-Projektion.

## Build-001-C-Dokumentationsdrift

Build 001-C war hardwarefrei und nutzte den In-Memory-`RecordedProcessRunner`; die Dokumentation bezeichnet die auswählbaren Szenarien an anderer Stelle uneinheitlich. Die verbindliche Szenarioliste für C war Erfolg, Prozessfehler, Timeout und unvollständiger Datenstrom. Produktions-RAW-Länge und TIFF-Struktur wurden durch Archivtests validiert; Mock-Artefakte waren ausdrücklich Fixture-Daten. D1 ergänzt die hier aufgeführten Ausgabefehler-Simulationen. Historische Testzahlen beziehen sich auf den jeweiligen damaligen Commit und sind keine aktuellen D1-Prüfergebnisse.
