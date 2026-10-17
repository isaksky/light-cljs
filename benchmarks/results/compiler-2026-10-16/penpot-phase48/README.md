# Native profile after the reader allocation changes

The frozen phase47 V8 binary was sampled during a complete main-none compile.
It exited successfully in **38.23 seconds** and reproduced the phase42 output
byte for byte. Sampling overhead makes this a diagnostic run; the clean
phase47 observation is 35.71 seconds.

There are **5,942 main-thread observations**. Of these, **1,377 (23.2%)** have
exclusive leaves in `libsystem_malloc.dylib`. This includes allocations from
different callers; it is not a measured speedup available from replacing
Rust's allocator, nor a count of retained bytes.

`compiler_read_form` appears in 540 inclusive observations, down from phase45's
1,400 in its 7,891-observation pre-change profile. `array_environment` no longer
dominates the deduplicated iterator leaves. Inclusive counts overlap and must
not be added. The profile still shows bridge conversion, macro execution,
allocation and unit emission. `emit_unit_parts` has 276 exclusive observations;
its generated-parameter selection scans the accumulated UTF-16 string table.
That motivates a simple vectorizable first-character prefilter experiment.

`run.bb`, `summarize.bb` and `archive.bb` reproduce collection and summaries.
`sample.txt.gz` contains the raw sample. The complete output and frozen binary
remain in temporary storage, fingerprinted here. The known baseline main ESM
syntax defect remains; no production source changes are made by this phase.
