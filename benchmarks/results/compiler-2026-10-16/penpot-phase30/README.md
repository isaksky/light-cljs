# Compiler-global read diagnostic

This phase only instruments reads through `__lightCompilerRoots`; no cache
or production source change was retained. Both complete outputs match phase29.

| Workload | Reads | Distinct names | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: |
| render | 32,408 | 94 | 15.12 | 499040256 |
| main | 1,307,500 | 245 | 120.09 | 1329332224 |

A native QuickJS microbenchmark performs one million already initialized
root reads in 240 milliseconds. Extrapolation puts main's reads near 0.31
seconds, subject to differences in real receivers and cache state. This is
too small to justify another production cache. These instrumented runs are
diagnostics, not the final clean comparison.

The instrumented executable and complete emitted outputs remain under
`/private/tmp/light-cljs-phase30/`. The scripts, counters and timing logs are
retained here.
