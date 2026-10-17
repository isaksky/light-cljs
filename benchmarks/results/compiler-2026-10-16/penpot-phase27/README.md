# Skip transport of provably unread bootstrap environments

Bootstrap compilation records whether each macro reads its hidden `&env`
parameter. The proof uses resolved local IDs, including nested captures,
and rejects raw JavaScript, arguments reflection and type/method bodies.
Reader-wrapped functions and source macros retain the complete environment.
Only compiler-owned macros at the bootstrap declaration frontier use the
fast path. Their functions execute normally with nil in the unread slot.

A bounded Rust traversal rejects compiler literals and inputs whose logical
size/depth cannot safely fit both normal import and snapshot budgets. Those
cases use the original transport, preserving conversion failures and literal
constructor behavior. Cancellation polling remains active. The full suite
passes **1,392 tests, zero failures, 106 ignored**; strict Clippy, formatting,
diff checking and all three Node workflows pass.

Three new tests cover the static proof, conversion limits and host recovery,
and invalid literal metadata. The separate CLI probes show why the fallback
is needed: the unguarded prototype silently accepts a too-small input budget
and an invalid regex in a loaded macro's metadata. Both the previous build
and the final guarded build reject them. The unguarded prototype was never
accepted as the production implementation.

## Clean rerun

Fresh processes, cold shadow caches, sequential jobs, no overlapping
CPU-heavy builds, tests or profiles. GB below means decimal bytes.

| Job | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| render | 15.7 | 561774592 | 12620640 |
| main-none | 124.17 | 1293697024 | 33586654 |
| main-advanced | 130.43 | 1403830272 | 25665457 |
| shadow-release | 54.72 | 3999285248 | — |
| shadow-compile | 30.18 | 2229993472 | — |

Main none improves 15.9% from phase25's 147.56 seconds; advanced improves
14.0% from 151.64 seconds. The unguarded isolated prototype measured
124.42 seconds for main and 15.88 seconds for render; the safety checks
preserve its improvement. RSS varies between runs. Advanced remains **2.38 times slower** than shadow release at **35% of its peak RSS**; none is **4.11 times slower** than shadow dev at **58% of its RSS**. The requested speed goal is still unmet.

All three complete outputs are byte-identical to phase25 and pass Node
syntax checking. The 996-file port fingerprint is unchanged. Scope still
differs slightly: shadow main includes render, rasterizer and code-highlight;
light-cljs main includes its five lazy modules, with render measured
separately. Bundling and running Penpot in a browser remain untested.

The executable, tracked patch, untracked name-index source and emitted
JavaScript are retained in `/private/tmp/light-cljs-phase27/production/`.
Source and port fingerprints accompany these results.
