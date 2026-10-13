# Build 001-D3-B3e — Execution Identity Hardening Research

Date: 2026-10-13
Branch: `research/001-d3-b3e-execution-hardening`
Baseline: merged D3-B3d commit `6eb56d8c74c888d695f91ed37c1fbda89179c546`; `origin/main` at audit start `b27905698b50b47842e721725a2e58e1b8e6a97c` (merge PR #11).

## Executive summary

**Decision: CONDITIONAL GO for a future, separately reviewed production-runner design; NO production activation in this build.** On this Mac, a harmless child launched suspended was found and dynamically validated through Security.framework. Replacing its pathname after spawn and after a first successful validation did not change the dynamic object's observed CDHash; the resumed process wrote the marker identifying the original loaded image. This is useful experimental evidence, not a general atomic-execution guarantee. Removing the path after validation also left dynamic identity lookup reporting the same hash, but resuming produced a child killed by signal 9. That surprising result is recorded, not generalized.

The current app remains deliberately locked: `OsProcessRunner` is test-only, the app exposes no real scanner command, and `ArchiveManager::finalize()` refuses non-simulation captures absent a future typed proof (`src-tauri/src/archive.rs:316-327`). The research harness only launches locally compiled marker-writing helpers. It does not touch scanner hardware, SANE, Homebrew, Research-005/009, or the Minolta client.

## 1. Baseline audit — merged D3-B3d

**PASS — baseline confirmed.** The D3-B3d commit is an ancestor of the `main` merge commit above. The worktree was clean before this research branch was created from current `origin/main`.

| Area | Existing property | Remaining boundary |
|---|---|---|
| `sidecar.rs:1-4,55-147` | Fixed resource enum; no PATH or shell; rejects symlinks, opens final file with `O_NOFOLLOW`, checks regular-file/executable status, device/inode identity, and SHA-256 against an explicit `TrustedSha256`. | It returns a pathname. A later `Command::spawn(path)` would resolve that name again; file identity is not atomically bound to execution. This resolver is not a launch API. |
| `os_process_runner.rs:1-4,25-79,523-541` | `#[cfg(test)]` runner invokes only the Rust test harness with fixed safe modes. Private ephemeral evidence requires OS-runner observations, actual `Exited(0)`, EOF on both streams, joined readers, successful reap/cleanup, matching executable digest, and no timeout/cancel/forced-kill/unresolved error. | It is not production-capable and its hash/path observation is not a macOS loaded-image attestation. Evidence is neither serializable nor consumable by the archive. |
| `process.rs:10-15,17-69,72+` | Typed lifecycle protocol; `RecordedProcessRunner` provides synthetic events without OS process creation. | Recorded events are test protocol data, not evidence of an OS process or SANE cleanup. |
| `adapter.rs:56-145` | `ProcessLifecycle` validates ordering and terminal consistency for runner events. | The event contract alone cannot prove that the producer observed kernel facts; producer trust and provenance matter. |
| `archive.rs:316-327` | Non-simulation finalization is rejected. | Production capture remains unavailable; future process evidence alone must not bypass artifact, metadata, and archive validation. |

**OPEN:** The research does not establish process-level SANE cleanup. A child being reaped proves only OS process lifecycle completion, not that SANE's internal `sane_cancel`/`sane_close`/`sane_exit` completed successfully.

## 2. Trust model for a future macOS sidecar

| Trust mechanism | What it can establish | Limits / policy |
|---|---|---|
| Ad-hoc signature + fixed CDHash | Integrity identity of the exact signed code relative to an independently provisioned hash, subject to the verified CodeDirectory/hash API semantics. | No publisher identity, no Developer ID chain, no notarization. Suitable for isolated experiments or tightly controlled local deployment only. A test child must never supply its own expected hash. |
| Developer ID + Team ID + designated requirement | A code-signature validity check constrained to the expected signer/team and a reviewed requirement can establish publisher/code identity under Apple's code-signing policy. Apple describes Team ID in the certificate subject OU and designated requirements as the code's identity requirements. | `SecCodeCheckValidity` without a publisher requirement is not a publisher check. Fail closed on absent certificates, invalid signatures, unexpected Team ID, requirement mismatch, or ambiguous signing information. This host has no valid certificate identity, so the path was not tested. |
| Signed/notarized app bundle | Distribution provenance, Gatekeeper/notarization assessment, and a signed resource envelope for packaged app resources. Apple notarization is an automated malware/security scan and ticketing workflow, not App Review and not a per-process runtime identity proof. | Notarization does not replace runtime process identity binding. Bundle/resource permissions and update integrity remain relevant. |
| Independently fixed CDHash | Exact version pin, if provisioned by a trusted release/update process separate from the candidate binary. | The pin must be authored/reviewed outside the tested child and protected by the signed app/update channel. Never “learn” a new hash from the binary being approved. |
| Updates / signer rotation | Explicitly versioned trust policy can allow planned upgrades. | No automatic trust-on-first-use. Use a signed update manifest and explicit transition policy (old signer authorizes new version, or a reviewed dual-pin window); reject unknown identities and rollback unless explicitly permitted. Preserve auditability of the transition. |

**Recommended future distribution:** distribute BeBlog as a Developer-ID-signed, hardened-runtime, notarized app bundle; sign nested sidecar code with an approved Developer ID identity; constrain dynamic validation with an explicit Team ID/designated requirement and, where operationally practical, a release-specific CDHash from a signed trust manifest. The app bundle and signing pipeline provide the distribution trust base; suspended spawn plus dynamic guest-code validation is defense in depth for the process actually observed. Notarization alone is not the publisher requirement. Until certificate-backed positive and negative tests exist on the supported macOS matrix, keep real launch disabled.

Apple references: [posix_spawn suspended-start flag](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/posix_spawnattr_getflags.3.html), [SecCodeCopyGuestWithAttributes](https://developer.apple.com/documentation/security/seccodecopyguestwithattributes%28_%3A_%3A_%3A_%3A%29), [guest attribute keys](https://developer.apple.com/documentation/security/guest-attribute-dictionary-keys), [SecCodeCheckValidity](https://developer.apple.com/documentation/security/seccodecheckvalidity%28_%3A_%3A_%3A%29), [SecStaticCode](https://developer.apple.com/documentation/security/secstaticcode), [CDHashes signing information](https://developer.apple.com/documentation/security/kseccodeinfocdhashes), [ad-hoc signature flag](https://developer.apple.com/documentation/security/seccodesignatureflags/adhoc), [Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/), [requirement language](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/RequirementLang/RequirementLang.html), [hardened runtime](https://developer.apple.com/documentation/security/hardened-runtime?changes=latest_minor%2Clatest_minor), and [notarization workflow](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution?changes=_9).

## 3. Isolated experiments

The harness lives under `research/d3-b3d/`; it is separate from Cargo/Tauri and is not an app-callable path. The parent contains a literal expected test CDHash, independent of the child. The script compiles harmless children which only create a marker and exit. Both signed test children are ad-hoc signed. No keychain identity was created or changed.

| Scenario | Result on this host | Classification |
|---|---|---|
| Correct pinned ad-hoc helper | Two dynamic lookups matched the pin; `SIGCONT`; child exited 0 and marker confirmed helper A. | **PROVEN observed here**; general cross-version guarantee **SUPPORTED**, not independently established. |
| Wrong pin | Hash mismatch; no resume; kill and wait/reap; marker absent. | **PROVEN observed here**. |
| Replace target before spawn | Executed candidate did not match independent pin; denied and reaped. | **PROVEN observed here**. |
| Replace after spawn, before first guest lookup | Dynamic validation returned `errSecCSStaticCodeChanged (-67034)`; denied and reaped. | **PROVEN observed here**. |
| Replace after first successful dynamic validation, before second validation/resume | Second lookup on the retained dynamic `SecCodeRef` still yielded original A's pinned hash; resume ran original loaded image, confirmed by marker. | **PROVEN observed here**. Atomic binding guarantee across macOS releases **OPEN**. |
| Remove original path after successful validation | Second dynamic lookup still reported original hash; after resume child terminated by signal 9 and wrote no marker. | Behavior observed; cause/guarantee **OPEN**. Do not infer safe path-removal semantics. |
| Tamper signed executable | Dynamic validation failed (`errSecCSGuestInvalid`, observed `-67063`); denied/reaped. | **PROVEN observed here**. |
| Invalid PID / unavailable guest lookup | Lookup failed (observed OSStatus 100003); child was not released and was reaped. | **PROVEN observed here**. This is not a general “SecCode unavailable” contract test. |
| Child unexpectedly terminated before lookup | Lookup on unreaped zombie failed; parent then waited/reaped and did not use PID again. | **PROVEN observed here**; reinforces no signal/query after reap. |
| Abort before resume / injected resume error | No `SIGCONT`; child killed and waited; marker absent. | **PROVEN harness behavior**. |
| Injected kill or reap failure | Sticky unresolved failure remains; emergency real kill/wait is attempted to avoid leaving test children. Result cannot pass cleanup gate. | **PROVEN fault-injection policy**, not an induced kernel failure. |
| Unsigned helper | Dynamic validation returned signing metadata/hash on this OS, but hash mismatched the independent pin; denied. | **PROVEN observed here**; demonstrates why successful validity without a publisher requirement is not sufficient signer evidence. |

The harness checks the fail-closed decision in the chain `Spawn → Suspend → Identify → Validate → Release or Abort → Reap`. It never sends signals after `waitpid` has reaped the child. Injected kill/reap errors occur before the real emergency syscalls; they test result semantics and fallback cleanup, not kernel failure modes.

## 4. macOS compatibility and API boundaries

**Tested:** macOS 26.6.2 (build 25G83), `arm64`; Command Line Tools SDK at `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`; Clang, `posix_spawn` with `POSIX_SPAWN_START_SUSPENDED`, Security.framework guest lookup and dynamic code signing APIs. SDK headers declare the suspended flag and APIs used by the PoC. Only this OS/architecture/toolchain combination was run.

Apple's archived `posix_spawn` reference documents that the suspended flag starts the process suspended before user-space execution. Apple's Security documentation describes looking up guest code by PID and dynamic validity checks; it also says dynamic validation is protected against modifications to the filesystem source. The experiments are consistent with identity tied to the observed dynamic object after spawn, but the report does not elevate that to an atomic path-to-image contract beyond the documented APIs.

**OPEN:** Older/newer macOS support, entitlement requirements, behavior under hardened runtime, launch restrictions, API availability/failure details, signed/notarized bundle resource behavior, and certificate-backed signer requirements have not been tested. If an API is unavailable or returns ambiguous/error data, production policy must reject and clean up rather than fall back to path hashing.

Developer ID testing requires a valid Apple Developer Program signing identity/certificate and provisioning suitable for distribution. This machine reports `0 valid identities found` for `security find-identity -v -p codesigning`; therefore no normal Developer ID or notarization test was possible. Direct distribution prerequisites include Developer ID signing, Hardened Runtime, secure timestamp, signing nested executable code, and notarization via Apple's current notary workflow. **No claim of notarization was made for the PoC.**

## 5. Fail-closed chain and architecture decision

| Stage | Fail-closed requirement | Research result / residual risk |
|---|---|---|
| Spawn suspended | Fixed trusted executable/configuration; no shell/PATH/UI path; fail on spawn error. | Harmless test path only. Production path binding remains a design decision. |
| Identify | Obtain dynamic guest `SecCode` for actual child PID; retain PID/process handle lifecycle carefully. | Works in observed cases; lookup failure is terminal. PID alone is not a durable identity after reaping. |
| Validate | Require valid signature **and** explicit publisher/designated requirement; compare independently provisioned CDHash if used. | Ad-hoc integrity checks tested; publisher requirement not tested due missing certificate. |
| Resume | Resume only once all identity conditions pass; any resume error is failure. | Error path injected before signal; actual OS resume failure was not forced. |
| Abort | Terminate only the known live child; bounded escalation; never signal after reap. | Harness follows this ordering; actual kill failure not induced. |
| Reap | Successful wait/reap required; errors remain errors even if emergency cleanup later physically reaps. | Reap failure only injected, not produced by kernel. |

**A — controlled sidecar with dynamic process check:** stronger process-specific defense in depth and can reject replacement/tampering before the child runs, but higher API, lifecycle, version-compatibility, and signer-policy complexity.

**B — signed immutable app bundle/resources:** simpler distribution trust boundary and a natural place for a pinned sidecar. It does not alone prove which image a later pathname spawn executed if writable replacement is in the threat model.
**Recommendation:** combine B as the primary trust/distribution model with A only if certificate-backed tests and supported-OS validation establish a clear operational benefit. Keep resources non-writable to the app's untrusted inputs, validate bundle signature/resource identity, and make runtime guest validation fail-closed. Avoid claiming that rehashing a pathname immediately before `Command::spawn()` removes TOCTOU.

**Production status: BLOCKED in current app.** The research decision is **CONDITIONAL GO** for designing a future runner, not authorization to enable it. Required gates: explicit threat model; Apple API review for supported OS versions; Developer ID signer/requirement positive and negative tests; signed/notarized app packaging tests; process-identity race review; deterministic failure cleanup review; security review of updater and trust-anchor rotation; and end-to-end artifact/process proof before Production archive integration.

## 6. Test and build results

Results are to be recorded after executing the full matrix on this branch. The PoC command is `bash research/d3-b3d/run_poc.sh`; it uses only newly compiled harmless marker helpers and temporary files. No Minolta or scanner executable is invoked.

| Check | Result |
|---|---|
| Research PoC | PASS — all expected assertions; harmless local test helpers only |
| `cargo test` | PASS — 88 passed, 0 failed, 12 ignored (harness child modes; separately exercised via allowlisted runner tests) |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `pnpm check` | PASS (`tsc --noEmit`) |
| `pnpm test` | PASS — 3 files, 7 tests |
| `pnpm build` | PASS — Vite production bundle |
| `pnpm tauri build --bundles app` | PASS — native app bundle, macOS arm64 host |
| `git diff --check` | PASS |

## 7. Scope and open risks

- No app/Rust/Tauri production code was changed; only the research harness and its documentation are in scope.
- The CDHash is an independent literal for the current test helper/toolchain, not a shipping trust root or update strategy.
- A publisher/designated requirement has not been exercised. Ad-hoc signature and CDHash do not establish publisher identity.
- Dynamic lookup after path replacement/removal was tested only on one OS build. The unlink-then-resume termination is unexplained.
- No actual kernel kill, resume, or wait failure was induced; those cases are deterministic harness fault injections.
- No Developer ID, notarization, or cross-version compatibility claims.
- No SANE cleanup, scanner execution, Production archive, or real sidecar integration was tested or unlocked.

Next build should first define the supported macOS range and distribution/update trust policy, then run certificate-backed signing/requirement and bundle/notarization experiments. Production implementation must remain a separate approval-gated task.
