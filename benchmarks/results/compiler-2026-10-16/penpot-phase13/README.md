# Phase13 protocol dispatch and updated profile

Public protocol consumers now retain their initial membership decision while
selecting a method. They still perform direct/native method lookup and reject
missing methods, but no longer repeat satisfies? afterward. Besides avoiding
redundant dispatch, this fixes a getter that removes its protocol marker turning
a missing method into a missing-key fallback. A regression exercises both the
missing and returned-method cases at every optimization level and in a macro.

All 1,379 tests pass, zero failures, 106 ignored; strict Clippy, formatting and
the three direct Node workflows pass. The 100,000-entry QuickJS traversal probe
fell from 1,730 to 1,366 ms. Probe sources and recorded outputs are included;
the before/after runtime factories remain under `/private/tmp/light-cljs-phase13/`.
This microbenchmark is diagnostic, not a replacement for Penpot measurements.

Cold render: 24.93 seconds, 558,809,088 bytes RSS, 12,618,455 output bytes. No
other CPU-heavy work overlapped it. The subsequent instrumented main run took
190.08 seconds at 1,394,130,944 bytes RSS; this is a diagnostic profile, not a
clean benchmark. Both application bodies match phase6b outside the embedded
runtime, and complete outputs pass node --check. Application execution remains
untested. The last complete clean main/shadow comparison is phase11/phase10.

The profile attributes 29.84 seconds to environment decoding, 13.19 to form
import, 11.12 to argument import, 8.09 to form export, 9.73 to unit evaluation,
7.43 to diagnostics and 24.32 to other analysis. Reader preparation is 15.30
seconds within macro calls, including 11.94 seconds preparing aliases.
Instaparse's grammar macro takes 16.57 seconds; rumext defc takes 20.54 seconds
across 1,033 calls. These are exclusive Rust scope buckets except the separately
reported reader preparation, which is included in macro-call time.

The deduplicated AST table storage is 284,162,256 bytes for 2,421,603 entries,
versus 470,467,984 bytes in phase7 before the Expr layout change. This excludes
heap-owned entry payloads and is not RSS. Runtime allocation, source-storage
and Rust data counts are retained in profile-main.log.

The profile is an isolated source archive at
`/private/tmp/light-cljs-phase13-storage-profile/`. Its preparation scripts in
`/private/tmp/light-cljs-phase13/` depend on the earlier phase5 instrumentation
templates and phase3 run script; this directory is evidence, not a standalone
supported profiling runner. The production binary, patch and untracked
name-index source are fingerprinted in source-hashes.txt.
