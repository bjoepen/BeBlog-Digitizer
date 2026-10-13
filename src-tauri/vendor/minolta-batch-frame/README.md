# Minolta client source snapshot

This directory contains a byte-for-byte source snapshot and a separately adapted working copy.

- minolta-batch-frame.original.c is the untouched source supplied from /Users/bernd/Minolta-Research-005-PASS/minolta-batch-frame.c. Expected and verified SHA-256: 35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae.
- minolta-batch-frame.c is the D3-B5a working copy. It is not the original, is not a built or approved production binary, and must not be run against hardware in this build.
- minolta_profile.c/.h hold pure CLI/profile checks shared by the working client and harmless C tests.

The source snapshot is pinned by content hash, not an upstream VCS revision. No license or reproducible original build metadata was present in the inspected source directory; those remain release prerequisites. The working copy must be reviewed against the snapshot with a source diff before any future client build or execution.
