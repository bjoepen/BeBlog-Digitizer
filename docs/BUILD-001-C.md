# Build 001-C – Sidecar- und Archivgrundlage

## Sicherheitsmodell

Build 001-C enthält keinen echten Prozessstarter und keinen Real-Adapter. `RecordedProcessRunner` implementiert dieselbe kontrollierte Prozessschnittstelle vollständig im Speicher. Er verwendet weder Shell noch `std::process` und kann ausschließlich aufgezeichnete beziehungsweise synthetische Ereignisse liefern.

## Prozessvertrag

Der Runner liefert typisierte Ereignisse für Start, stdout, stderr, Exit, Timeout, Abbruch und abschließende Bereinigung. `CleanedUp` ist auf jedem simulierten Ausgang das letzte Ereignis. Der defensive Parser erkennt nur bekannte, eng definierte Zeilen; unbekannte oder fehlerhafte Meldungen bleiben sichtbare Diagnosen und verändern den Kontrollfluss nicht.

Ein Exitcode 0 ist allein kein Erfolg. Erfolg erfordert zusätzlich:

- EOF-Bestätigung,
- exakt 1.438.200 gemeldete Capture-Bytes,
- TIFF-Bestätigung,
- erfolgreiche Archivfinalisierung.

## Archivvertrag

Jeder Start reserviert vor dem Scan ein neues Capture-Verzeichnis. `create_dir` und `create_new` verhindern Kollisionen und Überschreiben vorhandener Originale. Das Manifest beginnt mit `inProgress` und wird atomar über eine temporäre Datei fortgeschrieben. RAW und TIFF erhalten Größe, Medientyp und SHA-256. Beim Start werden zurückgelassene `inProgress`-Manifeste einmalig als `interrupted` markiert.

Die in diesem Build geschriebenen Mock-Artefakte tragen explizite Fixture-Inhalte und sind keine Produktionsscans.

## Verifikation

Erfolgreich ausgeführt am 11. Oktober 2026:

- `cargo test`: 21 Tests bestanden
- `cargo clippy --all-targets -- -D warnings`: bestanden
- `cargo fmt --check`: bestanden
- `pnpm check`: TypeScript-Prüfung bestanden
- `pnpm test`: 3 Frontendtests bestanden
- `pnpm build`: Produktions-Frontend gebaut
- `pnpm tauri build --bundles app`: ARM64-macOS-App erfolgreich gebaut

Die Rust-Suite prüft insbesondere Erfolg, Exitcode 1, Abbruch, Timeout, unvollständigen Stream bei Exitcode 0, unbekannte und fehlerhafte Logzeilen, garantierte Mock-Prozessbereinigung, parallelen Job-Ausschluss, Archivkollisionen, SHA-256-Metadaten und Wiederherstellung eines unterbrochenen Manifests.
