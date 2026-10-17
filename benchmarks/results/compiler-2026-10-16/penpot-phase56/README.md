# Isolated release profile trial

Same source as phase55, built with `CARGO_PROFILE_RELEASE_LTO=thin` and
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`. Crate roots are touched before building
to prevent the shared-target stale-artifact issue documented in phase55.

Main-none takes 25.14 s / 1,314,275,328 bytes RSS, and the complete output
matches phase42. Phase55 took 25.84 s / 1,358,200,832 bytes. The binary shrinks
from approximately 69 MiB to 65 MiB. The first build with these settings takes
45.35 seconds, including dependency rebuilding; it is not directly comparable
with an ordinary incremental project rebuild.

This is a single-run, small improvement, not a new validated production result.
The release profile is unchanged in production. No full suite was run for this
isolated profile. Scripts, logs, command, timing and hashes are preserved.
