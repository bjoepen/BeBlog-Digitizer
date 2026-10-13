# Build 001-D3-B3d — macOS Execution Identity Research

**Mode:** research only; no production integration.

**Base:** `main` after D3-B3c.1, merge `56d80819850e0dfef8fd8f7b74e43bb9ed33c420`.

**Host used:** macOS 26.6.2, Apple Silicon (arm64), Command Line Tools SDK `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`.

## Summary

**PROVEN (this host, this harness):** `posix_spawn()` with Apple’s `POSIX_SPAWN_START_SUSPENDED` created the harmless child in a state where its first user-space marker write had not happened. Looking up the suspended PID as a dynamic Security.framework `SecCode`, running `SecCodeCheckValidity()`, and comparing its SHA-256 CodeDirectory CDHash with an independently pinned preflight CDHash accepted the intended ad-hoc-signed helper. The marker appeared only after `SIGCONT`.

**PROVEN (negative scenarios):** Wrong expected hash, a replacement before spawn, replacement after spawn but before Security validation, changed signed code, invalid PID lookup, an already-ended child, and an unsigned helper did not pass the gate. Denied suspended children were sent `SIGKILL` and synchronously reaped with `waitpid()`; their marker remained absent. After-spawn replacement returned `errSecCSStaticCodeChanged` (`-67034`, “the code on disk does not match what is running”); the changed-code case returned `errSecCSGuestInvalid` (`-67063`, “code identity has been invalidated”).

**SUPPORTED, NOT FULLY PROVEN:** Apple documents the dynamic object as running code, `kSecGuestAttributePid` as a PID selector, and dynamic validity as secure against attempts to modify the filesystem source of the code object. This makes suspended spawn + PID guest lookup + validity checking a credible macOS path to bind trust to a running code identity. The test demonstrates the behavior on one current ARM64 host and the tested ad-hoc signature; it is not a formal guarantee across all macOS releases, architectures, launch contexts, or security configurations.

**BLOCKED for production today:** BeBlog’s resolver still returns a path after file/hash checks, and the OS process runner remains test-only. The PoC is not integrated, and neither this experiment nor an OS-process proof establishes SANE cleanup or valid/archived scan artifacts. No production adapter, Tauri command, sidecar bundle, or Production archive path was enabled.

## Phase 1 — Existing implementation audit

- **PASS — `src-tauri/src/sidecar.rs:1-4,55-63,113-145`:** fixed-resource resolver, no process start API, no PATH or UI executable selection. It rejects symlinked components and hashes the opened regular file, checking device/inode against the listed path. Its result remains path-based: later execution would reopen by pathname, so the resolver alone does not bind the inspected file object to the executed image.
- **PASS — `src-tauri/src/os_process_runner.rs:1-4,204-230`:** compiled only as a unit-test module; executes only the current test harness with fixed enum-selected modes. It is not a real-sidecar runner. `ProcessEvidence` is private and test-only (`:25-65`), and requires successful wait/exit 0, both stream EOFs, joined readers, reap and cleanup, with no timeout, cancellation, forced kill or unresolved error (`:523-541`).
- **PASS — `src-tauri/src/process.rs:10-15,17-69`:** events/outcomes are protocol values; they do not contain an OS handle or unforgeable process-identity capability. `RecordedProcessRunner` is explicitly synthetic (`:72-73`).
- **PASS — `src-tauri/src/adapter.rs:57-85,109-127`:** `ProcessLifecycle` validates ordering and terminal consistency, but receives generic runner events. It proves protocol conformance, not that an OS child or SANE lifecycle actually existed.
- **PASS — `src-tauri/src/archive.rs:316-328,375-398`:** production finalization is explicitly denied pending a future typed process proof; the archive validates data but is not authorized by this research PoC.

## Phase 2 — Apple API and SDK findings

### Suspended spawn

`posix_spawnattr_setflags()` accepts the Darwin extension `POSIX_SPAWN_START_SUSPENDED`. [Apple’s archived man page](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/posix_spawnattr_getflags.3.html) says the child’s task is suspended, permitting inspection before it begins execution in user space. The installed SDK defines it as `0x0080` in `usr/include/sys/spawn.h`. The experiment confirmed this operationally: before approval the helper had not created its marker; after `SIGCONT`, it did.

This is a path-based spawn API. Suspension prevents the helper’s user-space code from running before inspection, but by itself it does not pin an inode or eliminate a path replacement race.

### Dynamic and static code objects

Apple describes [`SecCode`](https://developer.apple.com/documentation/security/seccode) as an object for running code and [`SecStaticCode`](https://developer.apple.com/documentation/security/secstaticcode) as an object for code on disk. `SecStaticCodeCreateWithPath()` does not intrinsically link a disk object to a running process. [`SecCodeCopyGuestWithAttributes`](https://developer.apple.com/documentation/security/seccodecopyguestwithattributes%28_%3A_%3A_%3A_%3A%29) with [`kSecGuestAttributePid`](https://developer.apple.com/documentation/security/guest-attribute-dictionary-keys) asks the system code-signing host to identify running guest code by PID. Apple warns that supported guest attribute combinations depend on the host; failures must deny execution.

[`SecCodeCheckValidity()`](https://developer.apple.com/documentation/security/seccodecheckvalidity%28_%3A_%3A_%3A%29) dynamically validates the `SecCode`, its host and any supplied requirement. Apple explicitly documents this check as secure against attempts to modify the filesystem source of that code object. Apple also documents that some signing information for a dynamic object is obtained from disk, and recommends validity checking before using it. The PoC follows that order: PID lookup → dynamic validation → retrieve selected SHA-256 CDHash → exact comparison. The API accepts a `SecRequirement` for an additional publisher/code identity predicate, but the PoC passes `NULL` and pins the exact CDHash itself; a Team ID/designated-requirement policy is not exercised here.

The CodeDirectory value used here is the conventional **20-byte SHA-256 CDHash** exposed by [`kSecCodeInfoCdHashes`](https://developer.apple.com/documentation/security/kseccodeinfocdhashes) paired with `kSecCodeInfoDigestAlgorithm == SHA-256` (not a 32-byte full digest). The SDK’s generic [`kSecCodeInfoUnique`](https://developer.apple.com/documentation/security/kseccodeinfounique) documentation says it is a stable identifier for a static code version; the PoC does not treat `kSecCodeInfoUnique` alone as proof of the process image. Nor does it use the undocumented `cdhashes-full` key observed in a diagnostic dictionary dump.

An ad-hoc signature supplies code integrity/CDHash but no signer identity or certificate ([Apple documents ad-hoc signatures as lacking a signing identity](https://developer.apple.com/documentation/security/seccodesignatureflags/adhoc)). A production trust policy should pin a stable signer/team requirement and/or an exact approved CDHash, and must define updates and revocation. `SecCodeCheckValidity()` without an explicit requirement establishes validity, not that the signer is the intended publisher.

### Scenarios and interpretation

The standalone sources are in `research/d3-b3d/`. `identity_poc.c` only launches the paths passed by its test script, which creates its own tiny signed and unsigned helper variants. It is not built into BeBlog. The final script run completed in `/var/folders/.../beblog-d3-b3d.kW1iz2` (macOS temporary directory; test products retained there).

| Scenario | Result | Evidence |
|---|---|---|
| Valid ad-hoc-signed child | **PASS** | Matching live CDHash; marker absent while suspended, present after continue; exit 0 and reaped. |
| Wrong trusted hash | **PASS** | Hash mismatch; SIGKILL + `waitpid`; marker absent. |
| Replacement before spawn | **PASS** | The alternate helper’s live CDHash differed from the preflight trust value; denied and reaped. |
| Replacement after spawn, before identity check | **PASS** | `SecCodeCheckValidity` returned `errSecCSStaticCodeChanged`; denied and reaped. |
| Signed code modified before spawn | **PASS** | Dynamic validation returned `errSecCSGuestInvalid`; denied and reaped. |
| Unsigned helper | **PASS** | Guest PID lookup worked, but there was no trusted signed SHA-256 CDHash to match; denied and reaped. |
| Invalid PID attribute | **PASS** | Guest lookup failed (`OSStatus=100003`); suspended child was independently killed and reaped. |
| Child dies before identity lookup | **PASS** | Child was killed/reaped; subsequent guest lookup did not authorize it. |
| Spawn of nonexistent fixed test path | **PASS** | `posix_spawn()` returned `ENOENT`; no child was created. |
| Continue-path abort | **PASS, limited** | After successful identity validation, the harness injected an abort before issuing `SIGCONT`, killed/reaped the child, and observed no marker. This validates the fail-closed branch, not an OS-level `SIGCONT` failure. |
| Certificate-backed signature | **OPEN** | `security find-identity -v -p codesigning` reported zero valid signing identities on this host. No keychain identity was created. |
| Publisher/designated requirement | **OPEN** | `SecCodeCheckValidity()` accepts a requirement, but only exact CDHash comparison was exercised. |
| Forced `SIGCONT` syscall failure | **OPEN** | Not induced. Signalling a PID after reaping could hit a reused PID, so the harness intentionally does not perform that unsafe test. |
| `SIGKILL` or `waitpid` failure | **OPEN** | No deterministic, safe OS-level fault injection was available. The PoC checks/report failures, but successful cleanup tests do not prove failure handling. |
| File replacement after validation but before resume | **OPEN** | The requested pre-validation replacement was rejected. The later interval is not separately tested; production design must retain the dynamic code object and revalidate/fail closed as appropriate up to resume. |

The first run exposed a test-fixture mistake: appending a byte outside the sealed executable pages did not invalidate the code signature. The harness was corrected to flip a byte in the helper’s mapped code page (offset 4096); that produced the expected dynamic invalidation. This is a test-design correction, not a change to application files.

## Phase 3 — Alternatives

| Approach | Assessment |
|---|---|
| Signed, immutable app bundle / read-only trusted resource | **SUPPORTED.** Provides a strong distribution and tamper-evidence boundary when correctly signed/notarized and verified, but a checked pathname can still be replaced by a process with write access unless the bundle/resource is protected. Best baseline for app packaging; not by itself an atomic execution binding. |
| Suspended `posix_spawn` + dynamic Security.framework identity | **SUPPORTED; promising, limited PROVEN result.** Uses an OS-created PID and a dynamic `SecCode`, validates before resume, and rejected the tested post-spawn replacement. More implementation/platform complexity; must fail closed on unsupported PID guest lookup and repeat on supported macOS versions. |
| Static `SecStaticCode`/path validation | **BLOCKED as execution proof.** It validates a filesystem object; Apple says it is not inherently linked to running code. Resolver-style preflight hash checks alone retain TOCTOU. |
| Descriptor-bound execution (`fexecve`, `execveat`) | **OPEN/BLOCKED pending platform contract.** These declarations were not found in the installed macOS SDK headers inspected. The available `posix_spawn` API takes a pathname. Do not infer descriptor execution support from other Unix platforms. |
| Plain path-based `Command::spawn()` after a second hash | **BLOCKED.** Rehashing narrows but does not close the check-to-spawn race. |

## Recommendation for next build

**Do not integrate the real client yet.** The PoC is promising enough to justify a separate, narrow follow-up research/engineering gate on the minimum supported macOS versions and distribution builds. Before any productive runner exists:

1. Repeat the suspended PID guest lookup and dynamic validity tests on every supported macOS version and architecture; include signed Developer ID and ad-hoc helpers, PID lookup failure, path replacement at controlled points, and code-signing requirement checks.
2. Obtain security review of the exact trust predicate (publisher requirement plus pinned CDHash policy), process suspension/resume race, post-validation file replacement semantics, host/guest selection, and OS release compatibility.
3. Add deterministic safe fault seams for validation failure, resume failure, kill failure and wait/reap failure; ensure all failures leave the app’s production route disabled.
4. Only then propose a separate production-runner build. It must bind the validated dynamic code identity and exact process run to exclusive staging, actual OS exit/reaping, complete artifacts, RAW/TIFF pixel identity, checksums, metadata and archive transaction. SANE cleanup still requires client-level evidence and cannot be inferred from OS process cleanup.

There is **no current basis to unlock `CaptureKind::Production`**. The application remains unchanged functionally; only this audit and standalone test harness are added.

## Verification

- `bash research/d3-b3d/run_poc.sh` — **PASS**, all scripted assertions above completed on arm64 macOS 26.6.2.
- `clang -Wall -Wextra -Werror ... -framework Security -framework CoreFoundation` — **PASS**, run by the research script.
- `cargo test --manifest-path src-tauri/Cargo.toml` — **PASS** (88 passed, 12 ignored test-harness child entry points).
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — **PASS**.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — **PASS**.
- `pnpm check` — **PASS**; `pnpm test` — **PASS** (7/7); `pnpm build` — **PASS**.
- `pnpm tauri build --bundles app` — **PASS**, native macOS `.app` (10.79 MiB).
- `git diff --check` — **PASS**.
- No scanner, Minolta client, SANE, Homebrew, or Research-005/009 resource was opened or changed. No production process path or Tauri/UI command was added.
