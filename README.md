# BeBlog Digitizer – Build 001-B2

macOS-Anwendungsgrundgerüst mit Tauri v2, Svelte, TypeScript und einem ausschließlich simulierten Scanner-Adapter.

## Sicherheitsgrenze

Dieser Build enthält nur `FakeScannerAdapter`. Er startet weder SANE noch den Minolta-Batch-Client und besitzt keine Shell-Berechtigung. Ein Real-Adapter ist absichtlich nicht implementiert.

## Entwicklung

```sh
pnpm install
pnpm validate
pnpm tauri dev
```

Testszenarien `Research-009-Erfolg`, `Simulierter Lesefehler`, `Simulierter Halter-Timeout` und `Unvollständiger Datenstrom` sind in der Oberfläche auswählbar. Ein laufender Job kann kontrolliert abgebrochen werden.

Build 001-C ergänzt eine rein simulierte, typisierte Prozessschnittstelle und eine sichere Archivgrundlage. Details stehen in [docs/BUILD-001-C.md](docs/BUILD-001-C.md); die verbindliche Research-009-Korrektur in [docs/RESEARCH-009-CORRECTION.md](docs/RESEARCH-009-CORRECTION.md).

Die Textfixture `src-tauri/tests/fixtures/research-009-success.fixture` enthält ausgewählte, nicht ausführbare Zeilen aus dem read-only Hardware-PASS-Protokoll. Es werden keine Research-Artefakte verändert oder zur Laufzeit benötigt.
