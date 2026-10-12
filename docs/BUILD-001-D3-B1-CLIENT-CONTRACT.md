# Build 001-D3-B1 — Vertrag des vorhandenen Minolta-Clients

Status: Implementierungsvertrag und Testgrundlage; kein produktiver Runner. Der FakeScannerAdapter bleibt der einzige aktive Adapter.

## CLI und Ausgabedateien (Codebeleg)

Quelle: `~/Minolta-Research-005-PASS/minolta-batch-frame.c`, geprüft, nicht ausgeführt.

| Argument | Bedeutung | Im C-Code erzwungener Bereich / Default |
|---|---|---|
| `-d DEVICE` | erforderlicher SANE-Gerätebezeichner | muss angegeben sein; kein internes Geräte-Whitelist- oder Formatcheck |
| `-o PREFIX` | Ausgabepräfix | Default `minolta-batch-frame`; Suffixpfade müssen in je 1024 Zeichen passen |
| `-f FRAME` | ausgewählte Filmposition | Default 1, akzeptiert 1–4 (nicht 1–6) |
| `-t SECONDS` | Frist für Halterpolling | Default 120, nach Konvertierung > 0 |
| `-i SECONDS` | Pollingintervall | Default 2, nach Konvertierung > 0 und <= `-t` |

Die Argumente werden mit `getopt()` gelesen. Numerische Werte werden über `strtoul()` ohne Prüfung auf vollständigen Zahlentoken oder `errno` übernommen und dann auf `unsigned` reduziert. Der spätere Runner soll daher bereits typisierte, validierte Werte einsetzen, keine UI-Eingabe unverändert weiterreichen.

Für `PREFIX` erzeugt der Client `PREFIX.client.log`, `PREFIX.raw`, `PREFIX.params.txt` und `PREFIX.tiff`. Die Dateien werden mit `fopen(..., "wb")` angelegt; bestehende gleichnamige Dateien können überschrieben werden. Der Runner muss ein exklusives, frisches Stagingverzeichnis bereitstellen und den Prefix selbst sicher darin bilden.

RAW wird während `sane_read()` geschrieben. Bei EOF schreibt der Client Textmetadaten und versucht, aus dem RAW ein unkomprimiertes Baseline-TIFF für unterstützte Gray-/RGB-Parameter zu erzeugen. Das Metadatenformat besteht aus `key=value`-Zeilen (u. a. `device`, `format`, `bytes_per_line`, `pixels_per_line`, `lines`, `depth`, `raw_bytes`, `terminal_status`). `write_metadata()` liefert einen Status zurück, der Aufruf ignoriert diesen aber; Metadatenfehler können deshalb trotz erfolgreichem Scan/TIFF den Exitcode 0 unberührt lassen. Dateien müssen unabhängig geprüft werden.

## Streams und Exitcodes (Codebeleg)

Die Clientfunktion `diag()` schreibt jede eigene Diagnose nach `stderr` und zusätzlich in `.client.log`; Nutzbilddaten gehen in `.raw` und nicht auf stdout. Die Usage-Ausgabe erfolgt ebenfalls nach stderr. Das erfolgreiche Research-005-`frame1.client.log` ist ein Originalspiegel dieser Clientdiagnosen (Quelle SHA-256 unten). Das Batchskript leitet stdout und stderr zusammen in `.terminal.log`; diese kombinierte Datei darf nicht als streamgetreue Quelle behandelt werden.

Exitcodes im C-Code: `0` nur bei `SANE_STATUS_GOOD`; `1` bei Logöffnung oder SANE-/Scan-/TIFF-Fehlern; `2` bei Usage-/Argumentfehlern bzw. zu langen Pfaden. Bei `sane_init()`-Fehler kehrt `main()` vor der allgemeinen Exitdiagnose zurück. Der Logmarker `client exit -> N` ist eine zusätzliche Clientdiagnose, aber ersetzt den tatsächlichen OS-Exitstatus nicht.

## Abbruch und Cleanup (Codebeleg / Grenze)

Der Client installiert Handler für SIGINT und SIGTERM, die ein Signalflag setzen. Beim aktiven Scan prüft die `sane_read()`-Schleife das Flag. Bei erkanntem abnormalem Ende ruft `done:` `sane_cancel()` auf, danach – soweit ein Handle vorliegt – `sane_close()`, anschließend `sane_exit()`. Rückgabewerte dieser Cleanupfunktionen sind nicht prüfbar (SANE deklariert sie als `void`); der Prozessrunner muss sein eigenes Ende und Reaping nachweisen. Ein Verhalten bei dauerhaft blockierendem `sane_read()` oder Wirkung eines harten Kill auf den Scanner ist **OPEN**. `-t` begrenzt nicht die gesamte Prozesslaufzeit, sondern nur `wait_for_holder()`.

`prompt_exact()` existiert in der C-Datei, wird von `main()` aber nicht aufgerufen. Die Aufforderung „START“ ist Bestandteil von `minolta-batch4.sh`, nicht des direkten Clientaufrufs. In BeBlog muss deshalb die ausdrückliche Halterbestätigung vor dem Prozessstart liegen.

## Capability-Grenze

Der Halterdescriptor kann im Betrieb Positionen 1–6 anzeigen; Research-009 protokolliert den erfolgreichen Wechsel des Descriptorbereichs von `1..1` auf `1..6`. Der aktuelle C-Client akzeptiert jedoch nur Frameargumente 1–4. D3-B1 dokumentiert daher ausschließlich 1–4 als Clientumfang. Die Hardwarecapability erweitert die aktuelle CLI nicht.

## Fixture-Provenienz

- `src-tauri/tests/fixtures/research-009-success.fixture`: vorhandener, kurzer Textauszug aus dem finalen Research-009-Hardware-PASS (17:26-Lauf); im Fixture selbst als Textfixture gekennzeichnet. Finaler Clientlog SHA-256: `003af7035834dee9eb929c59bcfeb1842c8566ea457fb46a381732a79025f627`. Er belegt u. a. 4096er Gammaübertragung, RGB8/600 dpi, EOF, 1.438.200 Byte, TIFF und Exit 0. Dies ist ein eigener Einzelbildlauf.
- `src-tauri/tests/fixtures/research-005-frame1-client-stderr.fixture`: verbatim ausgewählte Zeilen aus `~/Minolta-Research-005-PASS/scan-results/frame1.client.log`, SHA-256 der vollständigen Quelldatei `aab64424b860ba3f98d529ca5fbaf4714955048f893f8174b0dde6cb3a20bd6d`. Enthaltene Quellbereiche sind im Fixturekopf genannt. Intermediate `sane_read`-Zeilen fehlen; die Chunkauszüge sind **nicht** als vollständiger Datenstrom zu summieren. Der vollständige Originalclientlog meldet den Abschlussstand separat. Client-`diag()` schreibt nach stderr; die Clientlogdatei selbst ist ein Spiegel, kein originaler separater Pipe-Dump.
- `scan-results/frame1.terminal.log` enthält durch das Research-005-Skript zusammengeführtes stdout/stderr. Seine Backenddiagnosen werden daher nicht in ein bestimmtes Originalstreamlabel umgedeutet. Die dort vorkommende Meldung `Max retry count reached: I/O error` trat in einem Lauf mit anschließendem EOF, TIFF und Exit 0 auf und darf nicht isoliert als Capturefehler behandelt werden.

Fixtures sind Testinputs im Repository und werden nicht als aktuelle Hardwaretests ausgeführt. Sie enthalten keine konstruierten Hardware-Fehlermeldungen.

## Erfolgsnachweise für spätere Builds

Logmeldungen allein reichen nicht für einen Capture-PASS. D3-B3 muss mindestens prüfen: tatsächlicher regulärer Prozessabschluss; nachgewiesenes Child-Reaping und keine Cleanupfehler; tatsächlich vorhandene RAW-Datei mit Profil-/Parameter-konsistenter Größe; valide und zueinander passende Metadaten und Scanparameter; strukturell valide TIFF-Datei; plausible kumulierte Chunkgröße und übereinstimmender Clientgesamtstand; sowie archivierte Dateigrößen und SHA-256. Abbruch/Timeout, fehlender EOF, fehlende Dateien oder inkonsistente Summen bleiben unvollständig/fehlgeschlagen. D3-B1 implementiert keine Dateiübernahme oder produktive Prozessstarts.

## Abgrenzung

D3-B1 korrigiert die Semantik der Logparserdaten und testet aufgezeichnete Streams. D3-B2 verantwortet einen OS-Prozessrunner samt Timeout, Cancel, Streamdrain und Reaping. D3-B3 verantwortet erst danach sichere Artefaktübernahme, RAW-/TIFF-/Metadatenprüfung und Archivfinalisierung. Kein Real-Adapter wird in diesem Build aktiviert.
