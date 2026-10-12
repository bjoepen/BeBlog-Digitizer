# BeBlog Digitizer – Build 001-D1

macOS-Anwendungsgrundgerüst mit Tauri v2, Svelte, TypeScript und einem ausschließlich simulierten Scanner-Adapter.

## Sicherheitsgrenze

Dieser Build enthält nur `FakeScannerAdapter`. Er startet weder SANE noch den Minolta-Batch-Client und besitzt keine Shell-Berechtigung. Ein Real-Adapter ist absichtlich nicht implementiert.

## Entwicklung

```sh
pnpm install
pnpm validate
pnpm tauri dev
```

Die Oberfläche steuert einen simulierten Vierer-Batch. Fehler-/Timeout-/Ausgabeszenarien können gezielt Frame 2, 3 oder 4 zugeordnet werden. Ein Batch startet erst nach separater Filmhalterbestätigung und kann kontrolliert abgebrochen werden.

Build 001-C ergänzt eine rein simulierte, typisierte Prozessschnittstelle und eine sichere Archivgrundlage. Build 001-D1 ergänzt einen geführten Vierer-Batch mit expliziter Halterbestätigung, Archivierung pro Frame und 30-Sekunden-Sperre zwischen erfolgreichen Frames. Details stehen in [docs/BUILD-001-C.md](docs/BUILD-001-C.md) und [docs/BUILD-001-D1.md](docs/BUILD-001-D1.md); die verbindliche Research-009-Korrektur in [docs/RESEARCH-009-CORRECTION.md](docs/RESEARCH-009-CORRECTION.md).

Die Textfixture `src-tauri/tests/fixtures/research-009-success.fixture` enthält ausgewählte, nicht ausführbare Zeilen aus dem read-only Hardware-PASS-Protokoll. Es werden keine Research-Artefakte verändert oder zur Laufzeit benötigt.
