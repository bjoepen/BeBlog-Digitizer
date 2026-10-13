# Build 001-D3-B5b – Hardwaretest-Vorbereitung Audit

## Ergebnis

**PASS (hardwarefrei vorbereitet); Hardwarefreigabe bleibt gesperrt.** Der Branch basiert auf `main` nach PR #16 (`be14775`). Die Arbeitskopie war vor Beginn sauber. Die lokale Research-005-Quelldatei und der unveränderte Snapshot `src-tauri/vendor/minolta-batch-frame/minolta-batch-frame.original.c` stimmen mit SHA-256 `35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae` überein. Weder Snapshot noch Research-005/009, SANE, Avision, Homebrew oder das D3-B4-Test-Sidecar wurden geändert.

Der C-Client wurde **nicht gebaut oder ausgeführt**. Es gab keinen Gerätezugriff, keine SANE-Aufrufe und keinen Hardwaretest. Der Rust-Vorbereitungshelfer ist nur im Testbuild verfügbar (`cfg(test)`), erstellt bei Aufruf synthetischer Tests frische Verzeichnisse und Argumentvektoren. Das separate `tools/run-minolta-hardware-test.sh` ist nicht mit Tauri, UI oder App-Start verbunden. Es stoppt geschlossen, solange ein vorab unabhängig erstelltes `tools/minolta-hardware-test.sha256` und ein ARM64-Client am festen Testpfad fehlen. Das Skript wurde nur mit `bash -n` geprüft, nicht ausgeführt.

## CLI-Korrektur

Der Ausgangscode verwendete für `-f`, `-t` und `-i` `strtoul(optarg, NULL, 10)`. Dadurch wurden etwa nachgestellte Zeichen nicht zuverlässig verworfen; zusätzliche Positionsargumente nach `getopt` wurden ebenfalls nicht abgelehnt. `-r` hatte bereits eine strengere Ziffernprüfung.

`minolta_profile.c/.h` stellen jetzt `minolta_parse_bounded_uint()` als gemeinsame Prüfung bereit: nur nichtleere ASCII-Dezimalziffern, keine Vorzeichen/Leerzeichen/Suffixe, Überlauf- und Bereichsprüfung. `-r` bleibt auf exakt 600 oder 2820 beschränkt. Für die Holder-Polling-Optionen gilt:

| Option | Gültiger Bereich | Bedeutung |
| --- | ---: | --- |
| `-f` | 1–4 | Clientumfang dieses Builds |
| `-r` | 600 oder 2820 | Unterstützte RGB8-Profile |
| `-t` | 1–3600 Sekunden | Nur Timeout der Filmhaltererkennung, kein Scan-Timeout |
| `-i` | 1–60 Sekunden und `i ≤ t` | Pollingintervall der Filmhaltererkennung |

Defaults bleiben `-f 1`, `-r 600`, `-t 120`, `-i 2`; damit bleiben bisherige gültige Aufrufe unverändert. Leere Device-/Prefixwerte, ein Intervall größer als das Timeout und nicht konsumierte zusätzliche Argumente werden zurückgewiesen. Die Obergrenzen für `-t` und `-i` sind defensive CLI-Grenzen dieses Builds, keine vom Scanner abgeleiteten Fähigkeiten.

## Getrennter manueller Hardwaretestablauf

Dieser Ablauf ist eine spätere Freigabekarte, keine in diesem Build ausführbare Integration:

1. Vor dem Test muss ein unabhängig geprüfter ARM64-Client-Build mit freigegebenem, extern hinterlegtem SHA-256 vorliegen. `minolta-batch-frame.original.c` ist nur die Quellreferenz; das veränderte Arbeitsverzeichnis ist kein freigegebenes Binary. Dieser Build hat kein Clientbinary erzeugt.
2. Der Bediener muss den aktuell gültigen SANE-Gerätebezeichner vor Ort ausdrücklich bestimmen und bestätigen. Keine alte USB-Adresse übernehmen oder erraten; dieser Build führt keine Geräteerkennung aus.
3. Einen neuen, exklusiven Versuchspfad verwenden, etwa unter einem eigens festgelegten lokalen Teststamm. Das Verzeichnis muss durch atomare Verzeichniserstellung neu angelegt werden; bei Existenz abbrechen. Nicht bereinigen oder wiederverwenden. Fester Präfix darin: `capture`.
4. Vor dem Start sichtbar gegenprüfen: bestätigter Gerätebezeichner, Frame `1`, angeforderte Auflösung `2820 dpi`, `RGB8`, TIFF sowie Holder-Polling `-t 120 -i 2`. Der Testhelfer bildet die Argumente ohne Shell-Konkatenation: `-d DEVICE -o NEW_DIR/capture -f 1 -r 2820 -t 120 -i 2`.
5. Erst nach separater Freigabe genau einen Prozesslauf auslösen. Keine Wiederholung, keine Resume-Funktion. Bei Abbruch oder Fehler stoppen; ein mit SIGKILL beendeter Prozess ist kein Nachweis für SANE-Cleanup. Der C-Client behandelt SIGINT/SIGTERM kooperativ über ein `sig_atomic_t`-Flag; bei aktivem Scan führt der Hauptthread `sane_cancel()` aus, danach folgen `sane_close()` (falls geöffnet) und `sane_exit()`. Vor Scanstart wird geschlossen/beendet, ohne `sane_cancel()`. Reguläre Fehler laufen über denselben Cleanup-Abschnitt. Der Client kann den Erfolg von `sane_cancel()`/`sane_close()` nicht anhand eines Rückgabestatus belegen (die API-Funktionen liefern hier keinen Status). SIGKILL und Strom-/Systemabbruch können den Cleanup-Pfad umgehen.
6. Protokollieren: tatsächlichen OS-Exitcode; die vier Dateien `.client.log`, `.raw`, `.params.txt`, `.tiff`; angeforderte und von SANE zurückgelesene DPI; `format`, Breite, Höhe, Bytes/Zeile, Tiefe; erwartete und tatsächliche RAW-Länge; TIFF-Geometrie und Pixelidentität RAW↔TIFF. Bestehenden Offline-Importer/Validator verwenden, aber dessen Ergebnis bleibt `OfflineImport`, nicht `Production` und beweist keinen Prozessabschluss.

Der aktuelle Client öffnet die Ausgabedateien mit überschreibenden Modi. Daher muss der separate spätere Ablauf zwingend ein neues, exklusiv angelegtes Verzeichnis und den festen Präfix `capture` verwenden; bestehende Dateien dürfen nicht als Ziel dienen. Es gibt in diesem Build absichtlich weder einen Tauri-Befehl noch einen App-Launcher, der diesen Client startet. Nur das isolierte manuelle Skript kann später nach Vorliegen von Client, separat erstelltem Vertrauenshash und zweifacher wörtlicher Bestätigung einen einzelnen Prozess starten. Bis zu einer separaten Hardwarefreigabe darf weder der Client noch das Skript ausgeführt werden.

## Hardwareprofil und offene Voraussetzungen

- Zielprofil: einzelnes Frame 1, 2820 dpi, RGB8, TIFF plus RAW/Parameter/Log.
- Bildabmessungen und RAW-Größe für 2820 dpi werden ausschließlich aus den beim späteren Scan zurückgegebenen SANE-Parametern berechnet; es werden keine hochgerechneten festen Maße angenommen.
- Für den 600-dpi-Referenzfall bleibt `1438200` Byte aus dem bisherigen Researchnachweis erhalten.
- 2820 dpi ist **nicht hardwarebestätigt**. Es fehlen tatsächliche Scannerparameter und ein echtes Artefaktset.
- Vor dem Test fehlen ein überprüfter ARM64-Client-Build samt unabhängig freigegebenem Hash, explizit bestätigter aktueller Device-Name und separate Hardwarefreigabe.
- Die Tauri-App bleibt Fake-only; `CaptureKind::Production` bleibt gesperrt.

## Prüfungen

- C-Vertragsprüfung: kompiliert und ausgeführt nur `minolta_profile.c` plus `tests/profile_contract_test.c` mit `clang -std=c11 -Wall -Wextra -Werror`; PASS. Getestet wurden gültige 600/2820, ungültige Zeichen/Vorzeichen/Overflow, Frame-/Timeout-/Intervallgrenzen sowie `i ≤ t`. Das Minolta-Clientprogramm wurde nicht gelinkt oder gestartet.
- Hardwaretestskript-Syntax: `bash -n tools/run-minolta-hardware-test.sh`; PASS. Skript nicht ausgeführt.
- `cargo fmt --check`: PASS.
- `cargo test -- --test-threads=1`: PASS, 97 bestanden, 0 fehlgeschlagen, 12 bewusst ignorierte OS-Runner-Childtests.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `pnpm check`: PASS; `pnpm test`: PASS (3 Dateien, 7 Tests); `pnpm build`: PASS.
- `pnpm tauri build --bundles app`: PASS; App-Bundle erzeugt und ausführbares Binary als `arm64` verifiziert.
- `git diff --check`: PASS.
- Artefaktfehler/-unvollständigkeit: durch die vorhandenen `offline_import`-Regressionstests abgedeckt; beim tatsächlichen späteren Test sind alle vier Originalartefakte anhand der realen Ergebnisse zu prüfen.

Client-C-Syntaxcheck wurde versucht, aber ist **OPEN/ENVIRONMENT-BLOCKED**: `pkg-config` ist nicht vorhanden und der verwendete Clang-Suchpfad enthält `sane/sane.h` nicht. Es wurde kein alternativer Backend-/Headerpfad gesucht oder verändert und der C-Client wurde nicht gelinkt. Die reine Parser-/Profilbibliothek und deren Vertragstest benötigen keine SANE-Header und bestehen separat.

## Freigabecheckliste für einen späteren Hardwaretest

- [ ] Unabhängige Prüfung des Client-Diffs gegenüber dem gehashten Original und ARM64-Buildnachweis
- [ ] Unabhängig festgelegter und gegengeprüfter Client-SHA-256
- [ ] Aktueller SANE-Gerätebezeichner ausdrücklich bestätigt (keine alte USB-Adresse)
- [ ] Neues exklusives Verzeichnis, keine vorhandenen Zieldateien
- [ ] Sichtbare Bestätigung von Device, Frame 1, 2820 dpi, RGB8 und genau einem Start
- [ ] Separate Freigabe erteilt; kein Start aus UI/App-Start
- [ ] Nachlaufende Prüfung von Exitcode, vier Dateien, Parametern, RAW-Länge, TIFF-Struktur/-Pixeln und Prüfsummen
- [ ] Bei jedem Fehler stoppen; keine Wiederholung oder automatische Wiederaufnahme
