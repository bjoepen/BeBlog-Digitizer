# Build 001-D3-B3b — Safe Sidecar Integration, Phase 1

## Read-only baseline audit

Baseline: `main` at `0bcf03754b1f36b73a6f040a1cb66fe656739455` (Merge PR #8). The worktree was clean before creating this feature branch.

- **PASS — OS runner boundary:** `src-tauri/src/os_process_runner.rs` is included only under `#[cfg(test)]` from `process.rs`. Its `SafeTestProgram` enum hard-codes the current test executable and fixed test-harness filters. It has no caller-supplied executable path, shell, or environment-based selection. The runner serializes execution, keeps stdout/stderr separate, observes exit/reaping, reader closure, timeout/cancel, kill escalation and cleanup. The real Minolta client cannot be launched through this code in a normal app build.
- **PASS — Lifecycle:** `ProcessEvent` in `process.rs` represents start, separated stream output/closure, reaping, exit, cancellation, timeout, forced kill, unexpected end and cleanup. `ProcessLifecycle` in `adapter.rs` rejects invalid ordering, requires both streams closed and a reaped child before accepting exit, and disallows success after timeout/cancel/cleanup failure. The lifecycle currently validates adapter events; it does not expose an unforgeable production-only process capability.
- **PASS — Parser:** `SidecarLogParser` and `TransferAccumulator` in `log_parser.rs` distinguish a single `sane_read` chunk from the cumulative total, preserve stream identity and unknown diagnostics, reject arithmetic/profile overflow, and separate EOF from the client's final byte total.
- **PASS — Offline artifact import:** `OfflineArtifactPaths` and `import_offline_artifacts()` in `offline_import.rs` accept four explicit Rust paths, copy fixed artifact names into an app-owned UUID staging directory using exclusive creation, validate metadata/log/TIFF including RAW/TIFF pixel identity, record sizes and SHA-256, then archive without overwriting. It does not start a process. The importer currently copies into its own staging area; a future runner-to-import handoff should avoid adding a second staging implementation.
- **PASS — Archive production gate:** `CaptureManifest` distinguishes `Production`, `Simulation`, and `OfflineImport`. `ArchiveManager::finalize()` rejects Production until a future typed process-evidence path exists. No Tauri command exposes offline import or arbitrary executable selection.
- **PASS — Tauri packaging baseline:** `src-tauri/tauri.conf.json` bundles only the app, has no `bundle.resources` or external-binary configuration, and defines a restrictive CSP. No sidecar binary is currently packaged. `lib.rs` registers scan/batch fake-core commands only.
- **PASS — Recovery and interruption:** application startup calls `recover_incomplete()`. Staging/captures are marked interrupted rather than resumed. Cancellation and timeout are explicit lifecycle terminals; no automatic retries or batch resume are present.

## Decisions and implementation

This phase adds a typed, crate-private resolver for one fixed future app resource name under `sidecars/macos-<arch>/minolta-batch-frame`. It rejects missing files, symlinks, non-regular files and non-executable files, opens without following a final symlink, and verifies SHA-256 against an explicit trusted 32-byte digest. Its result has private fields and is not accepted by any process-start API. The resource is not added to Tauri bundling and the resolver is not exposed through Tauri or the frontend.

The existing OS process runner remains test-only and is the only process integration exercised. It remains restricted to the Rust test harness and its enumerated harmless modes. That runner does not consume the future Minolta resolver result. This intentional separation means no production executable can be launched in this build; D3-B3c must implement and independently audit any bridge between verified resource identity and a real OS process runner.

In tests only, the existing runner now obtains its executable path via `SidecarResolver::resolve_test_harness()`. This records the OS-provided current test executable identity for wiring coverage; its digest is not checked against a production trust anchor and this helper is not compiled into release builds. The production resolver remains separate and requires a caller-supplied trusted digest.

The existing offline importer remains the only artifact staging/validation implementation. No parallel staging pipeline is added. Fixed client output names are documented for the future handoff; no log-derived paths are used. Production archive finalization stays blocked.

### Process evidence and production gate

`ProcessLifecycle` currently gives validated evidence inside the adapter, but its input is an event stream and it is not an unforgeable capability tied to a production OS runner. The OS runner itself is test-only. Therefore this phase does **not** introduce or accept a production process-proof value and does not change the `Production` archive guard. A future proof must bind one runner-owned run to: successful spawn; observed OS exit status 0; both reader streams fully closed; successful child reaping; successful cleanup; and absence of timeout, cancellation, forced kill, or unresolved cleanup errors. Client log text can never satisfy these requirements.

## Sidecar resolution and future packaging contract

- Fixed resource layout: `Contents/Resources/sidecars/macos-<Rust target arch>/minolta-batch-frame`; only the constant client name and current target architecture are resolved. No PATH lookup, UI path, shell, or arbitrary argument is accepted.
- A trusted SHA-256 must be supplied by compiled/package metadata from a separately reviewed release manifest. Computing a digest from the binary at runtime and treating it as trusted is not acceptable.
- The resolver rejects symlink path components and validates executable mode, regular-file identity and content digest. It currently returns a path-based verified identity; path replacement between verification and a later `Command::spawn` remains a TOCTOU risk. No spawn integration is enabled until D3-B3c defines a mitigation and verifies the final app resource immediately at launch.
- Tauri `bundle.resources`/external-binary packaging is not configured in this build. A future universal or per-architecture binary naming decision, Apple signing/notarization behavior, quarantine handling, executable permissions, and DMG/app resource verification remain OPEN. Do not weaken quarantine or bypass macOS signing policy.

## Regression coverage

Resolver tests cover fixed resource naming, correct digest, digest mismatch, missing binary, non-executable file, and file/directory/resource-root symlink rejection. A fixed output-layout constructor names only the four expected artifact files and never consults client log paths; the existing importer remains the sole validator/staging/archive path. Existing `OsProcessRunner` tests cover harmless test-harness success, nonzero exit, start failure, timeout, cancellation, ignored SIGTERM, closed pipes while the child remains alive, reaping/cleanup and serialization. Recorded lifecycle tests exercise cleanup failure. A deterministic OS-level cleanup syscall failure is not injected; this remains an open test limitation. Existing parser, lifecycle, archive, batch, fake and offline import suites remain enabled. No cleanup failure is fabricated as a successful process proof; production proof remains blocked.

## Changed files

- `src-tauri/src/sidecar.rs`: fixed-resource resolver, symlink/type/executable checks, SHA-256 trust-anchor check, and offline tests.
- `src-tauri/src/os_process_runner.rs`: test-only runner obtains its fixed Rust testharness executable path through the resolver test helper; no production path is enabled.
- `src-tauri/src/offline_import.rs`: fixed client output-layout constructor for the existing importer; no second validator or archive path.
- `src-tauri/src/lib.rs`: registers the resolver as an intentionally dormant private module; no command or UI exposure.
- `docs/BUILD-001-D3-B3B-AUDIT.md`: baseline, decisions, limits, packaging notes, and verification evidence.

## Quality and safety

No scanner program, Minolta C client, SANE command/library, scanner hardware, `/opt/homebrew`, Research-005/009 artifact, gamma configuration, or backend was run, changed, rebuilt, or activated. No Tauri API or UI can start a real scan.

## Open items and D3-B3c prerequisites

- **BLOCKER:** Design a non-forgeable production process attestation with runner-only construction and make archive Production finalization require it.
- **BLOCKER:** Resolve path replacement between hash verification and spawn; verify the packaged binary identity at the execution boundary.
- **OPEN:** Supply and review the trusted SHA-256 release manifest for the exact client build; no client binary was accessed for this task.
- **OPEN:** Decide macOS arm64/universal binary naming, bundle resource configuration, signing, notarization, permissions and DMG verification.
- **OPEN:** Refactor the importer/runner handoff so the runner writes only into an exclusive capture staging area and the existing validator consumes that area without duplicate staging.
- **OPEN:** Define whether OS-level timeout/cancel can safely terminate the complete process group on supported macOS versions and document that a forced kill does not prove SANE cleanup.
- **OPEN:** Define how malformed/missing artifacts remain attached to a failed journal without permitting retries or overwrite.

## D3-B3b verification

`cargo test --manifest-path src-tauri/Cargo.toml` PASS (85 passed, 12 ignored test-harness child entry points); `cargo fmt --manifest-path src-tauri/Cargo.toml --check` PASS; `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` PASS; `pnpm check` PASS; `pnpm test` PASS (7/7); `pnpm build` PASS; `pnpm tauri build --bundles app` PASS (native macOS `.app` bundle); `git diff --check` PASS.
