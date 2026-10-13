# Build 001-D3-B3c — Secure Process Evidence & Execution Identity

## Baseline audit

**Base:** `main` at `08ccf01f74a9292800f51e561f8aea76315819be` (Merge PR #9). The existing D3-B3b branch was clean before switching to the updated `main`; this feature branch was created from the clean merge base.

- **PASS — `os_process_runner.rs`:** Compiled only under `#[cfg(test)]` via `process.rs`. It starts only the current Rust test harness with enum-selected, fixed libtest filters/arguments, no shell and no UI-provided executable or args. It manages `Child`, separate readers, timeout/cancel, TERM→KILL, wait/reaping and cleanup. Resolver-derived test executable identity was only recorded before spawn; by itself this was not proof that an immutable file object was executed.
- **PASS — `process.rs`:** `ProcessRunner` is a general interface; `RecordedProcessRunner` emits synthetic events and has fault injection. `ProcessEvent`/`ProcessOutcome` therefore cannot alone establish OS facts. The latter has no PID, OS handle, wait result, or reader-thread join token.
- **PASS — `adapter.rs`:** `ProcessLifecycle` checks event ordering, streams, reaping marker, terminal status and cleanup. It consumes events from any `ProcessRunner`, including `RecordedProcessRunner`, so a valid lifecycle is protocol validation—not an unforgeable OS capability.
- **PASS — `sidecar.rs`:** A crate-private fixed-resource resolver checks expected architecture/name, regular-file/executable mode, symlinks and a caller-provided SHA-256. It returns a path-based result. There is no production runner consuming it; verifying a path/hash and later passing that path to `Command::spawn()` leaves a replacement window.
- **PASS — `offline_import.rs`:** Explicit offline file paths are copied into exclusive app staging under fixed archive names. The importer checks profile metadata, client log, TIFF structure/pixel equality and staged/archive sizes and SHA-256. It reports offline evidence only; it is not connected to a process outcome.
- **PASS — `archive.rs`:** `ArchiveManager::finalize()` currently accepts only `CaptureKind::Simulation`; Production finalization returns `PermissionDenied`. Offline import uses a separate typed kind and cannot be upgraded by the importer.
- **PASS — `core.rs`:** The UI-facing application core always uses `FakeScannerAdapter`; current Tauri commands expose the fake single/batch paths and cancellation only. There is no real-client command, adapter selection or sidecar resource packaging.

## Implementation

### Runner-owned ephemeral evidence

`ProcessEvidence` and its constructor live privately inside `os_process_runner.rs`, which itself remains test-only. The type is neither `Clone` nor serde serializable/deserializable, has private fields, and is not accepted by `ArchiveManager` or any Tauri command. Only the runner's private `run_with_evidence()` can return it. `ProcessLifecycle`, `RecordedProcessRunner`, adapter events, and log parsing have no API to construct or supply it.

For each invocation, the runner creates a fresh UUID run identifier and records the OS child PID and test-executable SHA-256 observation. It creates success evidence only after all of these runner-local facts hold:

- `Command::spawn()` returned a `Child` and the runner captured its PID;
- `Child::wait()` itself returned an exit status with numeric code 0 (the runner does not rely only on a prior poll or client log);
- both stream reader threads received actual EOF/closed-pipe completion messages (a reader error or synthetic fallback close does not qualify);
- both reader threads joined without panic;
- `wait()` succeeded, so the child was reaped;
- cleanup accumulated no process, reader, wait, or resource errors;
- there was no timeout, cancellation, or forced kill.

Rust documents that `Child::try_wait()` returning `Some(status)` reaps the PID on Unix; this runner additionally requires successful `Child::wait()` for its proof. `wait()` is documented to wait for complete exit and return the exit status. See [Rust `Child` documentation](https://doc.rust-lang.org/std/process/struct.Child.html). A wait error always prevents cleanup success and proof.

The proof is scoped to that UUID/PID/executable digest tuple and is ephemeral. In this build, only the harmless test-runner harness produces it. The test harness resolver calculates and records its own digest for test wiring; it is not a trusted production digest and is not presented as such.

The evidence means only successful OS process completion and OS resource cleanup. It does **not** prove SANE `sane_close()`/`sane_exit()` succeeded, that the scanner returned to a safe hardware state, or that any image artifact is complete or valid.

### Execution identity and macOS boundary

The sidecar resolver and test runner do not use a shell or PATH search. The real Minolta sidecar is not packaged, resolved, or launched. Current D3-B3c process evidence is test-only and therefore cannot be used to start or authorize a production process.

On this build machine, `xcrun --show-sdk-path` selected `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`. Its `unistd.h` declares path-based `execve()`; `spawn.h` declares path-based `posix_spawn()`. No `fexecve()` or `execveat()` declaration was found in those headers. Apple's documented `posix_spawn()` signature likewise accepts an executable `path`, while `posix_spawnp()` performs PATH lookup when no slash is supplied. See [Apple's execve man page](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/execve.2.html) and [Apple's posix_spawn man page](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/posix_spawn.2.html).

**OPEN / promising macOS design, not implemented:** This SDK also declares Apple's `POSIX_SPAWN_START_SUSPENDED` extension. Apple documents that it creates the child suspended before user-space execution. Apple's Security framework can identify guest code by PID with `SecCodeCopyGuestWithAttributes`/`kSecGuestAttributePid` and return signing information. A future candidate is therefore: `posix_spawn` the fixed bundle path suspended; inspect the actual suspended guest's dynamic signing identity/CDHash against a pinned signer/hash; kill it on any ambiguity or validation failure; send `SIGCONT` only after success. Apple notes that guest-attribute support is host-specific and can fail for unsupported combinations, so the Security API's exact behavior for this app/child relationship, code object, hash and supported macOS versions needs a harmless-process proof before relying on it. References: [Apple POSIX_SPAWN_START_SUSPENDED documentation](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/posix_spawnattr_getflags.3.html), [SecCodeCopyGuestWithAttributes](https://developer.apple.com/documentation/security/seccodecopyguestwithattributes%28_%3A_%3A_%3A_%3A%29), [guest PID attribute](https://developer.apple.com/documentation/security/ksecguestattributepid), and [signing information](https://developer.apple.com/documentation/security/seccodecopysigninginformation%28_%3A_%3A_%3A%29).

Rehashing immediately before `Command::spawn()` would still leave a check-to-use window and is not an acceptable proof. The suspended-spawn + live-code-signature approach is **not implemented or validated in D3-B3c**. Until a harmless test establishes that the returned SecCode object represents the exact loaded main executable and its pinned identity before any user-space instruction runs, the real sidecar spawn boundary remains BLOCKED. The existing Rust `Command` runner does not expose suspended spawn or live SecCode identity.

## Archive boundary

The Production guard was not changed. Even a future valid process proof is insufficient by itself. Before a capture can be Production, one transaction must bind its process run ID to its exclusive output reservation and frame, validate the four fixed artifacts and metadata/log claims, verify RAW/TIFF pixel identity and profile consistency, archive originals without overwrite, and verify on-disk byte sizes and SHA-256 before writing Completed. `OfflineImport` and `Simulation` must remain separate classifications. The current process evidence is not passed to archive code.

## Regression coverage

- Complete allowlisted test-harness process returns an ephemeral proof with a unique run ID, PID, executable digest and exit code 0.
- A second successful run has a distinct run ID.
- Nonzero exit, spawn failure, timeout, pre-start cancellation, and in-flight cancellation return no proof.
- Internal proof-policy cases reject missing stream EOF, reader/cleanup errors, and forced kill.
- Existing OS-runner tests cover ignored SIGTERM, forced SIGKILL, closed pipes while the child remains alive, wait/reap and cleanup order.
- Existing adapter tests reject invalid or incomplete synthetic event sequences; they cannot produce the private runner proof.
- Deterministic OS syscall cleanup failure and actual OS reader read failure are not injected; policy rejection is tested through private negative observations. These remain test-coverage limitations, not success paths.
- Full existing fake, four-frame batch, parser, archive, offline-import and recovery suites remain enabled.

## Changed files

- `src-tauri/src/os_process_runner.rs`: runner-private ephemeral proof and success/failure policy tests using only fixed test-harness modes.
- `docs/BUILD-001-D3-B3C-AUDIT.md`: baseline, proof contract, macOS execution-identity investigation, open limits and verification record.

## Security confirmation and open risks

- No Minolta client, scanner process, SANE, scanner hardware, Homebrew/avision, Research-005/009 artifacts or gamma configuration was used, changed, rebuilt or packaged.
- No new Tauri command, UI selection or production adapter was added. `FakeScannerAdapter` remains the only UI-reachable adapter.
- **BLOCKER:** No atomically verified binary-to-executed-image binding is demonstrated on macOS with the current Rust/path-based spawn mechanism.
- **OPEN:** macOS offers a possible suspended-spawn plus live guest-code signature verification route; its exact behavior and required API support remain untested.
- **BLOCKER:** Process proof is test-only and cannot authorize Production. Production also remains gated on complete artifact/archive validation.
- **OPEN:** Decide whether Apple code signing/notarization and a read-only bundled resource can provide an acceptable platform trust boundary, and independently prove it against replacement attacks.
- **OPEN:** Determine a supported macOS mechanism for descriptor-bound execution, or formally accept and mitigate the residual pathname race. Do not substitute a second hash check as proof.
- **OPEN:** Add deterministic OS-level failure injection for reader/reap/cleanup failures if a suitable safe seam can be built without weakening the runner.
- **OPEN:** D3-B3d or later should integrate a complete capture transaction only after independent review of execution identity and the process-to-artifact binding.

## D3-B3c verification

`cargo test --manifest-path src-tauri/Cargo.toml` PASS (88 passed, 12 intentionally ignored test-harness child entry points); `cargo fmt --manifest-path src-tauri/Cargo.toml --check` PASS; `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` PASS; `pnpm check` PASS; `pnpm test` PASS (7/7); `pnpm build` PASS; `pnpm tauri build --bundles app` PASS (native macOS `.app`); `git diff --check` PASS.

## D3-B3c.1 correction

The private evidence constructor now requires both the runner's complete OS observation and the final `ProcessOutcome`. It produces evidence only when that final outcome is explicitly `Exited(0)`, in addition to the existing successful wait/reap, stdout/stderr EOF, joined readers, cleanup, no-timeout, no-cancellation, no-forced-kill, and no-unresolved-error conditions. Regression coverage keeps the observation otherwise fully successful while supplying `Exited(7)` and `UnexpectedEnd`; neither can produce evidence. This does not alter the process runner's event policy or any production/archive gate.

D3-B3c.1 verification: `cargo test --manifest-path src-tauri/Cargo.toml` PASS (88 passed, 12 ignored); `cargo fmt --manifest-path src-tauri/Cargo.toml --check` PASS; `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` PASS; `pnpm check` PASS; `pnpm test` PASS (7/7); `pnpm build` PASS; `pnpm tauri build --bundles app` PASS (native macOS `.app`); `git diff --check` PASS. All execution was hardware-free; no scanner, SANE, Homebrew, or Research artifacts were accessed.
