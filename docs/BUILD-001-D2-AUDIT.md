# Build 001-D2 — Prozess-Lifecycle Audit

Auditbasis: `main` nach PR #4, Commit `3df80296274b845313b7032ae5e1a36ac2ae9c01`.
Die Prüfung war read-only. Es wurden weder Scannerhardware noch SANE oder ein realer Prozess verwendet.

## Vorhandene Garantien

- `ProcessRunner` liefert `ProcessEvent`s und ein `ProcessOutcome`; der einzige implementierte Runner ist `RecordedProcessRunner` (`src-tauri/src/process.rs`). Er ruft keine Shell und kein `std::process` auf.
- `FakeScannerAdapter` prüft den Exitcode, EOF, die protokollierte Gesamtbytezahl und die TIFF-Erfolgsmeldung (`src-tauri/src/adapter.rs`). Archivfinalisierung validiert anschließend tatsächliche Mock-RAW-/TIFF-Inhalte und schreibt Dateien kollisionssicher (`src-tauri/src/archive.rs`).
- Der Batch-Core verarbeitet Frames seriell mit eigenem Adapterlauf und Capture je Frame. Die 30-Sekunden-Pause liegt zwischen, nicht nach Frames; Batch-Abschluss hängt von der Journalfinalisierung ab (`src-tauri/src/core.rs`).
- Abgeschlossene Archive werden bei späteren Fehlern nicht ersetzt. Beim Start markiert `recover_incomplete` offene Capture- und Batchmanifeste als `Interrupted` und setzt sie nicht fort (`src-tauri/src/archive.rs`).
- Die State Machine weist unzulässige Zustandswechsel zurück (`src-tauri/src/state_machine.rs`). Der Batchpfad speichert ungültige Adapterübergänge als Fehler.

## Konkrete Lücken

- Die Ereignis-API hatte keinen expliziten Startfehler, Pipe-EOF oder Cleanup-Fehler. Daher konnte ein Adapter einen erfolgreichen Lauf nicht gegen die vollständige Prozess-Lifecycle-Spur verifizieren.
- `RecordedProcessRunner` emittierte `Started` auch bei bereits angefordertem Abbruch und nutzte kurze echte Sleeps; Timeout- und Abbruchtests waren dadurch nicht vollständig deterministisch.
- Der Adapter ignorierte Exit-/Cancel-/Timeout-Ereignisse während der Verarbeitung und vertraute anschließend allein auf `ProcessOutcome`. Fehlende, widersprüchliche oder verspätete Lifecycle-Ereignisse wurden nicht als Protokollfehler erkannt.
- Der Datenstrom wurde anhand des gemeldeten Capture-Endstands, nicht anhand eines monotonen, vollständigen Datenfortschritts validiert. EOF-/Capture-/TIFF-Reihenfolge war nicht strikt erzwungen.
- Im Single-Scan-Core wurden ungültige Adapterübergänge zwar gemeldet, aber nicht als latenter Jobfehler gespeichert; spätere Ereignisse konnten den Job noch erfolgreich finalisieren.
- Fehler beim Setzen eines Capture-Fehlerstatus wurden in mehreren Fehlerpfaden ignoriert. Batch-Journalfehler verhindern bereits einen falschen Batch-PASS, aber Capture-Fehler müssen ebenfalls sichtbar bleiben und für Recovery offen markiert werden.

## D2-Abnahmeregeln

Ein Capture darf nur erfolgreich sein, wenn Start, beide geschlossenen Ausgabeströme, erfolgreicher Exit, Cleanup, vollständige monotone Datenfolge, EOF, korrekte Gesamtbytezahl, TIFF-Erfolg und Archivvalidierung nachgewiesen sind. Startfehler, Timeout, Abbruch, unerwartetes Ende, widersprüchliche oder verspätete Ereignisse sowie Cleanup-/Archivfehler bleiben terminale Fehler; spätere Fortschrittsereignisse dürfen sie nicht aufheben.

D2 bleibt vollständig hardwarefrei. `RecordedProcessRunner` simuliert Fehler deterministisch ohne Wartezeiten. Batchumfang bleibt Frames 1–4, ein Prozesslauf je Frame, Halterbestätigung und drei 30-Sekunden-Cooldowns bei Erfolg. Die bestehende Research-Dokumentation weist Hardwarepositionen 1–6 aus; D2 erweitert die D1-Simulation ausdrücklich nicht auf Frames 5–6.

## D2-Umsetzung

- `ProcessEvent` unterscheidet Startfehler, stdout/stderr-Ende, Exit, Abbruch, Timeout, unerwartetes Ende, erfolgreiches Cleanup und Cleanupfehler. `FakeScannerAdapter` prüft diese Ereignisse gegen den zurückgegebenen Outcome und verwirft doppelte, fehlende oder verspätete Ereignisse.
- Die Fake-Prozessläufe warten nicht mehr in Echtzeit. Timeout und Abbruch werden über aufgezeichnete Szenarien bzw. kontrollierte Ereignispunkte deterministisch ausgelöst.
- Erfolgslogs werden nur bei monotoner vollständiger Bytefolge sowie richtiger EOF → Capture-Endstand → TIFF-Reihenfolge akzeptiert. Späte Scanner- oder Fortschrittsmeldungen überschreiben keine gelatchten Lifecycle-/State-Machine-Fehler.
- Fehler beim Persistieren eines Capturestatus werden im Fehlerereignis sichtbar; ein zusätzlich scheiternder Fehlerstatus beim Archivfinalisieren wird mit Recovery-Hinweis zurückgegeben.
- Tests decken Start-/Exit-/Cleanupfehler, Timeout, Abbruch vor und während des Laufs, unvollständige oder verspätete Ereignisse, Ausgabefehler, Single-Scan-State-Rücksprung, fehlgeschlagenen Batch ohne Retry sowie den vollständigen Vierer-Batch ab.
