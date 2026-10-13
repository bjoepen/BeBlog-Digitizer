# Build 001-D3-B5c – ARM64 Client Build & Gesamt-Timeout

**Basis:** `main` / Merge von PR #17 `47d91b61aabb47a3d42f28d7e172c775a3bc4f36`
**Branch:** `build/001-d3-b5c-arm64-client-timeout`

## Ausgangslage und Sicherheitsstatus

- **PROVEN:** Ausgangsbranch war `main`, Arbeitsverzeichnis sauber. Research-005-Original und vendorter Snapshot haben weiterhin SHA-256 `35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae`.
- **PROVEN:** `minolta-batch-frame.original.c` blieb unverändert. Kompiliert wurden nur die angepasste Arbeitskopie und `minolta_profile.c`.
- **PROVEN:** Der Minolta-Client wurde ausschließlich kompiliert, signiert und statisch inspiziert. Er wurde nicht gestartet. Kein Scanner-, SANE-Geräte-, UI-, Research-005/009- oder Backendzugriff fand statt.
- **PROVEN:** Tauri registriert weiterhin keinen Real-Client-Befehl; `FakeScannerAdapter` bleibt der aktive Adapter und `CaptureKind::Production` ist weiterhin gesperrt.
- **PROVEN:** `tools/minolta-hardware-test.sha256` wurde weder angelegt noch verändert. Der Build gibt den Hash nur auf stdout aus. Der Launcher startet ohne diese unabhängig zu erstellende Pin-Datei nicht.

## Lokale Buildumgebung (read-only erhoben)

| Komponente | Befund |
| --- | --- |
| Host / Ziel | macOS 26.6.2, `arm64`; Clang target `arm64-apple-darwin25.6.0` |
| Toolchain / SDK | Apple Clang 21.0.0 über Command Line Tools `/Library/Developer/CommandLineTools`; SDK `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk` |
| SANE headers | `/opt/homebrew/Cellar/sane-backends/1.4.0_2/include/sane/{sane.h,saneopts.h}` (über `/opt/homebrew/include/sane`) |
| SANE library | `/opt/homebrew/Cellar/sane-backends/1.4.0_2/lib/libsane.1.dylib`, Mach-O ARM64 |
| libusb dependency | `/opt/homebrew/Cellar/libusb/1.0.29/lib/libusb-1.0.0.dylib`, Mach-O ARM64 |
| pkg-config metadata | `sane-backends.pc` vorhanden, Version `1.4.0`; `pkg-config` CLI fehlt |

Die `.pc`-Datei wurde nur gelesen. Ihr Include-/Library-Vertrag entspricht den fest in `tools/build-minolta-arm64-client.sh` gesetzten Pfaden. Keine Datei unter `/opt/homebrew` wurde geändert. Die Library-Install-ID verweist auf `/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib`; das Binary verwendet damit die installierte `opt`-Referenz. Ein späteres Backend-Upgrade kann diese Referenz ändern; die Bibliothek wird nicht in die App kopiert.

## ARM64-Client-Build

`tools/build-minolta-arm64-client.sh` verwendet ausschließlich:

1. `src-tauri/vendor/minolta-batch-frame/minolta-batch-frame.c`
2. `src-tauri/vendor/minolta-batch-frame/minolta_profile.c`

Der Build prüft den Research-005-Snapshot-Hash, Hostarchitektur, Header, SANE-/libusb-Architektur und fehlende Zielüberschreibung. Er nutzt explizit `/opt/homebrew/include`, `/opt/homebrew/opt/sane-backends/lib`, das aktuelle Command-Line-Tools-SDK und `-arch arm64`. `-Wno-unused-function` unterdrückt ausschließlich die aus der unveränderten Originalquelle übernommene, derzeit unbenutzte `prompt_exact()`-Funktion; übrige `-Wall -Wextra`-Warnungen sind mit `-Werror` Fehler.

Reproduzierbarer Aufruf:

```sh
tools/build-minolta-arm64-client.sh
```

Der Ablauf linkt, prüft die Architektur, ad-hoc-signiert, verifiziert die Signatur und **danach** berechnet/zeigt er SHA-256. Er schreibt niemals die Hash-Freigabedatei und führt das Binary an keiner Stelle aus. Ziel: `src-tauri/target/minolta-hardware-test/minolta-batch-frame`; das Verzeichnis enthält nach dem Build ausschließlich dieses Client-Buildartefakt.

**Ergebnis: PASS.** `clang -fsyntax-only` mit ARM64-SDK und vorhandenen SANE-Headern bestand. Der vollständige Build bestand. `file` meldet Mach-O 64-bit executable arm64; `lipo -archs` meldet ausschließlich `arm64`. `codesign --verify --strict` bestand; Signaturdetails melden `Signature=adhoc`, `TeamIdentifier=not set`. Die Help-/Usage-Beispieladresse der angepassten Arbeitskopie wurde auf einen ausdrücklich zu bestätigenden Platzhalter geändert, damit keine alte USB-Adresse als Geräteempfehlung erscheint; der Original-Snapshot blieb unverändert.

`otool -L` des fertigen Clients:

```text
/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib
/usr/lib/libSystem.B.dylib
```

Die separat inspizierte `libsane.1.dylib` lädt außerdem libusb, `libSystem`, `libxml2`, CoreFoundation und IOKit. Diese sind vorhandene Runtime-Abhängigkeiten; der Clientbuild hat sie nicht kopiert oder verändert.

Vollständiger SHA-256 des nach Ad-hoc-Signierung geprüften Binaries:

```text
d106f68ed849a2294661e7b2ca97323de601263049bb6d5f83c19fdac6dc3701
```

Dies ist nur ein **Buildnachweis**, keine Freigabe. Insbesondere wurde kein Hash in `tools/minolta-hardware-test.sha256` übernommen. Ein zuvor erstelltes, veraltetes Buildartefakt wurde zur Vermeidung einer destruktiven Löschung außerhalb des Repositories unter `/tmp/beblog-minolta-client-rebuild.S7ywlSnz/` aufbewahrt; es ist nicht Teil dieses Buildnachweises.

## Gesamt-Timeout und Abbruch

`tools/run-minolta-hardware-test.sh` bindet nun `tools/process-timeout.sh` ein. Der getrennte Gesamt-Timeout ist fest auf **1.200 Sekunden (20 Minuten)** gesetzt; nach SIGTERM wartet der Wächter **15 Sekunden** auf kooperatives Ende. Der bestehende `-t 120` bleibt ausschließlich Holder-Polling und begrenzt nicht den Scan.

Der Wächter startet genau einen fest aufgelösten Clientpfad mit einem Argumentvektor (keine Shell-Kommandozeichenkette), prüft als direkte Elternprozessinstanz dessen Zustand und wartet anschließend auf `wait`/Reaping. Bei Timeout oder äußerem SIGINT/SIGTERM sendet er zunächst SIGTERM, wartet die Grace-Period, sendet nur bei weiterhin lebendem Kind SIGKILL und wartet danach weiterhin auf den tatsächlichen Prozessstatus. Timeout-/Abbruchursache, Shell-`wait`-Status, TERM/KILL-Versand und Artefaktzustand werden separat protokolliert. Timeout liefert Launcherstatus 124, Abbruch 130/143; Status 0 allein markiert keinen Capture als abgeschlossen.

Nach jedem Lauf werden ausschließlich die festen Suffixe `.client.log`, `.raw`, `.params.txt`, `.tiff` auf reguläre Nicht-Symlink-Dateien geprüft; Größe und SHA-256 werden erfasst. Das Skript validiert keine TIFF-/RAW-Inhalte und klassifiziert selbst bei vorhandenem Dateisatz nur `PENDING_OFFLINE_ARTIFACT_VALIDATION`. Timeout, Abbruch, Nichtnullstatus oder fehlende Artefakte ergeben einen unvollständigen Capture. Es gibt weder Retry noch Resume.

Der vorhandene C-Client fängt SIGTERM kooperativ ab: Bei aktivem Scan sollte sein Hauptthread `sane_cancel()` versuchen, anschließend `sane_close()` und `sane_exit()` aufrufen. Der Wächter wertet dies nicht als bewiesenes SANE-Cleanup. SIGKILL, Signalzustand und Cleanup-Status werden nicht als sauberer Scan dargestellt. Die Signal- und Bereinigungsaufrufe des Clients wurden nur aus Quelltext abgeleitet; der Client wurde nicht ausgeführt.

Die 20-Minuten-Frist wird im separaten Launcher vor der einzigen Clientinvokation angezeigt und erfordert danach eine ausdrückliche Bestätigung. Der Shell-Wächter kann SIGINT/SIGTERM behandeln, nicht aber SIGKILL oder einen abrupten Systemausfall; in diesem Fall kann er keinen geordneten Kindprozess-Cleanup garantieren. Dieses Restrisiko ist dokumentiert und verhindert eine automatische Capture-Freigabe.

### Hardwarefreie Timeout-Regressionen

`tools/tests/test-process-timeout.sh` kompiliert und verwendet ausschließlich `tools/tests/timeout-probe.c`, ein harmloses Testprogramm ohne SANE-, USB- oder Netzwerkcode. Nachgewiesen wurden:

- regulärer Exit 0 und fehlerhafter Exit 7;
- Timeout mit kooperativem SIGTERM, ohne SIGKILL;
- ignoriertes SIGTERM, SIGKILL-Eskalation, `wait`-Status 137 und Reaping;
- äußerer SIGTERM-Abbruch des manuellen Ablaufs;
- genau ein Prozessstart (Markerdatei), fehlender Executablepfad ohne Spawn;
- fehlende Artefakte und Symlink-Ablehnung ohne Validitätsfreigabe.

Alle Prozessmodi laufen mit kurzen Testtimeouts; es gibt keine 20-minütige Wartezeit in Tests. Der Minolta-Client ist weder Testprogramm noch Testfixture.

## Qualitätssicherung

| Prüfung | Ergebnis |
| --- | --- |
| Client-/Profil-C-Syntaxcheck, ARM64, `-Werror` (nur unbenutztes Original-`prompt_exact` ausgenommen) | PASS |
| C-Profil-/CLI-Vertragstest | PASS |
| ARM64-Clientbuild, Ad-hoc-Signatur, `lipo`, `file`, `otool`, finaler SHA-256 | PASS |
| Shellsyntax für Build-, Timeout-, Launcher- und Testskripte | PASS |
| Hardwarefreie Timeoutregressionen | PASS |
| `cargo fmt --check` | PASS |
| `cargo test -- --test-threads=1` | PASS: 97 bestanden, 0 fehlgeschlagen, 12 ignorierte Child-Modi |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `pnpm check` | PASS |
| `pnpm test` | PASS: 3 Testdateien, 7 Tests |
| `pnpm build` | PASS |
| `pnpm tauri build --bundles app` | PASS; separates App-Binary ARM64 |
| `git diff --check` | PASS |

Der `.app`-Build enthält unverändert die Test-Sidecar-Ressource und keinen Minolta-Client. Der Client liegt nur separat im ignorierten `src-tauri/target`-Buildordner.

## Offene Punkte / Hardwarefreigabe

- **OPEN:** SHA-256 unabhängig prüfen und separat freigeben; die Pin-Datei fehlt derzeit absichtlich.
- **OPEN:** Aktuellen SANE-Devicebezeichner vor Ort explizit bestätigen; keine alte USB-Adresse übernehmen.
- **OPEN:** Ein Hash vor `exec` ist eine pfadbasierte Prüfung und lässt ein enges Austauschfenster zwischen letzter Prüfung und Ausführung. Das Skript verengt es durch eine zweite Hashprüfung nach Benutzerbestätigung, beseitigt es aber nicht. Nur im kontrollierten lokalen Ablauf nach unabhängiger Prüfung verwenden.
- **OPEN:** Wächter überwacht und terminiert den direkten Clientprozess. Sollte ein künftiger Client zusätzliche Prozesse abspalten, ist deren Lifecycle hiermit nicht belegt.
- **OPEN:** Timeout, Dateipräsenz und Clientlog beweisen weder SANE-Cleanup noch valide TIFF-/RAW-Bilddaten. Der Offline-Validator und die tatsächlichen Scanparameter/Pixel müssen nach einem separat freigegebenen Test unabhängig ausgewertet werden.
- **OPEN:** 2820-dpi-Profil bleibt bis zu tatsächlichen SANE-Parametern und validierten Artefakten **nicht hardwarebestätigt**.

Vor einem ersten Hardwaretest sind ein unabhängiges Audit dieses Commits, separate Freigabe des Binaryhashs, aktuelle bestätigte Gerätekennung, neuer exklusiver Zielordner und explizite einmalige Freigabe erforderlich. Es wurde kein Hardwaretest ausgeführt.
