# Phase14 incremental namespace arity diagnostics

Namespace analysis now retains one diagnostic arity index across source files.
After deferred analysis completes, it adds only the new expressions before
checking that namespace's warnings. Redefinitions and overloads remain a union,
global identities remain distinct across namespaces, and standalone/deferred
analysis keeps its existing declaration range. The regression passed against
the previous implementation before applying the optimization.

Fresh release binary; one benchmark process at a time; no overlapping builds,
tests or profiles. Both shadow jobs delete their generated cache first.

| Job | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 24.20 | 527712256 |
| main none | 180.80 | 1430519808 |
| main advanced | 185.38 | 1522548736 |
| shadow release | 56.72 | 5443452928 |
| shadow compile | 30.19 | 2368634880 |

All 1,380 tests pass, zero failures, 106 ignored. Strict Clippy and formatting
pass. The direct Node workflow logs are reused from phase13 because phase14
changes only Rust diagnostic analysis and its regression. All three generated
application bodies match phase6b byte for byte outside the embedded runtime;
all complete outputs pass Node syntax checking. Penpot application execution
remains untested. The port remains unchanged at 996 files and the recorded hash.

The main advanced build is 11.0 times faster than the original 2,043-second
baseline. It remains 3.27 times slower than this shadow release run at 28% of
its peak RSS. Main none remains 5.99 times slower than shadow dev at 60% of its
RSS. Shadow main also emits render, rasterizer and code-highlight; light main
includes its five lazy modules and render is measured separately. These are
single cold observations, not statistical estimates of JVM peak memory.

Commands and raw results are in results.edn. Production binary, binary diff and
untracked name-index source remain under `/private/tmp/light-cljs-phase14/` and
are fingerprinted in source-hashes.txt.
