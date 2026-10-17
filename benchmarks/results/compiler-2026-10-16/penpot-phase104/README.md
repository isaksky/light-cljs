# Phase 104: current V8 JavaScript and native profiles

Isolated instrumentation of phase102: a V8 Inspector CPU profile at 1 ms plus
macOS native sampling at 10 ms during the same main build. The diagnostic takes
24.87 seconds and produces output byte-identical to corrected production. This
is an instrumented diagnostic, not a new clean benchmark or a validated backend.
No full suite was run for the instrumentation.

The JavaScript profile spans 23.85 seconds. Largest self categories include idle
4.01 s, bridge `visit` 2.42 s, program 1.60 s, GC 1.33 s, `publicMethod` 0.78 s,
`decodeNode` 0.75 s and `macroArguments` 0.73 s. Inclusive encoder time is 3.16 s;
decoder time is 2.67 s; macro-call time is 9.02 s. These categories overlap and
must not be added. Idle includes work outside JavaScript; native calls may be
attributed to their JavaScript caller.

Native sampling records 2,186 main-thread observations. V8 megamorphic property
lookups, WeakMap operations and map lookup are prominent. Remaining Rust costs
include namespace-global counting, environment-elision validation, arena reads
and bridge decoding. Sampling trampolines account for 168 observations (7.7%),
so these are targeting evidence rather than precise wall-time shares.

Encoder line ticks highlight sequence traversal, active-cycle checks, private
brand checks and provenance bookkeeping. The encoder also eagerly constructs
diagnostic path strings for every successful node; deferring that construction
until failure is a new candidate to measure. This profile does not establish
that the change will improve whole-build performance.

The archive contains both compressed original profiles, aggregation scripts,
summaries, exact source mapping for line ticks, commands, patch and hashes. Large
outputs and the frozen binary remain under `/private/tmp/light-cljs-phase104/`.
Production remains phase101. V8's existing resource-limit and packaging gaps
remain, and the overall performance goal is unmet.
