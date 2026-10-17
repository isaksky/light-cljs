# Phase15 source-tree lifetime and shared-storage audit

Release each namespace's parsed nodes and roots after its analysis, once
deferred definitions have owned forms and snapshots. Retain source text and
header facts for diagnostics and later lazy dependency discovery.

| Job | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 24.24 | 532840448 |
| main none | 181.02 | 1380941824 |
| main advanced | 186.85 | 1522352128 |

Fresh processes, one job at a time, no overlapping builds, tests or profiles.
Times and advanced peak memory are essentially unchanged from phase14. Main
none's observed peak is about 50 MB lower. All three complete output files are
byte-identical to phase14 and pass Node syntax checking. Penpot application
execution remains untested. Shadow was not rerun in this phase; phase14 retains
the preceding fresh release/dev comparison (56.72/30.19 seconds).

The standard `cargo test` invocation passes all 1,380 tests, zero failures,
106 ignored. Strict Clippy, formatting and diff checks pass. The earlier
`cargo test --all-targets` attempt also passed every test, then failed because
it invoked the compiler benchmark without its required release/sample flags;
that log is retained separately as all-targets-attempt.log. Direct Node runtime
workflow logs remain those in phase13; phase15 changes only Rust source-tree
ownership. The port is unchanged from phase14's recorded 996-file hash.

The isolated shared-storage audit uses the phase13 profile source, with added
counting of unique shared maps, vectors, sets and reader contexts. For render,
the remaining shared maps/vectors/sets estimate roughly 10 MB; reader-context
layers, bases and values add about 4.8 MB. These are capacity/payload estimates,
not RSS, and omit HashMap bucket/control overhead and allocator rounding. They
do not justify another wholesale snapshot representation change on their own.
The profile does not include the phase14 arity index or phase15 tree release.

Profile preparation files and render output are retained here. They depend on
the isolated `/private/tmp/light-cljs-phase13-storage-profile/` project and are
diagnostic evidence, not a standalone profiling tool. Production binary, patch
and name-index source remain under `/private/tmp/light-cljs-phase15/` and are
fingerprinted in source-hashes.txt.

Three subsequent experiments remain unapplied. A method-to-protocol name cache
reduced a 100,000-entry traversal probe from 1,413 to 1,373 ms. Disabling demand
preparation reduced a separate control from 1,445 to 1,305 ms, while filtering
unregistered protocols gave 1,422 ms. An eight-entry locals-view cache reduced
the instrumented render's misses from 5,888 to 5,371, environment decoding from
4.776 to 4.593 seconds and total time from 24.92 to 24.50 seconds, with higher
observed RSS. These single diagnostic observations do not establish a useful
whole-application gain. Sources and logs remain under
`/private/tmp/light-cljs-phase16/`, `phase17/` and `phase18/` respectively.
