# Deferred namespace counts and native profiling

This phase promotes one engine-independent change: carry the incremental count
of namespace globals in `DefinitionSnapshot`. Each deferred analyzer previously
started its count at zero and rescanned the frozen global prefix. It now starts
from the snapshot's count and examines only new globals. Namespace visibility,
array settings, reader context and the order of macro evaluation are unchanged.
`namespace-counts.patch` is the two-file delta from phase44; production contains
that delta plus all earlier retained changes.

The preceding native profile of the isolated V8 compiler recorded 7,891
main-thread observations. The global iterator called by `array_environment`
accounted for 762 exclusive observations (9.7%). There were 13,361 deferred
analyses in the earlier scoped profile. With the count snapshot, the clean V8
main-none trial takes **42.74 s / 1,444,806,656 bytes RSS**, versus phase44's
47.32 s / 1,410,547,712. Its complete output is byte-identical to phase42.
The isolated V8 full suite passes **1,401 tests, zero failures, 106 ignored**.
The V8 backend itself remains unpromoted, with phase44's documented limitations.

Production's full suite passes **1,400 tests, zero failures, 106 ignored**;
strict Clippy, formatting and diff checks pass. All five benchmark jobs were
rerun sequentially in fresh processes. Shadow caches were cleared by the
existing benchmark runner; the port's 996-file fingerprint is unchanged.

| Build | Wall seconds | User seconds | System seconds | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: |
| light render | 15.05 | 14.91 | 0.12 | 509689856 |
| light main none | 117.87 | 117.23 | 0.60 | 1228734464 |
| light main advanced | 123.01 | 122.27 | 0.71 | 1334099968 |
| shadow release main | 57.24 | 139.21 | 400.46 | 3840278528 |
| shadow compile main | 30.90 | 89.73 | 401.98 | 2455175168 |

Production's 117.87 s main observation does **not** show a speed improvement
over phase42's 117.05 s. The measured V8 improvement must not be presented as
a demonstrated production speedup. Production remains 3.81 times slower than
shadow dev, at 50% of its RSS; advanced is 2.15 times slower than shadow release,
at 35% of its RSS. The overall speed goal remains unmet.

All three production outputs match phase42 byte for byte. Explicit ESM checking
passes render but rejects both main outputs with the previously documented
rumext `const`/`var` redeclaration. Byte equivalence does not establish executable
correctness. Penpot has not been bundled or run in a browser.

The sample profile also identifies name conversion and reader work:
`Name::new` appears in 814 inclusive observations, and the UTF-16 vector
construction specialization in 623. Inclusive categories overlap and must not
be added. `summarize.bb` computes exclusive leaves and deduplicates recursive
inclusive frames. The raw native sample is compressed in `sample.txt.gz`.
The sampler run is diagnostic, not a clean benchmark. Its output also matches
phase42. These findings motivate the next allocation experiment.

`production/` contains full test, Clippy, build and benchmark logs, recorded
commands, and explicit ESM diagnostics. Frozen binaries, complete outputs,
tracked worktree patch and essential `name_index.rs` remain under
`/private/tmp/light-cljs-phase45/production/`, with hashes recorded here.
`archive.bb` and `finalize.bb` reproduce evidence collection. The three
standalone Node workflows were last run in phase42; this phase changes no
JavaScript runtime source.
