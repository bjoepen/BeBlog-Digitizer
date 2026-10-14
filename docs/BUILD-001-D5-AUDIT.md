# Build 001-D5 audit

## Ergebnis

Der D5-Einzelbildpfad unterstützt die Positionen 1–6 in der Anforderungsvalidierung, UI, Simulator, Metadaten und angepasst vorliegender Clientquelle. Die vorhandene freigegebene Client-Ressource bleibt unverändert und akzeptiert in der produktiven App weiterhin nur Positionen 1–4. Der neu gebaute ARM64-Kandidat für 1–6 ist nicht freigegeben. Es gab keine Scanner-, SANE-Geräteerkennungs- oder Hardwareausführung.

## Ausgangsbasis und Änderungen

- Featurebranch: `build/001-d5-scanner-discovery-six-frames`.
- Ausgangscommit: `a7147895b9b5ad2360405da242105def70b9bedb` (D4-HW2). Der Branch wurde von dieser ausdrücklich benannten Basis weitergeführt; `main` wurde weder gemergt noch umgeschrieben.
- Die ungetrackte Datei `tools/minolta-hardware-test.sha256` blieb unangetastet und wird nicht versioniert.
- `src-tauri/src/scanner_discovery.rs` ergänzt typisierte Zustände für kein/ein/mehrere passende Geräte und eine explizit aufgerufene macOS-Erkennung über die installierte SANE-Bibliothek. Sie ruft `sane_init`, `sane_get_devices(local_only=1)` und anschließend `sane_exit` auf; sie ruft weder `sane_open` noch Scanoperationen auf. Einträge außerhalb der sicheren Grenze von 256 werden abgewiesen.
- `AppCore` serialisiert Geräteerkennung gegen Scanstarts und weist Erkennung während eines aktiven Laufs oder Cooldowns ab. Tauri stellt Erkennung und Cooldown-Abfrage bereit. Die Svelte-Oberfläche startet Erkennung nur nach dem ausdrücklichen Klick, zeigt Null-/Einzel-/Mehrfachtreffer, erlaubt Auswahl und separate Bestätigung und stellt die Gerätekennung nur im Diagnosebereich dar.
- Einzelbildscan und Offline-Import akzeptieren Frame 1–6. D1-Batchumfang bleibt unverändert 1–4. Die UI bietet alle sechs Positionen an, blockiert aber reale Position 5–6 mit einer sichtbaren Meldung, weil der freigegebene D4-Client diese Positionen nicht unterstützt.
- Jeder manuelle Scan startet weiterhin einen neuen Clientprozess. Erfolg setzt vollständigen Streamabschluss, Exitstatus, Reaping, Cleanup, Protokollabschluss, Artefaktvalidierung und Export voraus. Nach einem realen Prozesslauf wird ein Core-seitiger 30-Sekunden-Cooldown erzwungen. Er beginnt konservativ erst nach Prozessabschluss und der Ergebnisverarbeitung. Vor jedem neuen Scan verlangt die App wieder den bisherigen `OPENED` → expliziter manueller `CONTINUE`-Klick → echte `HOLDER_DETECTED`-Ablauf; ein eingesetzter Halter wird nicht vorausgesetzt.
- C-Client-Arbeitskopie erweitert `-f` auf 1–6 und verwendet den bestehenden robusten Parser. Ein irreführender Diagnosehinweis, der den Halter im Batch als eingesetzt voraussetzte, wurde durch einen Einzelprozess-/Handshake-Hinweis ersetzt. Die unveränderte Quellaufnahme wird weiterhin gegen SHA-256 `35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae` geprüft.
- `tools/build-minolta-arm64-client.sh` erlaubt einen dedizierten D5-Kandidatenpfad. Es ersetzt weder die gepackte App-Ressource noch `tools/minolta-hardware-test.sha256`.

## Client-Kandidat

Die geänderte Arbeitskopie wurde für ARM64 mit der nativen Apple-Clang-/macOS-SDK-Toolchain und den vorhandenen SANE-Headern/-Bibliotheken kompiliert. Der Build schreibt nach `src-tauri/target/minolta-d5-frame6-candidate-revision-2/minolta-batch-frame`, signiert ad hoc und gibt danach den Hash aus.

- SHA-256 nach Signierung: `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404`
- Geprüft: dünnes ARM64 Mach-O, gültige Ad-hoc-Signatur, dynamische Referenz auf `/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib`.
- Der Kandidat wurde nicht gestartet, nicht in die App-Ressourcen kopiert und nicht als Vertrauensanker hinterlegt. Er ist nicht hardwarebestätigt oder zur realen Ausführung freigegeben.
- Ein vorheriger, von uns erstellter Zwischenkandidat liegt unter `src-tauri/target/minolta-d5-frame6-candidate/`; er wurde ebenfalls nicht gestartet oder freigegeben und nicht überschrieben.

## Prüfergebnisse

- **PASS** Rust: `cargo test -- --test-threads=1`; 113 bestanden, 0 fehlgeschlagen, 12 absichtlich ignorierte Kindprozess-Harnessfälle; Integrationstests einschließlich zwei nacheinander gestarteter Kindprozesse, je frischer OPENED-/CONTINUE-Handshake, getrennter Capture-Archive und validierter Exporte.
- **PASS** `cargo fmt --check`.
- **PASS** `cargo clippy --all-targets -- -D warnings`.
- **PASS** `pnpm check` und `pnpm test`; 16 Frontendtests.
- **PASS** `pnpm build`.
- **PASS** C-Profilvertragstest einschließlich 1/6 gültig sowie 0/7 und fehlerhafte Zahlen abgewiesen.
- **PASS** ARM64-Kompilierung, `lipo`, `file`, `codesign --verify` und Hashprüfung des Kandidaten.
- **PASS** `pnpm tauri build --bundles app`; natives App-Bundle erstellt. Der ausführbare App-Hauptprozess ist ein dünnes ARM64 Mach-O. Enthalten sind der bestehende Test-Sidecar und weiterhin der D4-Minolta-Client mit Hash `9049bb63b6a6a81ef1d686172f063d3949939e3acca7786fc3a414cb21d36aa6`.
- **PASS** `git diff --check`.
- **NICHT AUSGEFÜHRT** `detect_minolta_scanners`; UI-/Klassifikationsfälle sind ausschließlich mit simulierten Geräten getestet.

## Grenzen und offene Hardwarefreigaben

- Die SANE-Erkennung wurde weder gegen ein Gerät noch gegen die installierte Laufzeitbibliothek aufgerufen. Funktion, exakter SANE-Gerätebezeichner und Darstellung am realen Scanner sind daher **nicht hardwareverifiziert**. Die Laufzeiterkennung ist macOS-spezifisch und erwartet die vorhandene Bibliothek am fest verdrahteten Homebrew-Ressourcenpfad.
- Die vom SANE-Backend gemeldete Menge der Halterpositionen 1–6 war eine vorherige Hardwarebeobachtung. D5 ändert nicht die Backendoptionen oder Gamma-Konfiguration.
- Der App-Bundle-Client mit Hash `9049bb63b6a6a81ef1d686172f063d3949939e3acca7786fc3a414cb21d36aa6` bleibt unverändert; Positionen 5–6 sind damit real gesperrt. Die D5-Änderung erzeugt einen neuen Hash, der unabhängige Prüfung und ausdrückliche Freigabe benötigt, bevor er anstelle des D4-Clients paketiert oder ausgeführt werden kann.
- Der D4-Client kann über `-f` nur 1–4; darum sind reale Scans auf diese vier Positionen beschränkt, bis der neue Kandidat separat freigegeben und paketiert wird.
- Die Tauri-Bundlerstellung war erfolgreich. Eine zusätzliche manuelle `codesign --verify --strict`-Prüfung des erzeugten App-Bundles meldet `code has no resources but signature indicates they must be present`; das Bundle ist vom Build ad-hoc/linker-signiert, aber diese zusätzliche Bundle-Verifikation ist daher **WARN**, kein nachgewiesener Signatur-PASS. Sidecar-eigene Signaturprüfung und Hashprüfung des Kandidaten bestanden.
- Cooldown ist im Prozessspeicher und pro App-Sitzung erzwungen; ein Neustart der Anwendung setzt den Timer zurück. Er ist keine persistente geräteweite Sperre.
- `CaptureKind::Production` bleibt gesperrt. Die aktuellen Capture-Manifeste bewahren die konservative Klassifizierung bei.
- Die D5-Simulation beweist Prozess-/Handshake-/Archivlogik, nicht den Verbleib oder die erneute Erkennung des Filmhalters zwischen realen Scans. Jeder Lauf fragt erneut nach dem manuellen Einsetzen und wartet erneut auf die Scannerbestätigung.

## Status

`BLOCKED` für die vollständige D5-Hardwarefreigabe: Die automatische Erkennung ist noch nicht an echter Hardware geprüft, und Positionen 5–6 benötigen eine getrennte Freigabe des neuen Clienthashs sowie die anschließende Paketierung. Die hardwarefreie D5-Implementierung selbst ist getestet; reale Scans und Scannererkennung wurden nicht ausgeführt.
