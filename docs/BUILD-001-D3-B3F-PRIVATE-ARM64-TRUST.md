# Build 001-D3-B3f — Private ARM64 Trust Model

**Status:** Research and architecture preparation only

**Branch:** `research/001-d3-b3f-private-arm64-trust`

**Baseline:** `origin/main` at `0727897` (merge PR #12; contains D3-B3e commit `be8ef3409454103962b6880a3acea4b9baf24e6c`)

**Host inspected:** Mac mini M4, macOS 26.6.2 build 25G83, `arm64`; Rust target host `aarch64-apple-darwin`; Command Line Tools SDK `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`.

## 1. Decision summary

**Recommendation: Model A for the initial private sidecar release; Model B remains an optional, separately justified defense-in-depth step.** A controlled app bundle, fixed ARM64 sidecar resource, ad-hoc code signature, and a full SHA-256 digest pinned independently in the application/build configuration are proportionate to one privately controlled Mac mini. Updates must be explicit: a changed digest is a reviewed trust change, never an automatic “learn new hash” operation.

This recommendation assumes the primary threat is accidental replacement/corruption or an untrusted downloaded artifact, not a malicious local administrator or the same user deliberately editing both the app and its trust anchor. Ad-hoc signing supplies integrity metadata, not a publisher identity. The trust anchor stored in an app binary/config is independent of the sidecar file but is not a separate security boundary against an owner who can replace/rebuild the whole app.

**PROVEN:** The current app does not activate a real scanner process, does not package a sidecar, and cannot finalize a `Production` capture. This build adds only this document. The D3-B3d/e experiments remain research evidence, not a production process runner.

**BLOCKED:** This report does not authorize Minolta integration, a real OS runner, SANE calls, UI/Tauri scanner commands, or `CaptureKind::Production` finalization.

## 2. Binding project and platform requirements

- Private operation on the project owner's Mac mini M4.
- Apple Silicon only; `aarch64-apple-darwin` is the intended Rust target. Intel and Rosetta are explicitly out of scope.
- No Apple Developer ID, paid Developer Program membership, or notarization dependency.
- Local ad-hoc signing is acceptable.
- Unknown or changed sidecar binaries are rejected until explicitly reviewed and approved.
- Current tested reference is macOS 26.6.2 on arm64. No other OS release is claimed supported until tested.

**PROVEN:** The current audit host reports macOS 26.6.2 build 25G83, `arm64`, Rust host `aarch64-apple-darwin`, and the stated Command Line Tools SDK.

**SUPPORTED:** Rust/Tauri can build for a specified Apple target, and this host has already completed the native Tauri `.app` build. Current config relies on the build host and does not explicitly declare an ARM64-only release policy.

**OPEN:** The exact deployment target/minimum macOS version is not declared here and has not been separately selected. Do not invent or change a deployment target as part of this research-only build.

## 3. Read-only baseline audit

| Component | Finding |
|---|---|
| `docs/BUILD-001-D3-B3D-RESEARCH.md` | Records the original suspended-spawn/Security.framework PoC and the earlier, limited experiments. Its open items about replacement after successful validation and deterministic fault paths are superseded only by the additional observations in D3-B3e, not converted into general guarantees. |
| `docs/BUILD-001-D3-B3E-RESEARCH.md` | Records post-validation replacement/removal experiments, injected failure results, OS/API limitations, and lack of a Developer ID identity. Correctly limits conclusions to this tested host. |
| `research/d3-b3d/` | Standalone C harness and harmless marker helpers; not part of Cargo/Tauri and not app-callable. It creates local ad-hoc signatures for test helpers, has a test CDHash literal, and does not invoke a scanner. |
| `src-tauri/src/sidecar.rs` | Fixed resource enum; no PATH/shell/UI path selection. Rejects symlinked path components, opens the file with `O_NOFOLLOW`, checks regular-file/executable status and device/inode consistency, computes SHA-256 against a supplied `TrustedSha256`, then returns a pathname. It is not execution and does not itself eliminate path-to-exec TOCTOU. |
| `src-tauri/src/os_process_runner.rs` | Included under `#[cfg(test)]`, uses only the current test harness and fixed safe test modes. Not a production runner; its private test evidence is not accepted by the archive. |
| `src-tauri/src/archive.rs` | `ArchiveManager::finalize()` rejects any non-`Simulation` kind with `PermissionDenied` pending a future typed process proof. Offline imports have their own path and cannot be promoted to Production through this API. |
| `src-tauri/tauri.conf.json` | Tauri v2 config builds the `app` bundle target but declares no sidecar resource, `externalBin`, or architecture-specific resource mapping. `identifier` is set. No Intel/Rosetta packaging is present. |
| Build scripts/config | `package.json` exposes Vite/TypeScript/test/Tauri commands; `src-tauri/build.rs` is Tauri's build integration. There is no project release/signing script or explicit `.cargo/config.toml` target pin. The native build therefore follows the host unless a target is passed explicitly. |

**PROVEN — production scanner path remains locked:** there is no production process runner in `process.rs`; `os_process_runner.rs` is test-only; `sidecar.rs` has no start method; no sidecar is configured for bundling; archive finalization blocks Production. No source file implementing app behavior is changed in this build.

## 4. Private threat model

### In scope

- Accidental corruption, stale files, or wrong-architecture binaries.
- A sidecar copied from an untrusted or unintended source.
- A changed sidecar after explicit approval.
- Symlink/path substitution within the app resource tree.
- Software bugs that might run an unintended executable or silently accept an update.

### Out of scope / not defended by this design

- A malicious local administrator or owner who can rewrite the application, embedded trust configuration, and sidecar together.
- A compromised build toolchain, source repository, Rust dependency, or local account.
- Kernel compromise, code-signing subsystem compromise, or physical attacks beyond the owner's control.
- Publisher identity or third-party provenance: ad-hoc signatures have no Developer ID publisher.

For one privately operated Mac, the app bundle and OS account are the operational trust root. A separately reviewed source commit/build record and explicit digest approval protect against mistakes and untrusted replacement, but not a determined actor with write access to all local trust material.

## 5. Identity layers — do not conflate them

1. **Full file SHA-256:** hashes every byte of the sidecar file. It detects any byte-level change relative to a separately approved digest. It says nothing about who built/signed the file or what process image was actually executed.
2. **Ad-hoc code signature:** gives macOS code-signing structure and integrity checking, but has no certificate chain, publisher identity, Team ID, or notarization. A valid ad-hoc signature is not proof of trusted authorship.
3. **CDHash:** identifies the CodeDirectory signature hash (the D3 PoC used the 20-byte SHA-256 CDHash exposed by Security.framework). It is not the same as the full file SHA-256; changes outside the CodeDirectory-covered content may have different implications, so both pins serve distinct purposes.
4. **Signature validity:** verifies code according to the available signature policy. For an ad-hoc signature this is an integrity check, not a publisher check. An unsigned or modified binary must fail closed; never interpret signing-information availability by itself as a valid identity.
5. **Actual process identity:** requires evidence tied to the OS process, not a static file path. D3-B3d/e explored suspended spawn, PID-bound dynamic `SecCode`, validity, CDHash comparison and a second validation before `SIGCONT`. Observations are specific to macOS 26.6.2/arm64 and do not prove lifecycle completion or scanner correctness.

**PROVEN (local experiment):** D3-B3e's pinned ad-hoc helper passed on this host; tested tampering/replacement/hash mismatch was rejected; path replacement after first validation retained the observed original dynamic CDHash and ran the original marker helper.

**OPEN:** Cross-release semantics, resource exhaustion/permissions, and the exact guarantee for all process-image mutations are not established. Path removal after validation had a surprising signal-9 outcome. Treat Security API lookup/validity errors and ambiguous metadata as denial.

## 6. Recommended local build and update workflow

The following is a proposed controlled procedure, not an implemented release script:

1. Check out an explicitly reviewed source commit; record source SHA, dependency lockfiles, Xcode/Command Line Tools and Rust versions.
2. Build only for `aarch64-apple-darwin`. Inspect the output with `file`/`lipo -archs`; reject anything not strictly ARM64 or containing an unexpected architecture.
3. Apply local ad-hoc signing with macOS `codesign` to the sidecar. Do not create or modify a keychain identity. Verify the result with `codesign --verify --strict` and inspect signature details; require an ad-hoc signature and reject missing/invalid signatures.
4. Calculate full file SHA-256 (for example, `shasum -a 256`) and record CDHash/signature metadata (`codesign -dv --verbose=4` and/or Security.framework tooling). Record both separately; do not substitute one for the other.
5. Independently review the digest against the artifact and build record. Pin the approved full SHA-256 in a trusted app/build configuration, not in the sidecar, its stdout, or a file the sidecar can write. The expected digest must not be derived at runtime from the candidate binary.
6. Package the fixed sidecar path inside the app bundle using an explicit Tauri resource mapping. Ensure resource names are stable and ARM64-specific, and reject missing files, symlinks, unexpected architectures, signature failures, or digest mismatch.
7. Ad-hoc-sign/verify the completed app bundle locally if needed for normal macOS operation. This signature also supplies no publisher identity and no notarization ticket. Document any Gatekeeper/opening behavior observed on the target Mac; do not claim general distribution support.
8. For an update, stop using the old bundle, build and inspect the new artifact, explicitly review the new full SHA-256/CDHash and source provenance, update the external-to-sidecar pin in the reviewed app source/config, rebuild, and test. Unknown hashes fail closed. Keep the prior manifest/change record; do not auto-accept new sidecar identity.
9. Before a future real-process build, separately implement and audit process binding, stream collection, timeout/cancel/reap, staging, artifact validation and archive transaction. These steps are not granted by a file hash or app signature.

**SUPPORTED:** `codesign`, `shasum`, `file`, `lipo`, `xcrun`, Rust/Cargo and Tauri CLI are available as local macOS tooling. The prior PoC successfully used Clang, `codesign -s -` for ad-hoc signatures, `SecCodeCheckValidity`, and PID guest lookup.

**OPEN:** This build did not sign a production sidecar or app bundle and did not alter Keychain. Exact Tauri resource declaration, bundle signing order, first-launch behavior on this Mac, and update script are intentionally deferred until a separately approved implementation build.

## 7. ARM64 policy in project structure

- **Rust build:** next build should add an explicit release command/CI assertion for target `aarch64-apple-darwin`, not change the global developer target. Validate the produced executable architecture.
- **Sidecar resource:** retain fixed enum selection. Use a stable `macos-aarch64` resource directory and configure Tauri to package only that resource for this private app. Never accept architecture or executable paths from UI input. Current resolver formats `macos-{ARCH}`; confirm that `ARCH` resolves to `aarch64` for the selected target.
- **No Intel fallback:** do not add `x86_64`, universal binaries, Rosetta detection, or fallback PATH lookup.
- **Test policy:** record exact macOS version/build and `arm64` on every acceptance run. Other macOS versions are unsupported until the focused identity, packaging, and lifecycle suite passes there.
- **Deployment target:** leave unchanged until an explicit minimum version is chosen from actual API and Tauri support requirements.

## 8. Architecture comparison

| Criterion | A — controlled bundle + SHA-256 + ad-hoc signature | B — A plus suspended start and dynamic process check |
|---|---|---|
| Accidental/untrusted file replacement | Strong detection if digest is independently pinned and bundle/config is protected. | Adds detection tied to observed process identity before user code resumes. |
| Malicious same-user/admin changing app and pin | Not prevented. | Also not prevented if attacker can replace app/trust policy; dynamic check does not create a separate trust root. |
| Publisher identity | None. | None unless explicit publisher requirement exists; this project forbids relying on Developer ID, so do not claim publisher proof. |
| Implementation complexity | Lowest; fits one locally controlled Mac and local ad-hoc signing. | Higher: Darwin-specific spawn flags, Security.framework FFI, suspended-process cancellation/reaping and version testing. |
| Evidence maturity | File hash, bundle resource and signature validation remain to be integrated/tested for actual sidecar. | D3-B3d/e PoC observed behavior on one macOS/arm64 host only; no production runner. |
| Maintenance/update | Explicit pin review and bundle rebuild. | Same update policy plus OS API compatibility/fault tests. |

**Decision: choose A initially.** It is adequate for the stated private, single-owner threat model when the app bundle/trust configuration is controlled and changes require explicit review. B is optional defense-in-depth only if later threat review requires protection against a pathname race between verification and spawn. Do not pay its maintenance cost while representing it as publisher authentication. If B is later chosen, require PID-bound dynamic identity, explicit fixed hash, fail-closed behavior, bounded cleanup/reaping, and tests on every supported macOS version. Neither model demonstrates SANE cleanup nor artifact correctness.

## 9. Open risks and next-build gates

- **OPEN:** Establish whether app and trust configuration are installed under a location/permission model that prevents accidental writes; owner/admin can always alter local trust material.
- **OPEN:** Select and document a minimum macOS release; until then only 26.6.2 arm64 is tested.
- **OPEN:** Pin the actual C sidecar source commit/build recipe and confirm it can be built as a standalone ARM64 artifact without changing Research/SANE files.
- **OPEN:** Test local ad-hoc signing/verification on the actual packaged sidecar and whole `.app`, including first launch and updates. No Developer ID or notarization assumption.
- **OPEN:** Specify a human-reviewed hash-rotation procedure and prevent silent trust-on-first-use.
- **OPEN:** Decide whether dynamic process identity is necessary for the private threat model; D3-B3e's path-removal behavior needs interpretation before relying on it.
- **BLOCKED:** Real OS runner, real Minolta process, scanner hardware and Production archive remain locked.

### Prerequisites for the next development build

1. Approval of Model A and the owner-controlled trust boundary.
2. A separately supplied, reviewed C-client source/build artifact location (read-only in the eventual build), exact ARM64 output contract, and argument/output contract.
3. A Tauri resource-packing design that contains only the fixed ARM64 sidecar and has no UI-configurable executable path.
4. Unit tests for independent full-SHA pin, ad-hoc signature verification, wrong architecture, missing file, symlink, tampering and unknown-update rejection; all use harmless fixtures.
5. A documented explicit update approval flow before any real process-launch work.
6. Keep Production finalization disabled until process outcome and all validated/archive artifacts are bound to the same capture.

## 10. Scope and verification

This is a documentation-only research change. No scanner/client process was run; no scanner, SANE, Avision, Homebrew, Research-005/009, Gamma, Keychain, or production file was modified. No real sidecar was signed or packaged. The app's scanner path remains locked.

The required hardware-free Rust, frontend, formatting, Clippy, Vite and native ARM64 `.app` checks are recorded in the final task report and must pass on this branch. `git diff --check` is also required.
