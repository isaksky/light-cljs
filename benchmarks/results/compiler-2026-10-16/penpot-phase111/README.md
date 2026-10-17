# Adaptive initial heap threshold — validated, still isolated

V8 now starts at a 256 MiB collection threshold, bounded by the configured
session memory budget. After collection, live heaps can grow toward that budget;
the initial threshold is not a second hard cap. Backing-store usage reduces the
remaining heap allowance. Two new tests exercise successful adaptive growth and
terminal exhaustion after growth.

Full validation passes **1,442 tests, zero failures, 106 ignored** at normal
concurrency, strict Clippy, formatting and all three Node workflows. Sixteen
focused buffer/budget tests pass. The native engine is unchanged from phase109.

Sequential cold runs, with **no heap-threshold environment override**:

| Build | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.80 s | 479,182,848 |
| main none | 22.95 s | 966,885,376 |
| main advanced | 28.81 s | 1,161,953,280 |
| immediate frozen phase110 main, 256 MiB override | 22.52 s | 966,361,088 |

The ordinary defaults retain the candidate's approximately 1 GB memory footprint
and performance range. All complete outputs match phase110 byte-for-byte and
pass explicit ESM syntax checks. The 996-file port fingerprint is unchanged.
Penpot has not been bundled and run in a browser. The latest fresh Shadow timings
remain phase109: dev 33.08 s / 2,197,323,776 bytes; release 60.08 s /
4,129,030,144 bytes. Native backend packaging is underway; production remains
phase101. Scripts, logs, exact environments and hashes accompany this note.
