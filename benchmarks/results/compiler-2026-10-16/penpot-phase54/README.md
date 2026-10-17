# Isolated range iteration trial

Based on phase50's V8 CLI with mimalloc and a 256 MiB V8 heap. The private
reader view traverses added global ranges directly; older pending/changed
indices still use indexed lookup, in exactly the original order.

Main-none is 26.57 s / 1,327,398,912 bytes RSS, with complete output matching
phase42. Phase50 was 26.51/26.58 s: this single trial demonstrates no wall-time
gain. No full suite was run for this trial, and it is not promoted.

The incremental patch, preparation/run scripts, build log, exact benchmark
command, timing and source/binary/output hashes are preserved here.
