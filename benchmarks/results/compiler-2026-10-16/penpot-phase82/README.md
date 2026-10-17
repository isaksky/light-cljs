# Phase82: analyzed-program retention by table

Phase81 diagnostic, now clearing tables in all uniquely owned analyzed programs
before releasing those programs. All 13,367 analyzed program handles are unique.
At this point `Unit` occupies 1,792 bytes, including an optional 1,120-byte
definition snapshot and a 528-byte macro host even after their contents have
been consumed.

| Teardown checkpoint | Live requested Rust bytes |
| --- | ---: |
| before clearing program tables | 647130763 |
| reader contexts cleared | 639527358 |
| namespace tables cleared | 578234951 |
| protocol and associated tables cleared | 574788922 |
| expressions cleared | 391717932 |
| globals cleared | 389355731 |
| locals and recur targets cleared | 374596990 |
| strings and named literals cleared | 352539520 |
| lazy tables replaced with empty defaults | 353605298 |
| program objects cleared | 333392710 |
| catalog cleared | 280914088 |
| bootstrap cleared | 199392992 |
| complete host released | 196537702 |

The largest releases are expressions (183.1 MB) and namespace tables (61.3 MB).
These are releases in the stated order, not independent object sizes. Empty
replacement defaults themselves allocate, accounting for the small increase
at the lazy-table checkpoint. Counters exclude V8/C++ and allocator overhead.

Diagnostic main none takes 20.41 s / 1160167424 peak RSS bytes and matches the
complete phase42 output, including its known main ESM defect. No full suite is
claimed for this instrumentation. The patch is relative to phase81.
