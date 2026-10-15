# BeBlog Digitizer – Build 001-RC4 Final Build

Prüfdatum: 15. Oktober 2026

Releaseversion: `0.1.0-rc.1`

Git-Basis: `4a40c438120e9c0070439cd4a5c371abe0828938` (`main`, identisch mit `origin/main` und GitHub-`main`).

## Git- und Versionierungsstand

- `main`, `origin/main` und GitHub-`main` wurden nach kontrolliertem Fast-Forward auf den erwarteten Commit abgeglichen.
- Projektversion ist in `package.json`, `src-tauri/Cargo.toml`, dem lokalen Crate-Eintrag in `src-tauri/Cargo.lock` und `src-tauri/tauri.conf.json` jeweils `0.1.0-rc.1`.
- Die geschützte Datei `tools/minolta-hardware-test.sha256` blieb ungetrackt und unverändert; SHA-256 vor/nach: `1a03d2d6e7d7fd38452b982e019fae140e2f3bacb7efd63f157b7a3d7ea5a8ee`.
- Kein Commit, Push, Tag, PR, Merge oder GitHub-Release wurde für RC4 erstellt.

## Regressionstests

| Prüfung | Ergebnis |
|---|---|
| `pnpm check` | PASS |
| `pnpm test -- --run` | PASS — 4 Dateien, 22 Tests |
| `pnpm build` | PASS — Vite-Produktionsbuild |
| `cargo test --locked -- --test-threads=1` | PASS — 133 bestanden, 0 fehlgeschlagen, 12 absichtlich ignoriert; zusätzlich 3 Prozess-E2E-Tests bestanden |
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `pnpm tauri build --bundles app` | PASS |

## Native App und Client

- App-Bundle: `/Users/bernd/BeBlog-Digitizer/src-tauri/target/release/bundle/macos/BeBlog Digitizer.app`
- Die App wurde gestartet; der sichtbare Startzustand war „Bereit“ mit Filmstreifenansicht. Keine Scannererkennung, SANE-Abfrage oder Scanaktion wurde ausgelöst.
- App-Executable: Mach-O ARM64; Bundle-`CFBundleShortVersionString`: `0.1.0-rc.1`.
- Gebündelter Minolta-Client: ARM64; SHA-256 im Quellressourcenpfad und im Bundle jeweils `d591832bf08e09aaa2758634fa86f0c7138bb63fcb801350743637f58ce2e404` — stimmt mit dem festgelegten Pin überein.
- `codesign --verify --strict` für den gebündelten Client: PASS (`valid on disk`, Designated Requirement erfüllt).
- `otool -L` zeigt unverändert `/opt/homebrew/opt/sane-backends/lib/libsane.1.dylib` sowie `/usr/lib/libSystem.B.dylib`. Es wurde keine SANE-Bibliothek verändert oder aufgerufen.

## Signatur-WARN

Die read-only Prüfung `codesign --verify --deep --strict` für die gesamte App endete mit Status 1 und der bekannten Meldung:

`code has no resources but signature indicates they must be present`

Keine Signatur oder Ressource wurde deswegen geändert.

## Akzeptierte RC-Einschränkung: Test-Sidecar

Die Mitlieferung des bekannten `beblog-test-sidecar` im privaten RC-ZIP wurde ausdrücklich freigegeben. Das vorhandene `.app`-Bundle wurde nicht verändert.

Read-only-Abgleich:

- Im App-Bundle liegen genau vier reguläre Dateien: `Contents/Info.plist`, `Contents/MacOS/beblog-digitizer`, `Contents/Resources/sidecars/macos-aarch64/beblog-test-sidecar` und `Contents/Resources/sidecars/macos-aarch64/minolta-batch-frame`.
- Der Test-Sidecar-Hash lautet `09dc4296d9f347fccd2e4fe8aa62a7e39160a04be6a652e2b79adbb0de360729`. Er stimmt mit dem fest hinterlegten D3-B4-Hash in `src-tauri/src/sidecar.rs` und dem D3-B4-Audit überein. Die Datei ist ein ARM64-Mach-O; `codesign --verify --strict` besteht. Sie wurde bei dieser Prüfung nicht ausgeführt.
- Das D3-B4-Audit beschreibt den Sidecar als dediziertes harmloses Testprogramm ohne Scanner-, SANE-, USB- oder Netzwerkcode. Seine Mitlieferung ist eine bekannte private RC-Einschränkung, kein Produktions-Scanpfad.
- Das Bundle enthält keine separaten Logs, Fixture-Dateien oder Nutzerdateien. Die Bundle-Metadaten enthalten nur die App-Kennung, den Produktnamen und die Releaseversion. Eine gezielte Suche in den Bundledateien fand keine gängigen Muster für private Schlüssel, API-Tokens oder E-Mail-Adressen. Das ist eine begrenzte Inhaltsprüfung, keine allgemeine Malware- oder Datenschutzbescheinigung.

## ZIP-Artefakt

- Datei: `/Users/bernd/BeBlog-Digitizer-RC4-artifacts/BeBlog-Digitizer_0.1.0-rc.1_macos-arm64.zip`
- Größe: `3,188,196` Bytes
- SHA-256: `0833be4da63ceb49aab527ea23f83d9e9682bde4da2a13f81f62963aa944f82e`
- Erstellt mit macOS `ditto -c -k --sequesterRsrc --keepParent` aus dem vorhandenen `.app`-Bundle.
- ZIP-Inhalt: ein einziger Top-Level-Ordner `BeBlog Digitizer.app`; darin App-Executable, `Info.plist` und die beiden erwarteten ARM64-Sidecars. Keine weiteren Dateien.
- ZIP wurde in ein temporäres Prüfverzeichnis entpackt; SHA-256 aller vier regulären Dateien stimmt jeweils mit dem `.app`-Quellbundle überein, und der Verzeichnisvergleich besteht. Das App-Bundle blieb unverändert.
- ZIP und Prüfbericht bleiben lokal. Kein Commit, Push, Tag oder GitHub-Release wurde erstellt.

## Ergebnis

**CONDITIONAL PASS** für die lokale RC-Paketierung unter der ausdrücklich akzeptierten Einschränkung, dass das bekannte D3-B4-Test-Sidecar im privaten RC-Bundle enthalten ist. Build, automatisierte Tests, ARM64-Architektur, App-Version, Minolta-Clientintegrität sowie ZIP-Struktur und Hash bestehen. Die bekannte Deep-Signatur-WARN bleibt bestehen. Keine Hardware- oder SANE-Aktion fand statt.
