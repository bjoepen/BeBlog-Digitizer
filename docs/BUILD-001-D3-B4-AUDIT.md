# Build 001-D3-B4 — ARM64 Sidecar Packaging & Verification

**Branch:** `build/001-d3-b4-arm64-sidecar-packaging`

**Baseline:** `origin/main` at `a38f3ee` (merge PR #13)

**Target:** macOS arm64 / `aarch64-apple-darwin`; tested on macOS 26.6.2, build 25G83.

## 1. Ausgangsaudit

**PROVEN:** D3-B3f's Model A is the basis. `sidecar.rs` previously provided fixed-path/hash/symlink checks, but had no actual sidecar resource to resolve and did not check Mach-O architecture or code-signature validity. `os_process_runner.rs` remains a `#[cfg(test)]` runner for fixed benign test modes only. `process.rs` keeps `FakeScannerAdapter` as the app's sole active adapter. `ArchiveManager::finalize()` rejects all non-Simulation finalization. `tauri.conf.json` previously had no sidecar resource map; `build.rs` only calls `tauri_build::build()`.

The existing `sidecar.rs` enum referred to the future Minolta client but was not wired to any process start. D3-B4 replaces that dormant resource identifier with a fixed test-sidecar identifier; no Minolta path or binary is packaged.

**PROVEN — production path still locked:** the application only resolves and verifies the test resource during Tauri setup. There is no `Command`/shell/process launch in `sidecar.rs`, no real scanner adapter or command, and no Production archive authorization.

## 2. Test sidecar and local build procedure

Source: `src-tauri/sidecars/src/test_sidecar.c`. The helper has no scanner, SANE, USB, or network code. It prints `arch=arm64 id=beblog-test-sidecar-v1`, exits 0, or accepts only `--marker-dir TEMP_DIRECTORY`. The marker mode resolves both the system temp root and caller's existing directory, requires the directory to be inside `TMPDIR`, then uses `openat()` with a fixed filename, `O_CREAT|O_EXCL|O_NOFOLLOW`, and mode 0600. It cannot choose a marker filename or overwrite one.

Build script: `scripts/build-arm64-test-sidecar.sh`:

1. Compiles only the local C test source with `clang -arch arm64`.
2. Produces an unsigned test fixture using `-no_adhoc_codesign`; separately builds the packaged candidate.
3. Signs candidate locally using `codesign --force --sign -` (ad-hoc; no Keychain identity).
4. Runs `codesign --verify --strict`, confirms `adhoc` in signing details, and requires `lipo -archs` to equal exactly `arm64`.
5. Computes full-file SHA-256 and compares it to the literal approved hash in Rust `sidecar.rs`. On mismatch it exits without replacing the packaged resource.
6. Installs the exact candidate at `src-tauri/sidecars/macos-aarch64/beblog-test-sidecar` only after every check passes.

**PROVEN for this checked-in resource:** `file` reports a thin 64-bit arm64 Mach-O; `lipo -archs` reports only `arm64`; `codesign` reports ad-hoc signature flags and strict verification passes. Full SHA-256 is:

```text
09dc4296d9f347fccd2e4fe8aa62a7e39160a04be6a652e2b79adbb0de360729
```

The value is pinned independently in `src-tauri/src/sidecar.rs`, not read from the candidate during startup. The test binary's CDHash is separate from the full-file digest and is not the trust anchor. A changed candidate fails the build script and runtime resolver until the pin is deliberately reviewed and updated.

**SUPPORTED, limited to this toolchain:** two builds using the script on this host produced the same pinned file hash. This is a local repeatability observation, not a general reproducible-build claim across Xcode/Clang/linker versions.

## 3. Verification and Tauri packaging

`SidecarResolver` accepts only the `Arm64Test` enum and resolves the constant path `sidecars/macos-aarch64/beblog-test-sidecar`. It retains the prior root/component symlink rejection, regular-file and executable checks, `O_NOFOLLOW|O_CLOEXEC`, device/inode race check, and independent full-SHA comparison. It additionally parses a thin Mach-O header and rejects any CPU type other than `CPU_TYPE_ARM64`; universal/fat binaries are not accepted.

On macOS it uses Security.framework `SecStaticCodeCreateWithPath`, `SecStaticCodeCheckValidity`, and signing information. It requires the `kSecCodeSignatureAdhoc` flag. Missing or invalid signature metadata is rejected. Non-macOS builds fail closed with `Unsupported`; no portable fallback silently bypasses verification.

At app setup, Tauri's `app.path().resource_dir()` is passed to the resolver. No path is received from UI input and no `PATH` search or shell is used. `tauri.conf.json` maps exactly the one test binary into `Contents/Resources/sidecars/macos-aarch64/beblog-test-sidecar`; the unsigned test fixture and C source are not configured as app resources.

The setup check only verifies; it does not execute the resource. The returned path is not consumed by a runner. App startup failure is expected if the fixed resource is missing or does not match the hash, architecture, executable, and ad-hoc-signature requirements. The built `.app` was launched once; the process remained running, confirming Tauri setup and `resource_dir()` verification completed, then it was quit. The test-sidecar was not started.

**OPEN:** The runtime resolver verifies the file before any future process start. It does not close a later pathname-to-spawn TOCTOU race. No process start API exists in this build, and this task does not claim that file validation binds the checked inode to a future loaded image.

## 4. Tests and negative cases

Rust tests use the checked-in harmless sidecar and test-only temporary copies:

- Correct independent SHA, ARM64 header, executable bit, ad-hoc signature — accepted.
- Missing resource, wrong/unapproved digest, non-executable file — rejected.
- Symlinked resource, directory, or root — rejected.
- x86_64 CPU type in otherwise thin Mach-O — rejected.
- Unapproved file-byte change — rejected by SHA pin.
- Genuinely unsigned ARM64 fixture — rejected by Security.framework signature check.
- Modified signed code — rejected by signature validity check.

The C test program was manually invoked only as the explicitly approved harmless test program. Its architecture/identifier output and marker inside a fresh temporary directory were verified. No Minolta client, other scanner program, or hardware path was started.

## 5. Test results

Final full-matrix results on macOS 26.6.2 arm64:

| Check | Result |
|---|---|
| ARM64 sidecar build/sign/hash script | PASS — repeat candidate hash matched independent pin |
| Harmless helper output/temporary marker | PASS — fixed id/architecture; marker limited to supplied temp directory |
| Sidecar positive/negative Rust tests | PASS — 6 sidecar resolver/security tests |
| `cargo test` | PASS — 90 passed, 0 failed, 12 ignored test-harness child modes |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `pnpm check` | PASS |
| `pnpm test` | PASS — 3 files, 7 tests |
| `pnpm build` | PASS |
| `pnpm tauri build --bundles app` | PASS — `.app` bundle 10.86 MiB |
| `.app` architecture and packaged resource inspection | PASS — app and sidecar thin arm64; exact mapped resource present; SHA-256 and ad-hoc signature verified |
| `git diff --check` | PASS |

## 6. Limits and next gates

- No Developer ID, publisher identity, or notarization is present or required. Ad-hoc signing provides no publisher attribution.
- Trust is appropriate for private local use against accidental/untrusted file substitution only while the app code containing the anchor is itself trusted. An owner/admin able to change both app and resource can also change the anchor.
- Only the current Mac mini M4 / macOS 26.6.2 setup is tested; other macOS versions are not claimed supported. Intel and Rosetta are intentionally unsupported.
- Sidecar process launch, stdout/stderr, process identity, cancel/timeout/reaping, client-level SANE cleanup, scan artifacts, staging and Production archive remain unimplemented/locked.
- Future runner work must separately bind process lifecycle and capture identity, validate all artifacts, and retain no-retry/no-auto-resume behavior.
- A production C client must not replace this test resource until a separate approved build defines source provenance, reviewed hash update, CLI contract, artifact staging, and acceptance tests.
- Existing app archive manifests were inspected read-only before launch; none were `InProgress`. The brief app launch performed setup/recovery, but no captures were created or changed. The app was quit after confirming it stayed running.

## 7. Scope confirmation

Only Digitizer source/configuration, a harmless C test helper, its local build script, the pinned test resource/unsigned test fixture, and this audit report are changed. No scanner hardware, Minolta client, SANE, Avision, Homebrew, Gamma configuration, Research-005/009 artifacts, Keychain configuration, Production archive path, or scanner UI command was touched or activated.
