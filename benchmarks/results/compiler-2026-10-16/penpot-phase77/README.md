# Phase77: live allocation bytes around macro-session release

Diagnostic version of phase76. A wrapper around mimalloc counts requested bytes
passing through the Rust global allocator. It excludes V8/C++ allocation,
allocator padding/metadata and reserved address space. These are not RSS values.

| Checkpoint | Live requested bytes | Peak requested bytes | Cumulative requested bytes |
| --- | ---: | ---: | ---: |
| after analysis, session retained | 647918028 | 658056060 | 15226118119 |
| after macro-session release | 196537678 | 658056060 | 15226118504 |
| after output preparation | 197752988 | 658056060 | 15261238279 |
| after output emission | 264959850 | 658056060 | 15744917517 |

Releasing the session frees about **451 MB** of allocations through the Rust
allocator. That is a significant retained compiler-state cost, separate from
V8's roughly 27 MB live managed heap after collection. Program tables contain
1,220,906 expressions, 110,119 locals, 13,392 globals, 336,622 strings and
204,735 named literals.

The instrumented run takes 21.07 s / 1183498240 peak RSS bytes and produces
complete output identical to phase42, including its known main ESM defect.
Its timing is diagnostic; no full suite is claimed for the instrumentation.
Production remains QuickJS phase72. Patch, scripts, exact command and counters
are preserved alongside source/output hashes.
