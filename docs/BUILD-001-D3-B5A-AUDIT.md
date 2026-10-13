# Build 001-D3-B5a — Minolta 2,820-dpi Client Profile

## Ergebnis

**PROVEN:** Die angegebene Research-005-Quelle stimmt vor dem Kopieren exakt mit dem freigegebenen SHA-256 überein. Eine unveränderte Kopie und eine davon getrennte Arbeitskopie liegen im Digitizer-Repository.

**PROVEN:** Die Arbeitskopie kennt die CLI-Erweiterung -r DPI mit ausschließlich 600 und 2820, prüft den SANE-Readback sowie die tatsächlich gelieferten SANE-Bildparameter und berechnet die erwartete RGB8-RAW-Größe über geprüfte Arithmetik.

**PROVEN:** Der Offline-Importer kann neben dem unveränderten, festen 600-dpi-Profil variable, intern konsistente 2.820-dpi-RGB8-Geometrien verarbeiten. TIFF-Dimensionen und Stripgröße müssen zu den Parametern passen; TIFF-Pixelbytes müssen mit der vollständigen RAW-Datei übereinstimmen.

**OPEN:** Es gibt weiterhin keine Hardwarebestätigung für 2.820 dpi. Sämtliche neuen 2.820-dpi-Testdaten sind synthetisch und bleiben als OfflineImport klassifiziert. Kein produktiver Scannerstart und keine CaptureKind::Production-Freigabe sind enthalten.

Baseline: main nach PR #15, Mergecommit b137928.

## Quellenherkunft und Änderungen

Vor Übernahme wurde /Users/bernd/Minolta-Research-005-PASS/minolta-batch-frame.c gehasht. Der tatsächliche SHA-256 entsprach exakt dem vorgegebenen Wert:

35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae

Die Datei wurde bytegetreu nach src-tauri/vendor/minolta-batch-frame/minolta-batch-frame.original.c kopiert; der Zielhash wurde erneut verifiziert. Das Research-005-Verzeichnis wurde nicht verändert. Die Quelle ist durch Inhaltshash, nicht durch einen Upstream-Git-Commit identifiziert; Lizenz- und reproduzierbare Original-Buildinformationen lagen nicht vor (OPEN, siehe README im Quellbereich).

Die abgeleitete Arbeitskopie minolta-batch-frame.c unterscheidet sich vom Original durch:

- CLI-Erweiterung -r DPI; Default bleibt 600, nur 600 und 2820 werden akzeptiert. Numerische DPI-Tokens werden vollständig und strikt geprüft; andere Werte enden mit Usage-Fehlercode 2.
- Setzen und direktes Zurücklesen der SANE-Resolution-Option; ein abweichender Readback wird abgelehnt.
- Prüfung von RGB-Format, 8-Bit-Tiefe, positiven Abmessungen und dichtem RGB8-Stride (bytes_per_line == pixels_per_line × 3) vor sane_start() und erneut nach dem Start. Pre-/Post-Parameter müssen übereinstimmen.
- 600 dpi bleibt an die bisherige bekannte Geometrie 564 × 850, 1.692 Byte/Zeile und 1.438.200 Byte gebunden. 2.820 dpi verwendet ausschließlich die tatsächlich gemeldete Geometrie; es wurden keine hochgerechneten Maße ergänzt.
- Geprüfte Berechnung und Begrenzung der RAW-Größe auf 512 MiB sowie den Classic-TIFF-Adressraum. Überlange Datenströme, EOF vor der berechneten Gesamtgröße und inkonsistente Parameter werden fehlgeschlagen, nicht finalisiert.
- Metadaten dokumentieren angeforderte und zurückgelesene Auflösung, drei RGB-Kanäle, tatsächliche Geometrie und erwartete/übertragene RAW-Größe. Metadaten-Schreibfehler verhindern Erfolg.

Die reine Hilfslogik befindet sich in minolta_profile.c/.h; tests/profile_contract_test.c testet sie ohne SANE-Aufruf. Diese Tests führen nicht den Minolta-Client aus. Das harmlose D3-B4-Test-Sidecar und dessen Trust-Hash wurden nicht ersetzt oder geändert.

Geänderte/neue Dateien: src-tauri/vendor/minolta-batch-frame/README.md, minolta-batch-frame.original.c (unveränderte Quelle), minolta-batch-frame.c (Arbeitskopie), minolta_profile.c, minolta_profile.h, tests/profile_contract_test.c, src-tauri/src/offline_import.rs, src-tauri/src/archive.rs und dieser Auditbericht.

## Profil- und Validatorvertrag

Der Validator akzeptiert nur die expliziten DPI-Werte 600 und 2.820. Für beide gilt RGB, 8 Bit pro Kanal, exakt drei Bytes je Pixel, positiver Zeilen-/Bildumfang und eine per checked arithmetic ermittelte RAW-Größe bis 512 MiB. Padded SANE-Zeilen werden ausdrücklich abgelehnt, weil der unterstützte Client-TIFF-Writer nur den aktiven Pixelbereich schreibt.

Für 600 dpi bleiben die etablierten Dimensionen und 1.438.200 RAW-Bytes unverändert verbindlich. Für 2.820 dpi kommen Breite, Höhe und Stride aus params.txt; DPI-Anforderung, SANE-Readback, Metadaten, Pre-/Post-Parameter, Log-Gesamtbytezahl, RAW-Dateigröße, TIFF-Abmessungen/Stripbytezahl sowie TIFF-/RAW-Pixelidentität müssen zusammenpassen. TIFF-Auflösungstags sind optional wie im vorhandenen Clientwriter; falls vorhanden, müssen sie die angeforderte DPI ausdrücken.

2.820-dpi-Artefakte werden nur offline und synthetisch getestet. Ihre Geometrieprüfung beweist interne Datenkonsistenz, nicht Scannerfähigkeit, optische Auflösung oder Research-009-Kompatibilität.

## Prüfungen

- Rust: 94 bestanden, 0 fehlgeschlagen, 12 ignorierte Child-Harness-Tests (cargo test).
- cargo fmt --check: PASS.
- cargo clippy --all-targets -- -D warnings: PASS.
- Harmloser C-Profilvertragstest, nativ kompiliert und ausgeführt: PASS (600/2.820, ungültige/überlange Tokens, bisheriges 600-Profil, variable synthetische 2.820-Geometrie, falsche Kanäle/Tiefe/Stride und Größenlimits).
- Angepasster C-Client samt Profilmodul: Clang-Syntaxcheck mit vorhandenen SANE-Headern PASS; separate ARM64-Objektdateien wurden erzeugt und als Mach-O arm64 geprüft. Es wurde nicht gelinkt und nicht ausgeführt.
- TypeScript-Prüfung: PASS.
- Frontendtests: 7 bestanden.
- Vite-Produktionsbuild: PASS.
- Nativer ARM64-Tauri-App-Build (--bundles app): PASS.
- git diff --check: PASS.

Die ARM64-App paketiert weiterhin ausschließlich das harmlose D3-B4-Test-Sidecar. Die Minolta-Arbeitskopie wird nicht paketiert. Es wurden weder SANE-Funktionen aufgerufen noch der Minolta-Client gestartet.

## Noch offen vor dem ersten Hardwaretest

1. Lizenz, verbindliche Quellrevision und reproduzierbare Client-Link-/Buildanleitung klären; die aktuelle Quelle ist lokal per Hash fixiert, aber ohne vorgelagerten Upstream-Commit.
2. Angepasste Clientänderungen unabhängig prüfen, insbesondere DPI-Readback, Parameterfehler, Dateifehler und Verhalten bei Cancel/Timeout.
3. In einem separat freigegebenen Test reale 2.820-dpi-Parameter, RAW-Dateigröße, TIFF-Struktur/Pixeldaten sowie Gamma-/Farbverhalten erheben. Derzeit gibt es dafür keine Hardwarefixtures.
4. Danach die akzeptierte 2.820-dpi-Geometrie und eventuelle gerätespezifische Grenzen festlegen; der generische Offline-Validator ersetzt keine Hardware-Akzeptanz.
5. Separaten Prozess-/Stagingpfad, OS-Prozessnachweis und Artefaktverknüpfung unabhängig auditieren. Der Client ist weiterhin nicht in Tauri/UI integriert; Production-Archivierung bleibt gesperrt.

**Fazit:** D3-B5a schafft einen überprüfbaren Client- und Artefaktprofil-Unterbau, aber keine aktivierte Scannerintegration und keinen 2.820-dpi-Hardware-PASS.
