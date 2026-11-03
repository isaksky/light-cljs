# Serial source-derived macro costs

The bounded serial evidence supports proceeding with the snapshot experiment.
It does not require Goose to match V8, and does not establish a compiler speedup.
Two costs are selected: persistent collection copying/scans during invocation,
and the fixture runner's per-job file transport. Keep the flat persistent layout
for the experiment, test removing redundant temporary copies in light-cljs-739,
and leave live transport to the existing compiler adapter task (733).

## Reproduce and scope

From the repository root:

```sh
bb crates/big-boy-pants-js/prototypes/goose/macros.bb
bb crates/big-boy-pants-js/prototypes/goose/macros.bb --measure
bb crates/big-boy-pants-js/prototypes/goose/macros.bb --profile
bb crates/big-boy-pants-js/prototypes/goose/regenerate.bb --check
```

`--measure` builds the existing native Goose runner and an optimized Rust
capture-test executable before collecting samples. The reference binary uses
current embedded `V8Evaluator` and `BbpEvaluator`, synchronously, with one macro
execution worker each. V8 may use its ordinary auxiliary engine threads. The
Babashka coordinator and process launcher do not execute macros concurrently.
No QuickJS backend, parallel scheduler or production evaluator selection changes.

The unchanged actual emitted `churn` factory is called at 0, 16 and 100, once
for warmup, then twice on the same session/closures. The six measured results
sum to 10,836. Every round starts a fresh process/session. There are five rounds,
with engine orders G/V/P, P/V/G, V/G/P, P/G/V, G/P/V. Correctness preflight precedes
the rounds; each sample additionally checks its owned results. One warmup batch
is an explicit short-run policy, **not evidence of saturated V8 JIT warmup**.
There were no concurrent builds/tests from this run during clean samples; no
machine isolation, CPU pinning or statistical confidence claim is made.

Before timing, `macros.bb` checks the existing churn, closure, branch/consumer and
context corpus against independent compiler captures and live Node/V8. It checks
expansion values, supported metadata, origin tokens, exact source bytes/spans,
reused boot/factory/producer counts and context occurrence aliases. The new Rust
reference test obtains the original compiler-owned requests, verifies the emitted
factory and GSP bytes against the committed corpus, then checks complete returned
form records, metadata/origins, source text/spans and value equality after releasing
engine handles. Churn has an empty declared effect trace in both capture profiles;
all three measured implementations execute its original pure body. Effectful
`gensym` remains rejected before mutation by the existing macro tests. This does
not claim Goose execution of the declared-effect fixture.

Implementation: `3eaed058de97f242b21100194aeb5a47a3e93096`, based on
`31cd7abe72015adf969ce67689e4e23c6360982a`; the source VM, form bridge and actual
emitted inputs are unchanged from that base. Corpus source locations/attribution
remain in `corpus/manifest.edn`, `.cljs`, factories and `.jobs.edn`. Churn was
adapted from `benchmarks/native-macros-k.bb`. The final clean timing command ran on this committed implementation; the counter
build uses a separate temporary source copy. Documentation-only changes follow.

On 2026-11-03 UTC: macOS 15.7.3 ARM64 Apple M3 Ultra (Virtual), Goose
v2026.3-267-g369c7a2c/runtime 3025e5810379, Babashka 1.12.218, Rust 1.99.1
(58cf8f9c3), patched V8 crate 152.2.0. Goose uses default `-O2` inlining and
`/usr/bin/cc -O2` (Apple clang 21.0.0), without `--debug`; Rust uses `--release`,
thin LTO, one codegen unit, default features plus `bbp-macros`,
`LIGHT_CLJS_V8_CACHE=require`, `CARGO_BUILD_JOBS=3`. Build parallelism is distinct
from execution workers. Node is an additional correctness check, not the V8
performance reference in this investigation.

## Included phases and results

All numbers below are **milliseconds: median [min, max]**. Raw per-job samples,
including warmup, all 15 cold setups, process walls, teardown and separate counter
rows, are in [serial-samples.edn](serial-samples.edn). Goose's clock has 1 us
resolution: a zero means below resolution, not free work.

| Phase | Goose | Pants | Embedded V8 |
| --- | --- | --- | --- |
| Cold load | .073 [.065,.094] | 1.219 [1.071,1.377] | .094 [.089,.104] |
| Cold boot | .008 [.008,.009] | 3.392 [2.885,3.456] | 7.986 [7.106,8.150] |
| Six imports | .108 [.103,.177] | .015 [.014,.020] | .514 [.423,.550] |
| Six invocations | .604 [.579,.718] | 1.265 [1.234,1.443] | 2.254 [2.223,2.573] |
| Six exports | .012 [.008,.016] | .011 [.008,.017] | .020 [.017,.034] |
| Six file writes | .325 [.294,.414] | n/a | n/a |
| Six handle releases | in job/session scopes | .025 [.023,.033] | .018 [.016,.019] |

| Individual reused invocation | Goose | Pants | Embedded V8 |
| --- | --- | --- | --- |
| churn(0), ten observations | .002 [.001,.002] | .075 [.064,.132] | .031 [.022,.043] |
| churn(16), ten observations | .034 [.028,.051] | .139 [.121,.157] | .183 [.158,.213] |
| churn(100), ten observations | .277 [.246,.357] | .442 [.390,.505] | .932 [.872,1.089] |

Goose load includes GSP I/O, repeated validation, decoding and relocation; boot
includes runtime construction and factory invocation. Reference boot constructs
the actual evaluator/runtime; reference load parses/compiles/executes the source
factory against that runtime. Goose import includes GFB read/decode/validation,
heap import and argument preparation. References import compiler-owned forms and
environments directly. Exports include provenance conversion; Goose additionally
validates and binary-encodes GFB. Output writing is timed separately. Native `Job`
scratch disposal occurs before invocation returns. End-of-session Goose scope
restoration is recorded separately and was below its clock resolution; Rust
teardown timings are in the raw rows. Process wall includes launch and teardown.

Source preparation/export and native compilation are outside these phase totals.
The reference runner reports compiler input preparation separately: Pants median
76.335 ms, V8 31.561 ms. Within that preparation, exporting and checking the GSP
image takes .510 and .682 ms respectively. This is diagnostic source preparation,
not required work charged to either reference engine. Goose packets are prepared
by Babashka before process launch. Therefore the process-wall medians (Goose
5.388 ms, Pants 90.923 ms, V8 53.865 ms) have **different preparation scopes** and
must not be ranked as compiler end-to-end results. Code export/native build are
not repeated per macro invocation.

V8 executes its full JavaScript collection runtime; Pants and Goose use their
documented native leaves. The source-derived factory profiles are compiler
emissions for those runtimes, not identical internal instruction streams. The
short warmup, runtime surfaces, bridge paths and wire formats limit interpretation
of the apparent invocation advantage. The later live compiler adapter and its
end-to-end costs are absent here.

## Cost 1: persistent collection payload work

`source_vm.goose::form_conj` and `form_assoc` copy all current children into a
scratch `Value` array, then `form_collection` copies that array into the append-only
heap. Map assoc/get also linearly probe keys through `form_equal`. The separate
`--profile` command copies the existing Goose sources to its temporary build
folder, adds counters at those explicit append/probe paths, checks/builds that
runner natively and verifies the same forms/provenance. No counters are inserted
into the clean executable. Its wall times are not used as performance samples.

| Per invocation | n=0 | n=16 | n=100 |
| --- | ---: | ---: | ---: |
| VM steps | 55 | 1,671 | 10,155 |
| Charged invocation work | 86 | 2,814 | 46,536 |
| Persistent collection Value copies | 0 | 456 | 15,450 |
| Temporary assoc/conj Value copies | 0 | 408 | 15,150 |
| Map key probes | 0 | 152 | 5,150 |
| All structural equality calls | 1 | 169 | 5,251 |

These counters identify quadratic payload/probe growth: increasing n 6.25 times
raises persistent payload copies 33.9 times, while VM steps rise 6.08 times.
The unchanged emitted churn body, not a synthetic kernel, exercises these paths.
Counts explain a concrete cost source; they do not assign a percentage of CPU
time to dispatch versus copying. The profile adds clocks only in the runner.

`Value` has five 64-bit fields (40 bytes). At n=100, measured explicit payload
copies are 618,000 bytes into retained collection ranges plus 606,000 bytes into
temporary arrays: 1,224,000 bytes of array payload writes. These are **logical
record-copy bytes**, not measured physical memory-bus traffic, RSS, or allocator
reservation totals. Requested payload grows by 618,000 persistent bytes; scratch
requests cumulatively hold 606,000 bytes over the call but their scopes are
reclaimed, so that sum is not peak retained memory. Compiler-elided copies,
frame/Node copying, capacity rounding and allocator metadata are not counted.
After all nine calls, heap pools retain 48,295 Values (1,931,800 payload bytes),
2,385 Nodes, 3,555 source bytes, and other small pools until session disposal.
No allocation interceptor or RSS/peak-memory measurement was performed.

The corresponding Pants path is `src/vm/intrinsics.rs::form_op` →
`collections/form.rs::{assoc,conj,map_assoc}`. Vectors use the 32-way path-copying
`collections/vector.rs`; ordinary maps promote after eight entries to the HAMT
in `collections/map.rs`. Imported positional maps deliberately remain positional
until that promotion. This explains a representation alternative, not evidence
that porting either full trie would improve this bounded Goose workload.

**Decision:** retain the simple flat representation for the snapshot experiment.
Light-cljs-739 tests direct filling of a fresh immutable range to remove the
measured redundant scratch copies, preserving budgets, metadata, old versions and
occurrence identity. It must measure baseline/change with the same real corpus;
no speedup is assumed. A trie, hash index, general builder or collector is not
justified here. Map key scans remain an explicit limitation pending branch
copy-size and larger real compiler evidence.

## Cost 2: fixture transport, not the provenance walk

`macro_run.goose` reads/decodes one GFB file per job and writes one owned result
file with `os::write_file`; `macros.bb` launches the process and decodes those
results after exit. The original export timing combined conversion and writing.
Splitting it disproves the initial hypothesis that export itself dominated:
six warm exports need 8–16 us, while six writes need 294–414 us. Writing often
exceeds invocation for n=0/16; filesystem variability remains visible.
This is fixture transport overhead, not intrinsic macro execution.

The Rust references instead use `MacroEvaluator::{import_macro_form,
import_environment,macro_arguments,export_form,release}` directly. Pants routes
calls through `BbpEvaluator::request` and native task/bridge state; V8 enters its
isolate and JS runtime. Their per-job output needs no intermediate file. There
is no assertion that these transport implementations do equal internal work.

**Decision:** keep files as an inspectable correctness/measurement adapter.
Existing light-cljs-733 owns the long-lived actual compiler connection and
conversion/transport accounting; do not create a duplicate transport pellet.
The current bridge also copies all accumulated source bytes and sizes its ID
map to retained heap nodes on every export (`form_bridge.goose::export_graph`).
Export visits rise from 1,775 to 16,803 over nine calls despite one-node results;
source payloads grow from 487 to 3,711 output bytes. This is an observed memory/
work limitation inside this transport path, but is not a dominant wall-time
cost at these sizes. No separate export optimization is inferred from it.

## Queue and verification

Checked the active queue and relevant closed history (including 713 and the
completed 726/727/728/737). Snapshot/relocation/scheduling/effects/compiler/final
benchmark work stays with 729–735. Created **one of the maximum three** authorized
implementation follow-ups across attempts: **light-cljs-739**, immediately before
729, request ID `goose-738-direct-range-fill-20261103`. No planning pellet.

`macros.bb` correctness, all 15 embedded-reference timing samples, and the separate
native counter build pass. `regenerate.bb --check` passes: two capture tests,
five exporter tests, manifest validation and the complete Goose format/check/
debug/native smoke. Changed Rust/Goose formatting and `git diff --check` pass.
Full compiler/engine suites were not run for this test/diagnostic-only change.

Strict root/workspace formatting retains existing differences in `src/js_syntax.rs`
and engine files listed in the README. Strict all-target Clippy with default plus
`bbp-macros` stops at eight existing engine diagnostics (deprecated fetch_update,
chunks_exact and byte_char_slices); `--no-deps` exposes the existing root byte-char
arrays in compiler_bootstrap.rs and single-element loop in namespace_publication.rs.
QuickJS-only strict all-target Clippy stops at three existing program_fragment
unused items and analyzer's items_after_test_module. Every reported path is byte
identical to base 31cd7abe; no suppressions or unrelated fixes were added.
