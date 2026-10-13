# D3-B3d/D3-B3e isolated macOS execution-identity research

This is a standalone research harness; it is not part of the Tauri/Rust build and is not callable by the app. It compiles harmless local C helper variants that only create a marker file and exit. They receive no scanner, SANE, network, or privileged capabilities. The parent uses `posix_spawn()` with the fixed `POSIX_SPAWN_START_SUSPENDED` flag, looks up the child by PID through Security.framework, validates the dynamic `SecCode`, compares its SHA-256 CodeDirectory hash to a literal trust anchor compiled into the parent harness (not supplied by the child), and either sends `SIGCONT` or kills and waits for the child.

Run from the repository root:

```sh
bash research/d3-b3d/run_poc.sh
```

The script uses `clang`, `codesign -s -` for ad-hoc signatures, and temporary directories created by `mktemp`. Test files are kept in the printed temporary directory for inspection. No keychain identity is created or changed. The experiment deliberately does not signal any PID after it has been reaped, avoiding the possibility of a PID-reuse race.

Scenarios: valid ad-hoc signature; mismatching trusted hash; replacement immediately before spawn; replacement after spawn and before initial identity lookup; replacement after one successful dynamic validation followed by a second validation immediately before resume; removal after validation; signature tampering; invalid PID lookup; unexpected child termination; abort before resume; injected resume/kill/reap failures; spawn failures; and an unsigned executable. The script's expected results are assertions in the harness, not a claim that each scenario proves a general operating-system guarantee. The fault cases preserve a sticky failure result; emergency cleanup is attempted so tests do not intentionally leak children.

The hard-coded test CDHash is deliberately independent of the helper. If the toolchain, SDK, or ad-hoc signature changes it, preflight fails closed and the research constant must be reviewed manually. This is a test pin only, not a production trust-update mechanism.

An ordinary certificate-backed signing identity is not available on the current host (`security find-identity -v -p codesigning` reports zero valid identities). A normal Developer ID / Apple Development signature test is therefore unavailable. Ad-hoc signatures test integrity identity only, not signer trust, notarization, or distribution policy. In particular, a successful `SecCodeCheckValidity` call without an explicit publisher requirement is not evidence that an expected publisher signed the process.
