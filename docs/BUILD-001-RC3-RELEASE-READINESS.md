# BeBlog Digitizer v0.1.0-rc.1 – Release Readiness

Stand: 15. Oktober 2026

Ausgangscommit: `17ff0d7fb0d4b78a835618adf65796670cdbb91e`

## Überblick

Dieser RC bereitet die private, lokale macOS-ARM64-Anwendung zur unabhängigen Release-Prüfung vor. Es wurden keine neuen Funktionen oder Scannerexperimente begonnen. Projektversionen wurden konsistent auf `0.1.0-rc.1` gesetzt.

## D4–D7-C-Meilensteine

- **D4:** geführter Scannerworkflow, `OPENED`/manuelle Halterbestätigung/`CONTINUE`/tatsächliche Haltererkennung, Prozess- und Artefaktintegration sowie nativer hardwarefreier Simulatorablauf. Kein gesonderter D4-Hardware-PASS wird hier behauptet.
- **D5:** automatische Scannererkennung wurde in D5-HW1 über die App mit dem Minolta DiMAGE Scan Dual II bestätigt. D5-HW3 dokumentiert drei manuelle reale Einzelbildläufe (Positionen 3, 5, 4) mit 2.820 dpi, RGB8, 2.664 × 3.996 und gemeldeten 31.936.032 RAW-Bytes je Lauf. Dafür lagen Logs und Parameterdateien vor; die zugehörigen RAW-/TIFF-Dateien wurden nicht unabhängig auf Pixelidentität geprüft. Position 6 ist softwareseitig verfügbar, aber durch diesen Beleg nicht hardwarebestätigt.
- **D6:** D6-HW1 dokumentiert einen realen GUI-geführten Batch der Positionen 1–4 vom 14.10.2026. Gemeldet wurden 2.820 dpi RGB8, 2.664 × 3.996 und 31.936.032 RAW-Bytes pro Frame. Jeder Frame lief in einem eigenen Prozess; die Berichte nennen EOF, TIFF-Erstellung, `sane_close`, `sane_exit`, Exitcode 0 und Cooldowns von ungefähr 30 Sekunden. Die vier Clientlogs und Parameterdateien wurden ausgewertet. Die tatsächlichen RAW-/TIFF-Dateien wurden nicht unabhängig byteweise verglichen; Batchjournal und Exportmanifeste wurden nicht unabhängig geprüft. Die erfolgreiche reale Viererfolge ist bestätigt, nicht darüber hinausgehende Archiv- oder Pixelnachweise.
- **D7-C-RC2:** native ARM64-App visuell bei breiter, mittlerer und schmaler Fenstergröße geprüft; hardwarefreier Einzelbildsimulator bis validiertem Export bestanden. Der RC2-Bericht kennzeichnete die fehlende dedizierte Frontendprüfung der Batch-Cancel-Projektion als WARN und den fehlenden D6-GUI-Simulator als Einschränkung.
- **D7-C-RC2-Fix1:** ergänzte Frontend-Regression prüft Cancel auf Frame 3 bei zwei abgeschlossenen Frames: frühere Bilder bleiben fertig, aktueller Frame abgebrochen, Folgeframe wartet, keine Gesamterfolgsmeldung und kein automatischer Neustart. 22 Frontendtests bestanden. Dies behebt die konkrete RC2-Test-WARN; der fehlende GUI-D6-Simulator bleibt eine Einschränkung.

Die lokalen historischen Dokumente `BUILD-001-D7-RC1-ACCEPTANCE.md` und `BUILD-001-D7-C-RC2-ACCEPTANCE.md` werden unverändert versioniert. Ihre damaligen Beobachtungen und Warnungen bleiben historisch; D7-C-RC2-Fix1 ergänzt den späteren Testnachweis, ohne sie umzuschreiben.

## Aktueller Funktionsumfang

- Ausschließlich native macOS-Apple-Silicon-ARM64-Anwendung für privaten lokalen Betrieb.
- Lokale Ad-hoc-Signierung; keine Apple Developer ID und keine Notarisierung.
- Automatische Scannererkennung nur auf manuelle Anforderung und mit ausdrücklicher Geräteauswahlbestätigung.
- Einzelbildmodus mit Filmpositionen 1–6.
- Filmstreifenmodus mit 2–6 aufeinanderfolgenden Positionen `1..N`, Standard vier.
- RGB8 bei 600 oder 2.820 dpi.
- Jeder reale Frame verwendet einen separaten Clientprozess und eine eigene SANE-Verbindung; zwischen Batchframes gilt eine Core-seitige Bereitschaftspause von 30 Sekunden.
- Explizite Halterbestätigung beim ersten Frame; Folgeframes verwenden nach erneutem `OPENED` die bestehende automatische Fortsetzung und verlangen weiterhin die tatsächliche Haltererkennung.
- Validierte RAW-, TIFF-, Parameter- und Clientlog-Artefakte werden exportiert; keine automatischen Wiederholungen fehlgeschlagener Scans.
- Negativentwicklung und Farbgestaltung bleiben externe Arbeitsschritte, beispielsweise mit GIMP oder NegPy.

## Bekannte Einschränkungen und WARNs

- Kein vollständiger hardwarefreier D6-Batchlauf über die native GUI: der D6-Ablauf wird durch Rust- und Prozess-E2E-Tests abgedeckt, die GUI bietet keinen Batchsimulator.
- D5- und D6-Hardwareberichte beruhen auf Clientlogs und Parameterdateien; sie beanspruchen keine unabhängige RAW/TIFF-Pixelgleichheit. Beim D6-Test wurden Journal und Exportmanifeste nicht unabhängig auditiert.
- `CaptureKind::Production` bleibt gesperrt. Validierte reale Captures behalten die vorhandene konservative Archivklassifizierung.
- `codesign --verify --deep --strict` auf der vollständigen `.app` schlägt mit der bekannten Meldung `code has no resources but signature indicates they must be present` fehl (Exitstatus 1). Der App-Build selbst gelingt; die App ist lokal ad-hoc-signiert, ohne Team-ID. Diese RC-Arbeit ändert die Signatur nicht und führt keine neue Signierungsuntersuchung durch.
- `git diff --cached --check` meldet ausschließlich den unveränderten Markdown-Zeilenumbruch aus zwei abschließenden Leerzeichen in Zeile 3 von `BUILD-001-D7-C-RC2-ACCEPTANCE.md`. Diese historische Datei wurde bytegenau übernommen; die Ausnahme für genau diese Zeile ist für RC3 ausdrücklich genehmigt. Whitespace-Prüfung der übrigen gestagten Änderungen besteht.
- Der Minolta-Client ist ARM64 und seine Ad-hoc-Signatur wird lokal als gültig geprüft. Seine dynamische SANE-Abhängigkeit verweist auf die bestehende lokale Installation unter `/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib`; diese Umgebung wurde nicht verändert.

## Versionsvorbereitung

| Datei | Version | Behandlung |
|---|---|---|
| `package.json` | `0.1.0-rc.1` | geändert |
| `src-tauri/Cargo.toml` | `0.1.0-rc.1` | geändert |
| `src-tauri/Cargo.lock` lokaler `beblog-digitizer`-Eintrag | `0.1.0-rc.1` | synchronisiert, keine Dependency-Upgrades |
| `src-tauri/tauri.conf.json` | `0.1.0-rc.1` | geändert |
| `pnpm-lock.yaml` | keine Root-Projektversion vorhanden | unverändert |

## Validierungsmatrix

| Prüfung | Ergebnis |
|---|---|
| `pnpm check` | PASS — TypeScript ohne Fehler |
| `pnpm test -- --run` | PASS — 4 Dateien, 22 Tests |
| `pnpm build` | PASS — Vite Produktionsbuild |
| `cargo test --locked -- --test-threads=1` | PASS — 133 bestanden, 0 fehlgeschlagen, 12 absichtlich ignoriert; 3 Prozess-E2E-Tests bestanden |
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `pnpm tauri build --bundles app` | PASS — native App erzeugt |
| App-Bundle-Architektur | PASS — ARM64 |
| gepinnter Client im Quellbaum und App-Bundle | PASS — beide SHA-256 `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404`; ARM64 |
| Client Ad-hoc-Signatur | PASS — `codesign --verify --strict` erfolgreich |
| vollständige `.app` Deep-Signaturprüfung | WARN — bekannte Ressourcen-Meldung oben, Exitstatus 1 |
| `git diff --check` | PASS |

Es wurden keine Scannererkennung, SANE-Geräteabfrage, Minolta-Clientausführung oder Hardwaretests durchgeführt. SANE/avision, `/opt/homebrew`, Research-009, Gamma, Clientquelltext, Scannerprotokoll, D6-Orchestrator, Archivierungs- und Exportlogik sowie der SHA-256-Pin blieben unverändert.

## Release-Empfehlung

**CONDITIONAL PASS für einen privaten v0.1.0-rc.1-Prüfkandidaten.** Die Softwaretestmatrix und der native ARM64-Build bestehen; die RC2-Cancel-Testlücke ist geschlossen. Vor einer endgültigen RC-Freigabe ist die bekannte strikte `.app`-Signaturwarnung durch die unabhängige Freigabeinstanz zu bewerten. Dieses Dokument ist keine Hardwarefreigabe, kein öffentlicher Distributionsnachweis und keine Freigabe von `CaptureKind::Production`.
