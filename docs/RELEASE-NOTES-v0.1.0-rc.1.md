# BeBlog Digitizer v0.1.0-rc.1

Erster privater Release Candidate von BeBlog Digitizer. Diese Vorabversion richtet sich an den lokalen Einsatz auf Apple-Silicon-Macs und begleitet den bereits hardwareerprobten Minolta-DiMAGE-Scan-Dual-II-Workflow.

## Funktionsumfang

- Native macOS-App für ARM64.
- Manuelle Scannererkennung mit verständlicher Geräteauswahl.
- Einzelbildscan für Filmpositionen 1–6.
- Geführter Filmstreifenmodus für 2–6 Bilder; Standard sind vier Bilder bei 2.820 dpi und RGB8. Ein Vierer-Batch wurde an der Minolta-Hardware erfolgreich durchgeführt.
- Geführte Filmhalterbestätigung: Die App fordert erst nach bestätigtem Öffnen der Scannerverbindung zum Einsetzen auf. Die tatsächliche Haltererkennung bleibt maßgeblich.
- Jeder Bildscan verwendet einen eigenen Scannerprozess. Zwischen Batchbildern liegt eine 30-Sekunden-Bereitschaftspause.
- Unterstützte Profile: RGB8 bei 600 oder 2.820 dpi.
- RAW-, TIFF-, Parameter- und Clientlog-Artefakte werden geprüft und können in einen gewählten Ausgabeordner exportiert werden.
- Responsiver Filmstreifen-Workflow mit Live-Inspector für Scannerstatus, Bildfolge, Bytefortschritt, Prozessereignisse und Fehler.
- Keine automatischen Wiederholungen fehlgeschlagener Scans.
- Für die Negativentwicklung sind weiterhin externe Werkzeuge wie GIMP oder NegPy vorgesehen.

## Bekannte Einschränkungen

- Dies ist eine private, lokal ad-hoc-signierte Anwendung. Es gibt keine Apple Developer ID-Signierung und keine Notarisierung.
- Die strikte Deep-Signaturprüfung des vollständigen `.app`-Bundles schlägt mit der bekannten Meldung `code has no resources but signature indicates they must be present` fehl. Diese Warnung ist im RC4-Abschlussbericht festgehalten.
- Ein vollständiger D6-GUI-Simulatorlauf steht nicht zur Verfügung; die automatisierten Prozess- und Rust-Regressionstests decken die hardwarefreie Batchlogik ab.
- `CaptureKind::Production` bleibt gesperrt.
- Die historischen Hardwareberichte dokumentieren Scanner-/Clientabläufe, Logs und Parameter. Sie belegen keinen unabhängigen vollständigen RAW/TIFF-Pixelvergleich für alle Hardware-Captures.
- Das private RC-Bundle enthält den bekannten harmlosen D3-B4-`beblog-test-sidecar` zusätzlich zum Minolta-Client. Seine Mitlieferung wurde für diesen RC ausdrücklich akzeptiert und im RC4-Bericht dokumentiert.
- Der hardwarebestätigte Batchumfang ist vier Bilder; Position 6 ist nicht durch die dokumentierten D5-Hardwareläufe bestätigt.

## Begleitende Nachweise

- [RC4 Final Build und lokales Artefakt](BUILD-001-RC4-FINAL-BUILD.md)
- [D5-HW3 Hardware-Meilenstein](BUILD-001-D5-HW3-HARDWARE-PASS.md)
- [D6-HW1 Hardware-Meilenstein](BUILD-001-D6-HW1-HARDWARE-PASS.md)

Die RC4-Buildprüfung basiert auf Commit `4a40c438120e9c0070439cd4a5c371abe0828938`. Der Release-Tag wurde zum Zeitpunkt dieser Notizen noch nicht erstellt.
