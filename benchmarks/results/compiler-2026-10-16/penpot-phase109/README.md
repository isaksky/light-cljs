# Backing-store budgets — validated, still isolated

The isolated V8 backend now accounts for fixed ArrayBuffers and
SharedArrayBuffers, resizable/growable buffers, and WebAssembly memory. This
includes construction, in-place growth, shrinkage, mixed aggregate backing
storage, and release. The quota charges committed pages rather than reserved
virtual capacity. Shared allocator ownership keeps the accounting alive even
when a native backing-store reference outlives its isolate.

The engine patch adds opt-in allocator callbacks at V8's backing-store boundary.
Ordinary allocations retain V8's garbage-collection retries before failure
becomes terminal. Callbacks only update Rust atomics; they do not reenter V8.
The patched engine is rebuilt from the pinned phase108 sources, then frozen
together with its matching generated bindings. No binary or archive surgery is
used. The compiler adapter and its vendored public header match that library.

The negative control reproduces **13 quota bypasses** in the unmodified engine
under a 32 MiB limit. All now return the owned memory-limit error. Nine new tests
also cover caught failure followed by an infinite loop, startup, the Wasm
`memory.grow` instruction, ordinary catchable RangeErrors, getter/coercion order,
zero-filled growth, shrink refunds, garbage-collection retries, and native
backing-store lifetime after isolate disposal.

Full validation passes **1,435 tests, zero failures, 106 ignored** at normal
concurrency, strict Clippy, formatting, and all three Node workflows. All three
Penpot outputs match phase108 byte-for-byte and pass explicit ESM syntax checks.
The 996-file port fingerprint is unchanged. These checks do not execute the
bundled application in a browser.

Sequential cold builds, without overlapping CPU-heavy work:

| Build | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.89 s | 488,226,816 |
| main none | 24.19 s | 966,918,144 |
| main advanced | 29.05 s | 1,185,284,096 |
| Shadow release | 60.08 s | 4,129,030,144 |
| Shadow compile | 33.08 s | 2,197,323,776 |

An immediate frozen phase108 main takes 22.80 s / 954,761,216 bytes. Because the
first candidate run was slower, two more alternating comparisons were run:

| Run | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| candidate repeat 1 | 22.99 s | 958,988,288 |
| baseline repeat 1 | 23.15 s | 953,729,024 |
| candidate repeat 2 | 22.93 s | 952,926,208 |
| baseline repeat 2 | 23.09 s | 963,936,256 |

All repeated outputs match. The three-run medians are 22.99 s for the candidate
and 23.09 s for the baseline; this does not establish a meaningful speed change
from the quota patch. The candidate median is 30.5% below fresh Shadow dev time,
at about 44% of its RSS.

This closes the demonstrated backing-store bypasses. Heap and backing storage
still have separate budgets; their combined session limit is the next check.
Backend packaging also remains incomplete. Production remains phase101 and the
overall dramatic-speed goal remains open.

For reproduction, restore the pinned phase108 engine inputs, run `prepare.bb`,
apply `engine-budget.patch` in the engine tree and `compiler-budget.patch` in
the project, then run `build-source.bb` and `validate-and-bench.bb`. The separate
Shadow runs use `bb benchmarks/penpot.bb OUTPUT shadow-release shadow-compile`;
`compare-main.bb` records the repeated frozen-binary comparison. The scripts,
patches, logs, environments, hashes, and port fingerprint accompany this file.
