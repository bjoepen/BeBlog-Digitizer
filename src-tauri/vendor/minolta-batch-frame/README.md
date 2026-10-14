# Minolta client source snapshot

This directory contains a byte-for-byte source snapshot and a separately adapted working copy.

- minolta-batch-frame.original.c is the untouched source supplied from /Users/bernd/Minolta-Research-005-PASS/minolta-batch-frame.c. Expected and verified SHA-256: 35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae.
- minolta-batch-frame.c is the adapted working copy (D3-B5a profile support plus D5 frame-range 1–6). It is not the original and is not an approved production binary.
- A D5 ARM64 candidate, if built, is only an untrusted candidate: its hash must be reviewed and separately approved before any real scan. It must not replace the packaged D4 client or the independent `tools/minolta-hardware-test.sha256` trust value.
- minolta_profile.c/.h hold pure CLI/profile checks shared by the working client and harmless C tests.

The source snapshot is pinned by content hash, not an upstream VCS revision. No license or reproducible original build metadata was present in the inspected source directory; those remain release prerequisites. The working copy must be reviewed against the snapshot with a source diff before any future client build or execution.
