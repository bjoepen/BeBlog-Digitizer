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

## D5-HW2 Addendum

Dieses Addendum dokumentiert den nachfolgenden HW2-Integrationsauftrag; obige Abschnitte bewahren den historischen D5-Ausgangsstand.

- **D5-HW1, Nutzerbericht:** Scanner „Minolta DiMAGE Scan Dual II“ wurde in der nativen GUI erkannt und ausgewählt; technische Kennung `avision:libusb:002:006`; kein Scan gestartet. Ergebnis laut Nutzer `PASS`.
- **Client-Reprüfung:** Vor Integration erneut verifiziert: Kandidat SHA-256 `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404`, thin ARM64 Mach-O, gültige Ad-hoc-Signatur, dynamische Referenz auf `/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib`, CLI-Fehlermeldung bestätigt gültige Frames 1–6. Das Kandidatenprogramm wurde nicht ausgeführt.
- **Integration:** Kandidat liegt unter dem festen Ressourcenpfad `src-tauri/sidecars/macos-aarch64/minolta-batch-frame`; der fixe Resolver-Trust-Anchor ist auf exakt denselben SHA-256 gesetzt. Es gibt keinen Hash-Fallback oder dynamisches Vertrauen. Der D4-Client wurde ersetzt.
- **UI-Korrektur:** Erfolgreiche Gerätebestätigung zeigt nun einen Bestätigungsstatus statt der widersprüchlichen Aufforderung zur erneuten Auswahl. Die reale manuelle Einzelbildauswahl 1–6 ist freigegeben; Batch bleibt bei 1–4.
- **Erhaltene Ablaufbedingungen:** Ein eigener Clientprozess pro Einzelbild, `OPENED` → manueller Klick → `HOLDER_DETECTED`, tatsächliche Parameter/EOF, Prozessabschluss und Reaping, Artefaktvalidierung/Export sowie Core-erzwungener Cooldown von 30 Sekunden. Keine automatische Wiederholung. Jeder nächste Lauf beginnt erneut mit dem bestehenden Holder-Handshake; Holder-Wiederverwendung wird nicht angenommen.
- **Sicherheitsstatus:** App-/Codeintegration und hardwarefreie Tests bestanden. Der reale D5-Client wurde nicht gestartet; SANE-Geräteerkennung wurde in dieser Phase nicht erneut aufgerufen. `CaptureKind::Production` bleibt unverändert gesperrt. Erst nachdem der Nutzer den ersten D5-Scan separat genehmigt und ausgeführt hat, kann ein Hardware-PASS für den neuen Client behauptet werden.

## D5-HW2 Abschluss

- **Git:** Ausgangs-HEAD und Remote-Featurebranch stimmten auf `8ca4368ea8f8d417f942697125d10bc0730617de` überein; `origin/main` blieb `4d88b4cb5ed46ebc8d827e12fc4984f2d2d02fe4`. Kein Pull, Merge oder Reset wurde vorgenommen. `tools/minolta-hardware-test.sha256` blieb ungetrackt und unberührt.
- **Fester Client:** `src-tauri/sidecars/macos-aarch64/minolta-batch-frame`, exact SHA-256 `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404`; der Resolver vergleicht ausschließlich diesen fest eingetragenen Wert. Die integrierte Datei und die Datei im finalen App-Bundle stimmen bytegenau per SHA-256 überein.
- **Framefreigabe:** reale Einzelbildscans der Positionen 1–6 werden von Request, CLI und UI akzeptiert. Batch bleibt 1–4; es gibt keine automatische Framefolge.
- **Prüfungen:** Rust 114 bestanden, 0 fehlgeschlagen, 12 absichtlich ignorierte Harnessfälle; Prozess-E2E 2 bestanden; Frontendtests 17 bestanden; `cargo fmt --check`, Clippy `-D warnings`, TypeScript, Vite-Build und `git diff --check` bestanden. Der native `pnpm tauri build --bundles app` erstellte ein ARM64-Bundle.
- **Paketprüfung:** Das Bundle enthält den D5-Client mit dem erwarteten Hash, thin ARM64-Architektur und gültiger Ad-hoc-Signatur. `otool -L` zeigt die erwartete installierte SANE-Bibliothek. Der Client wurde in dieser Phase nicht ausgeführt. Der zusätzliche strikte `codesign --verify`-Aufruf für das `.app`-Bundle meldet weiterhin `code has no resources but signature indicates they must be present`; das ist eine dokumentierte Bundle-Warnung, obwohl der Tauri-Build selbst erfolgreich ist.
- **Regressionen:** Geräteerkennung ohne Autostart, No/One/Multiple-Zustände, ausdrückliche Auswahlbestätigung ohne widersprüchlichen Folgehinweis, alle Frames 1–6, fester Hash und Manipulationsablehnung sowie zwei getrennte Simulatorprozesse mit erneuter OPENED-/CONTINUE-Sequenz und separaten Capture-Exports sind abgedeckt.
- **Nicht ausgeführt:** keine Scannererkennung durch Codex, kein Clientstart, kein Scan, keine SANE-Aufrufe durch Codex.
- **Abschlussstatus:** `READY FOR D5-HW3 HARDWARE APPROVAL`. Vor einem realen Scan bleibt die ausdrückliche separate Hardwarefreigabe erforderlich. `CaptureKind::Production` bleibt gesperrt; erfolgreicher Lauf bedeutet daher nicht automatisch Produktionsklassifizierung.
