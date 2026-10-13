# D3-B3d isolated macOS execution-identity PoC

This is a standalone research harness; it is not part of the Tauri/Rust build and is not callable by the app. It compiles harmless local C helper variants that only create a marker file and exit. They receive no scanner, SANE, network, or privileged capabilities. The parent uses `posix_spawn()` with the fixed `POSIX_SPAWN_START_SUSPENDED` flag, looks up the child by PID through Security.framework, validates the dynamic `SecCode`, compares its SHA-256 CodeDirectory hash to a preflight trust value, and either sends `SIGCONT` or kills and waits for the child.

Run from the repository root:

```sh
bash research/d3-b3d/run_poc.sh
```

The script uses `clang`, `codesign -s -` for ad-hoc signatures, and temporary directories created by `mktemp`. Test files are kept in the printed temporary directory for inspection. No keychain identity is created or changed. The experiment deliberately does not signal any PID after it has been reaped, avoiding the possibility of a PID-reuse race.

Scenarios: valid ad-hoc signature; mismatching trusted hash; replacement immediately before spawn; replacement after spawn and before identity lookup; signature tampering; unsupported/invalid PID lookup; child termination before identity lookup; abort before resume; and an unsigned executable. The script's expected results are assertions in the harness, not a claim that each scenario proves a general operating-system guarantee.

An ordinary certificate-backed signing identity is not available on the current host (`security find-identity -v -p codesigning` reports zero valid identities). A normal Developer ID / Apple Development signature test is therefore unavailable. Ad-hoc signatures test integrity identity only, not signer trust, notarization, or distribution policy.
