# Build 001-D7-C-RC2-Fix1 – Batch-Abbruch Regressionstest

Datum: 15. Oktober 2026

## Ausgangszustand und Audit

- Basis: lokaler `main`, `origin/main` und GitHub-`main` jeweils `e0bdc846fa2521243acd194d79e82a2bc72be342`.
- Featurebranch: `test/001-d7-c-rc2-cancel-regression`, erstellt von dieser bestätigten Basis.
- Geschützte ungetrackte Dateien blieben unverändert und ungetrackt: `tools/minolta-hardware-test.sha256` (SHA-256 `1a03d2d6e7d7fd38452b982e019fae140e2f3bacb7efd63f157b7a3d7ea5a8ee`), `docs/BUILD-001-D7-RC1-ACCEPTANCE.md` (SHA-256 `55369c23c7795ea9079d9fe471983af83ff15caa4798a26cf0f682f655b62447`) und `docs/BUILD-001-D7-C-RC2-ACCEPTANCE.md` (SHA-256 `2e9c8e90aab3e96e55ae6f176af241b51be17a1c06c250e2824a379977525687`).
- Read-only-Befund in `src/App.svelte`: Positionen bis einschließlich `completedFrames` werden zuerst als fertig abgebildet; anschließend markiert `currentFrame` zusammen mit `phase === "cancelled"` die abgebrochene Position. Die Batchmeldung zeigt `completedFrames` von `totalFrames`. Die Erfolgsmeldung ist ausschließlich an `phase === "completed"` und vollständige Framezahl gebunden.

## Regressionstest

In `src/App.test.ts` wurde ein gezielter Test mit `totalFrames=4`, `completedFrames=2`, `currentFrame=3`, `phase="cancelled"` ergänzt. Er prüft:

- Bild 1 und 2: „Fertig“.
- Bild 3: „Abgebrochen“.
- Bild 4: „Wartet“.
- Abbruchmeldung: zwei von vier Bildern fertig.
- Keine Erfolgsmeldung „4 von 4 Bildern digitalisiert“.
- Kein automatischer Aufruf von `start_d6_batch`.

Der Test besteht mit dem vorhandenen Anwendungscode. Es wurde kein Produktcode geändert und kein Ereignis an einen Scannerpfad gesendet.

## Prüfungen

| Prüfung | Ergebnis |
|---|---|
| `pnpm check` | PASS |
| `pnpm test -- --run` | PASS — 4 Testdateien, 22 Tests |
| `pnpm build` | PASS |
| `cargo test d6 -- --test-threads=1` | PASS — 10 D6-Unit-Tests; zusätzlich 1 gefilterter D6-Prozess-E2E-Test |
| `cargo test --test guided_process_e2e -- --test-threads=1` | PASS — 3 Prozess-E2E-Tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `git diff --check` | PASS |

Ein nativer Tauri-Neubuild war nicht erforderlich: Es gab keine Anwendungs- oder Buildsystemänderung.

## Geänderte Dateien und Einschränkungen

- `src/App.test.ts` — neuer Frontend-Regressionstest.
- `docs/BUILD-001-D7-C-RC2-FIX1.md` — dieser Prüfbericht.

Cancel-Projektion und Sicherung vorher abgeschlossener Captures sind damit direkt im Frontend regressionstest-abgedeckt. Hardwarepfad, Batch-GUI-Simulationsablauf und reale Scannerzustände wurden nicht ausgeführt oder bewertet.

## Empfehlung

**PASS für diesen gezielten RC2-Fix.** Der Test bestätigt den erwarteten Cancel-Zustand und schützt gegen automatische Folgestarts. Die unabhängige RC-Prüfung bleibt davon unberührt; dieser Bericht erteilt keine Hardwarefreigabe.
