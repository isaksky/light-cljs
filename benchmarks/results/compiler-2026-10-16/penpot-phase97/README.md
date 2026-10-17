# Phase97: bounded V8 native stack

The isolated candidate records the owning thread's OS stack bounds, installs an
initial V8 stack limit and refreshes it at each outer engine operation. Nested
compiler callbacks share that operation's boundary. A 64 KiB native reserve
remains available for unwinding. Zero and oversized configured limits are
clamped to the thread's bounds; an unusably small limit returns an owned error.

The pinned Rust binding lacks `Isolate::SetStackLimit`. A small C++ shim calls
that public API using the matching V8 15.2.124.1 headers from rusty_v8 152.2.0.
`v8_thread_stack.cc` is separate so the OS queries can be tested without linking
an engine. To reconstruct the candidate, apply `changes.patch` to phase96 and
copy the pinned crate's `v8/include` directory to `vendor/v8-headers/include`.
The exact header hashes and upstream license are preserved.

Final, normal-concurrency validation: **1,416 passed, zero failures, 106 ignored**;
strict Clippy, formatting and all three Node workflows pass. Four new tests
cover recursive JavaScript recovery, deeper Rust entry, oversized budgets on
small native stacks, and recursive compiler callbacks sharing the outer budget.
The full checks were repeated after splitting the native translation units.

Native boundary probes also pass on Linux ARM64 (GCC 16.1.1) and Windows x64
(UCRT64 GCC 16.2.0), exit 0. They compile the exact V8 shim and execute the exact
OS boundary helper on main and one-MiB worker threads through 256 KiB of nested
native frames. These are not full V8 runtime or compiler-suite runs on those
platforms. Commands, ownership records and downloaded logs are included.

| Cold build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.71 | 514015232 |
| main none | 23.13 | 1123794944 |
| main advanced | 29.30 | 1235697664 |

An immediate sequential comparison takes **22.64 s / 1114357760 RSS bytes** for
the frozen phase96 binary and **22.67 s / 1127350272 bytes** for phase97. This pair
shows no meaningful stack-limit cost; the earlier 21.05 versus 23.13 comparison
alone does not isolate that cost. Both comparison outputs also match phase42.

All complete outputs match phase42 and the 996-file port is unchanged. Explicit
ESM checking passes render and reproduces the known main defect. No bundling or
application execution is claimed. The frozen binary contains neither earlier
QuickJS sampling nor macro-timing instrumentation.

This candidate remains isolated. Production sources still exactly match the
validated phase87 QuickJS candidate. V8 backing-store memory limits, startup
monitoring, scheduler creation failure and packaging remain adoption work;
the dramatic speed goal is not met.

Intermediate native probe failures are retained: GCC warnings inside upstream
headers, a redundant Windows-version macro in the test driver, and Windows
linking of an otherwise unused V8 symbol. The final header warning suppression
is scoped to the upstream include; the final probe links the independent OS
helper while compiling the V8 shim separately.

The Windows probe's `committed` label is misleading: it prints the span returned
by `GetCurrentThreadStackLimits`, which reports system-allocated stack boundaries,
not physical committed bytes ([Microsoft documentation](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getcurrentthreadstacklimits)).
The measured spans equal the queried reservation sizes. The final shim comment
was clarified after validation; no executable code changed.
