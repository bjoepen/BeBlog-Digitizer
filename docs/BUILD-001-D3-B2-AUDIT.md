# Build 001-D3-B2 – Rust Process Runner Audit

## Ausgangsbasis und Umfang

- **Basis:** `main` nach PR #6, Mergecommit `278a66ed3185916d2a6e5dafaeab965939ce295a`.
- **Branch:** `build/001-d3-b2-process-runner`.
- **Integration:** Der neue OS-Runner ist in diesem Build nur unter `cfg(test)` verfügbar und wird nicht in den produktiven Scannerpfad eingebunden.
- **Adapter:** `AppCore` aktiviert weiterhin ausschließlich `FakeScannerAdapter`; `RecordedProcessRunner` bleibt die Quelle für D2-/D3-B1-Simulationen.
- **Hardware:** Es wurden nur untergeordnete Prozesse des Rust-Test-Harness mit fest kodierten, harmlosen Modi gestartet. Kein Scannerclient, SANE-Prozess, Research-Client oder Hardware wurde angesprochen.

## Auditbefund vor der Änderung

`ProcessRunner` war eine synchrone Abstraktion mit `RecordedProcessRunner`; der Recorded Runner verwendet weder `std::process` noch eine Shell. `ProcessEvent` enthielt bereits Start, getrennte stdout-/stderr-Zeilen, Streamabschluss, Exit, Cancel, Timeout und Cleanup. `FakeScannerAdapter` validiert diese Ereignisfolge mit `ProcessLifecycle`: beide Streams müssen vor dem normalen Exit geschlossen sein, ein Terminalereignis muss zum zurückgegebenen `ProcessOutcome` passen und ein gestarteter Prozess benötigt ein erfolgreiches Cleanup-Ereignis.

Der Core verwendet einen exklusiven Jobslot und aktiviert den Fake-Adapter fest. Es gab keinen OS-Prozessrunner. Prozess-ID, echte parallele Streamdrainage, vollständige Child-Reaping-Semantik sowie OS-seitiger Timeout-/Signalpfad waren damit noch nicht abgedeckt.

## Implementierung und Sicherheitsmechanismen

Die neue interne `OsProcessRunner`-Implementierung liegt in `src-tauri/src/os_process_runner.rs` und wird ausschließlich aus Tests kompiliert. Sie ist kein `ScannerAdapter` und kann nicht über Tauri oder die UI aufgerufen werden.

- Der ausführbare Pfad ist fest auf `current_exe()` (das eigene Rust-Testbinary) gesetzt. Prozessargumente wählen ausschließlich einen genau benannten, ignorierten Child-Test aus; der Child-Modus ist damit durch den typisierten Allowlist-Eintrag vorgegeben. Es gibt keine umgebungsvariablegesteuerte Auswahl, keinen Shell-Aufruf und keine öffentliche API für beliebige Pfade.
- Eine private `SafeTestProgram`-Allowlist stellt ausschließlich fest kodierte Testmodi bereit. Der kontrollierte Startfehler verwendet einen reservierten, nicht vorhandenen Sentinel-Pfad und wird vor dem Start abgelehnt, falls dieser wider Erwarten existiert.
- Auf Unix erhält der Testprozess eine eigene Prozessgruppe. Cancel/Timeout sendet SIGTERM an diese Gruppe, wartet höchstens 250 ms und sendet danach nötigenfalls SIGKILL. `ForcedKill` kennzeichnet diesen Fall separat; er ist kein regulärer Scannerabschluss.
- Beide Pipes werden gleichzeitig durch eigene Reader-Threads geleert. Ein begrenzter synchroner Kanal (32 Ereignisse) verhindert ungebremstes Anstauen im Runner. Zeilen werden bei 16 KiB begrenzt und mit einem sichtbaren Trunkierungsmarker ausgegeben. Ungültiges UTF-8 wird verlustbehaftet durch U+FFFD dargestellt; eine letzte Zeile ohne Newline bleibt erhalten.
- Pro Runnerprozess wird eine globale Sperre bis zum Ende von `wait()` und Reader-Join gehalten. Ein wartender Aufruf startet erst nach vollständigem Ende des vorherigen Laufs; ein Cancel, das vor dem Start gesetzt wurde, wird nach Erhalt der Sperre erneut geprüft.
- Ein normaler `Exited`-Abschluss wird erst nach Beobachtung beider Stream-EOFs und anschließendem `Child::wait()` ausgegeben. `CleanedUp` folgt nur nach erfolgreichem Warten/Reaping und Join beider Reader. Fehler dabei werden als `CleanupFailed` ausgegeben und verhindern Erfolg.
- OS-Signalterminierung ohne numerischen Exitcode wird als `UnexpectedEnd` mit dokumentiertem Signalhinweis abgebildet, nicht als regulärer Exitcode. Bei Timeout/Cancel bleiben `TimedOut` bzw. `Cancelled` die Terminalereignisse; ein nötiger harter Kill ist zusätzlich durch `ForcedKill` erkennbar.

Die D2- und D3-B1-Verträge der produktiven Fake-Simulation, des Parsers und der Archivierung bleiben erhalten. Die Erweiterung von `Started` um eine optionale PID lässt aufgezeichnete Events mit `pid: None` unverändert semantisch darstellbar.

## Prozessereignisvertrag

| Phase | Ereignisfolge |
|---|---|
| Abbruch vor Spawn | `Cancelled`; keine PID, kein Cleanup-Ereignis |
| Spawnfehler | `StartFailed`; keine gestartete Child-Ressource |
| Erfolgreicher Spawn | `Started { program, pid: Some(..) }` |
| Laufzeit | beliebig viele `Stdout`-/`Stderr`-Zeilen aus getrennten Pipes |
| Pipe-Ende | `StdoutClosed` und `StderrClosed`, jeweils einmal |
| OS-Abschluss/Reaping | `Reaped { code }` nach erfolgreichem `wait()` und nach beiden Pipe-Enden; `None` kennzeichnet ein Signalende ohne numerischen Status |
| reguläres Ende | `Exited { code }` nach `Reaped` mit passendem Exitcode |
| Cancel/Timeout | optional `ForcedKill`, danach genau ein `Cancelled` oder `TimedOut` |
| Ressourcenfreigabe | `CleanedUp` nach erfolgreichem Wait/Reap und Reader-Join; andernfalls `CleanupFailed` |

Ausgaben werden auch während der Beendigungsphase weiter geleert. Der Runner startet keine Retries. Ein Fehlerereignis wird nicht durch spätere Diagnosezeilen in einen Erfolg umgewandelt.

## Testmatrix

Die neuen Tests starten nur das eigene Rust-Testbinary über die interne Allowlist.

| Szenario | Ergebnis |
|---|---|
| Startfehler über reservierten, nicht vorhandenen Testpfad | `StartFailed`, kein Spawn |
| Exit 0 und Exit 7 | PID/Exitstatus bzw. Nichtnullstatus beobachtet |
| unerwarteter Selbstabbruch durch Signal | `UnexpectedEnd` mit Signalursache, danach Cleanup |
| parallele stdout/stderr-Ausgabe und 128 Zeilen je Stream | ohne Deadlock vollständig drainiert |
| stille Ausführung und vorzeitiges Schließen beider Pipes | Exit/Streamende korrekt erkannt |
| ungültiges UTF-8, letzte Zeile ohne Newline | Ersatzzeichen bzw. letzte Zeile erhalten |
| Zeile oberhalb 16 KiB | abgeschnitten und explizit markiert |
| Cancel vor Start | kein `Started`-Ereignis |
| Cancel nach installiertem SIGTERM-Handler | kontrollierter SIGTERM-Pfad, kein erzwungener Kill |
| Timeout bei SIGTERM ignorierendem Testprozess | Grace-Period, `ForcedKill`, Cleanup/Reaping |
| konkurrierende Runneraufrufe | durch Runner-Sperre serialisiert |
| PID nach Cleanup | Prozess-ID ist nach Reaping nicht mehr im OS-Prozesstabellenzugriff auffindbar |
| Exit ohne Reaped-Nachweis | `FakeScannerAdapter` lehnt die unvollständige Lifecycle-Folge ab |

Die vorhandenen Fake-, Einzel-Scan-, Batch-, Recovery-, Archiv-, D2-Lifecycle- und D3-B1-Parser-Fixturetests laufen weiterhin.

## Qualitätssicherung

| Prüfung | Ergebnis |
|---|---|
| `cargo test` | PASS – 69 Tests bestanden, 11 absichtlich ignorierte Child-Entry-Points |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `pnpm check` | PASS – TypeScript |
| `pnpm test` | PASS – 3 Dateien, 7 Tests |
| `pnpm build` | PASS – Vite Produktionsbuild |
| `pnpm tauri build --bundles app` | PASS – native macOS `.app` erzeugt |

## Einschränkungen und offene Punkte für D3-B3

- **Bewusste Grenze:** Der Runner ist in regulären App-Builds nicht verfügbar. D3-B3 benötigt eine separate, überprüfbare Produktions-Allowlist und typisierte Prozesskonfiguration, bevor der C-Client überhaupt startbar wird.
- Es wurde kein echter Minolta-Client gestartet. Das reale CLI-, stderr-, RAW-/TIFF- und SANE-Cleanup-Verhalten ist durch diese Änderung nicht neu verifiziert.
- Fehler von `wait`, `kill`, Reader-Thread-Join und Stream-I/O sind defensiv behandelt; die seltenen OS-Fehlerpfade lassen sich in den Tests nicht zuverlässig deterministisch erzwingen. Gezielte Fault-Injection für diese Betriebssystemfehler bleibt OPEN.
- Das Testprofil auf macOS/Unix verwendet Prozessgruppen und SIGTERM/SIGKILL. Das nicht-Unix-Fallback kann mit `std::process` nur den Childprozess direkt beenden und ist nicht als Nachweis einer späteren Windows-Prozessbaumstrategie zu verstehen.
- Die Event-Callback-Funktion wird synchron auf dem Runnerthread aufgerufen. Ein dauerhaft blockierender Consumer kann damit auch die Beobachtung von Timeout/Cancel verzögern; der spätere Integrationsvertrag muss nicht-blockierende Eventzustellung sicherstellen.
- Gesamte Ausgabe wird zeilenweise gestreamt, nicht dauerhaft im Runner gepuffert. Einzelzeilen sind begrenzt; Speicherverantwortung für eine optionale spätere Archivierung der vollständigen Logs liegt beim Consumer.
- Signalnummern werden derzeit als Diagnose in `UnexpectedEnd` transportiert, nicht als eigenes strukturiertes Feld. Falls D3-B3 sie maschinell auswerten muss, sollte `ProcessOutcome` einen typisierten Exitstatus enthalten.

## Sicherheitsbestätigung

`FakeScannerAdapter` blieb der einzige aktive Scanneradapter. Der neue Runner wird nicht aus der App gestartet und ist nur im Testbuild vorhanden. Keine SANE-Kommandos, Scannerclients, Research- oder Gamma-Artefakte, Homebrew-Komponenten oder Scannerhardware wurden verändert oder angesprochen.
