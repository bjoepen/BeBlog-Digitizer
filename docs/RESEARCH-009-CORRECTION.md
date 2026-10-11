# Research 009 – verbindliche Korrektur zu Audit 001-A

Audit 001-A ordnete einen älteren Gamma-Logzustand irrtümlich dem finalen Hardware-PASS zu. Für BeBlog Digitizer ist ausschließlich der finale Bestand im read-only Referenzarchiv maßgeblich:

`/Volumes/Kioxia/Projekte/Minolta-SANE-Patch/Research-009-Hardware-PASS/`

Verifiziert am 11. Oktober 2026:

- `libsane-avision.1.so` SHA-256: `732820b2bf2c1d525cc1ce91dbfc8852a77130de0e8df0e823b8dab309cc59f6`
- finaler Lauf: `r009-test-01.log` und `r009-test-01.client.log`, Zeitfenster 17:26 Uhr
- `table_raw_size = 4096`
- `table_size = 4096`
- drei Transfers zu je 4096 Gamma-Bytes
- RGB8, 600 dpi
- 1.438.200 RAW-Bytes
- `SANE_STATUS_EOF`
- TIFF erfolgreich geschrieben
- Client-Exitcode 0

Ältere Logs mit `table_size = 512` sind nicht Teil dieses finalen PASS-Nachweises und dürfen nicht mit ihm vermischt werden. Build 001-C verwendet ausschließlich eine kurze Textfixture aus dem finalen Lauf. Weder Backend noch Scannerclient werden ausgeführt oder verändert.
