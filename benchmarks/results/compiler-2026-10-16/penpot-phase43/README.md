# Embedded V8 feasibility probe

This standalone Rust executable embeds pinned `v8 = 152.2.0`, using the
same phase31 captured Instaparse grammar invocation and phase38/current
runtime as the QuickJS replay. It has no Node runtime dependency. The
production compiler and its dependency manifest are unchanged.

Every trial below reproduces the captured encoded result exactly after the
capture's documented provenance removal. This is one macro invocation,
not full engine conformance or a prediction for a whole build. The replay
uses already emitted sources, excluding Rust compilation callbacks.

| Fresh process | First call ms | Second call ms | Peak RSS bytes |
| --- | ---: | ---: | ---: |
| QuickJS, current retained patches | 9253 | 9276 | 182435840 |
| V8, 4,000,000,000-byte heap limit, run 1 | 1149 | 1081 | 244744192 |
| V8, same, run 2 | 1119 | 1069 | see v8-2.time |
| V8, 64 MiB heap limit | 1463 | 1386 | 150159360 |
| V8, 128 MiB heap limit | 1146 | 1089 | 149536768 |
| V8, JIT disabled, 4,000,000,000-byte heap limit | 5148 | 5154 | 224706560 |

The first two V8 runs preceded the runner's optional environment-variable
switches; they used its unchanged default heap parameters. The archived
runner reproduces those defaults and the subsequent heap/JIT experiments.
All jobs ran separately, after builds completed.

`contracts.rs` verifies direct UTF-16 including isolated surrogates and
NUL, negative zero and NaN, opaque descriptor identity, native-to-JS
compiler callback reentry, explicit promise-job checkpoints, and GC-owned
Rust provenance. Retained inputs keep their payload; unreachable inputs
release it; remaining payloads are released at isolate destruction. All
payload destructors ran on the owner thread with atomic cppgc marking and
sweeping. It also verifies uncatchable external termination, termination
from a near-heap-limit callback, and a bounded custom ArrayBuffer allocator
whose refusal remains observable after JavaScript catches the exception.

These are boundary probes, not the full evaluator contract. The heap
callback reserves 16 MiB emergency headroom to let termination unwind;
`heap_limits` does not establish a strict process-RSS cap. Whole-compiler
integration still needs stack-limit behavior, sticky resource diagnostics,
complete bridge regressions, and the full suite. No backend replacement is
promoted on the strength of these probes. See phase44 for the isolated
compiler experiment that followed.

Reproduce by building `v8-probe` with `cargo build --release`. Its replay
binary takes runtime-source and replay-source paths (fingerprinted here).
Set `V8_PROBE_HEAP_BYTES` for a heap limit or `V8_PROBE_FLAGS=--jitless` for
the interpreter control. Run `contracts RUNTIME`, `contracts heap`, and
`contracts buffers` separately. The large capture, runtime and executables
remain in the temporary directories named in `source-hashes.txt`.

Primary API references: [rusty_v8](https://github.com/denoland/rusty_v8),
[CreateParams](https://docs.rs/v8/152.2.0/v8/struct.CreateParams.html), and
[cppgc](https://docs.rs/v8/152.2.0/v8/cppgc/index.html).
