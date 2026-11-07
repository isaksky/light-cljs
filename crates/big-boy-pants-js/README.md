# big-boy-pants-js (bbp)

bbp is light-cljs's JavaScript runtime for concurrent compile-time execution:
it runs the JavaScript that macros and helpers compile to, on isolated
workers, with explicit snapshots instead of a shared heap. It is implemented
in [Goose](https://github.com/aardappel/goose); the Pellets backlog holds the work toward a Goose
compiler path with no V8 (group `goose-generated-200k`) and essentially complete
JavaScript (group `goose-javascript`). Read [AGENTS.md](AGENTS.md) first.

## Layout

| Path | Contents |
| --- | --- |
| `src/` | The Goose runtime: VM, snapshots, scheduler, compiler session, form bridge, and the `*_tests.goose` test programs beside the modules they import |
| `corpus/` | Captured bytecode images, jobs and expectations used by the tests (`corpus/manifest.edn` records their sources) |
| `scripts/` | Babashka entry points: `smoke.bb`, `compiler.bb`, `correctness.bb`, `regenerate.bb`, `conformance.bb`, benchmarks |
| `conformance/` | The JavaScript conformance harness's area map (`areas.edn`), expected Goose passes (`expected-pass.edn`) and bbp-authored cases (`cases/`); see `scripts/conformance.bb` |
| `evidence/` | Measurement reports and raw archives |
| `docs/` | Design notes |
| `rust/` | The first, Rust implementation (package `big-boy-pants-js`). Its parser and bytecode compiler still produce what Goose executes (`rust/src/frontend/goose.rs` exports it), and its VM is the reference oracle for porting JavaScript semantics. See [rust/README.md](rust/README.md). |

The Rust glue that connects light-cljs to the Goose runtime lives in the
compiler, under `src/goose/` at the repository root.

The evidence archives and sample files were recorded before this layout
(November 4, 2026) and keep their original `crates/big-boy-pants-js/prototypes/goose/` paths.

## Goose runtime notes

The [generated 200k target](evidence/generated-200k.md) pins the application-scale
acceptance workload, its execution boundary and original gap inventory.
The [full-project baseline and reusable comparison command](evidence/generated-200k-baseline.md)
record exact V8/Goose 1/2/4/8 application checks and the quiet V8 reference.
The [execution-support recheck](evidence/generated-200k-probes.md#execution-support-recheck)
maps the eleven reached macro families to current implementation and tests.
The [full-session safety recheck](evidence/generated-200k-probes.md#full-session-safety-and-lifetime-recheck)
measures retention, native memory, actual execution overlap and lifecycle bounds.
The [complete-project form audit](evidence/generated-200k-probes.md#complete-project-forms-and-teardown-verification)
checks every expansion after teardown in both native modes at 1/2/4/8,
with explicit V8 provenance ambiguities and independent Goose occurrence checks.
The [full-project performance report](evidence/generated-200k-performance.md) records the
five-round V8/Goose matrix, complete phase/concurrency diagnostics, memory scopes
and separately labelled cache/leaf behavior. Goose4 is the best Goose cell but
remains slower than V8 on the pinned 200k project.
The [final conversation audit](evidence/generated-200k.md#final-conversation-audit)
maps the delivered milestone to both source conversations, reviewed commits,
verification evidence, remaining limitations and workflow recommendations.

From the repository root, run:

```sh
bb crates/big-boy-pants-js/scripts/regenerate.bb
bb crates/big-boy-pants-js/scripts/regenerate.bb --check
```

The first command regenerates deterministic source artifacts, independent
V8/Pants expansion and output expectations, bytecode, metadata and SHA-256
manifest. The second checks without changing artifacts. Both validate metadata,
test the Rust exporter, and run Goose format/check/debug/native checks and
inspection of every binary. They require Rust and the installed V8 cache, Node,
Babashka and Goose. Builds stay inside this checkout; temporary native outputs
are removed on success or failure. For the fast Goose-only command:

```sh
bb crates/big-boy-pants-js/scripts/smoke.bb
```

No thread program builds more than one `source::Heap` (light-cljs-794), so a
VM image can later live in global index-typed tables. A test `main` that needs
several heaps builds none: it runs each heap's function as a thread program
(`src/heap_program.goose` carries the session-identity counter between them)
and passes snapshots and handles between them as flat messages. Those tests
therefore run as native `--debug` and native builds. In `compiler_session.goose`
the coordinator's base image is its only heap; definition preparation and the
`profile` mode's restore run in programs it spawns and joins, and the
scheduler prototype runs each job's heap in a program of its own.

`src/inspect.goose` accepts `.gsp` paths as arguments. Scripts resolve their own
locations, so they work from any working directory.
Validated Goose: `v2026.3-267-g369c7a2c (runtime 3025e5810379)`.

The wrapper subset retains all actual arguments in bounded job-local storage,
including excess arguments, through yields and helper suspension. `NewArguments`
copies them to an independently owned, read-only unmapped object; `Rest` builds
a fresh ordinary array. Indexed reads and `length` work; arguments mutation,
`callee`, and prototype inspection fail explicitly. Export uses the frontend's
strict mode, so mapped sloppy arguments are outside this subset.
Arguments objects compare by identity, including as form map keys/set members;
separate calls with the same arguments remain distinct.

Callable `.call` forwards the exact receiver, including omitted `undefined`
and explicit `null`, to ordinary source functions and the supported runtime
leaves. Each heap owns one immutable call method identity, relocated by the
version-2 snapshot header. Function property writes and other prototype behavior
remain unsupported. `fnApplyNull` copies a closure with its immutable Clojure
apply receiver convention; `fnApplyReceiver` observes that convention. These
operations do not add general `apply` support.

Syntax-quote assembly supports bounded eager `cljConcat`/`cljConcatAll` over
nil, lists, vectors and existing sequence cursors. Copies retain child
occurrence handles and leave inputs immutable. The one-argument `core.sequence`
leaf preserves list/cursor identity and converts vectors or nil through the
existing sequence implementation. Map/set membership tests keys independently
of their false/nil values; vector/string membership tests integral indices.
Other collection categories fail explicitly.

`core.mapSeq(callback, input)` defers callbacks for one finite nonchunked list,
cursor or nil input, the shape of the generated sum-all rest argument. It
reserves at most the remaining heap value budget (default 1,048,576 slots).
`cljConcat`, `cljConcatAll`, count, vec and into fully realize it in source order;
`sequence` preserves its identity without demand. Successful callbacks are
memoized, including across a caught throw and retry. Completed aliases share
the same cursor and child occurrences. Callback frames and the memo range
survive helper suspension and private snapshot relocation; work, instruction,
frame, memory and cancellation bounds apply throughout. Partial lazy protocol
operations, chunked/vector sources, multiple sources, nested lazy sources and
transducers are explicit boundaries. Returning an unrealized map directly
through the forms bridge is also unsupported. This is a finite full-demand
slice for syntax-quote assembly, rather than a general lazy-sequence runtime.

Temporary JS arrays support numeric integral `length` writes, bounded by both
the JS uint32 range and the heap value limit. Shrinking unlinks indexed own
properties; extension creates undefined holes and never revives removed values.
Removed records remain charged to the property limit until teardown. A work
rejection during truncation leaves the array unchanged. Coercing nonnumeric
lengths and invalid lengths fail `UnsupportedOperand`; lengths above the heap
bound fail `MemoryLimit`. Snapshot version 2 retains the same layout, validates
array bounds and admits the `sequence` builtin.

The compiler regression loads the unchanged pinned generated macro namespace
and compares 26 calls across all eleven macro families against V8, including
metadata and returned protocol/record forms, after teardown at 1/2/4/8 workers.
Its additional repeated-number cases pin an
existing reference limitation: V8's value-based scalar provenance assigns all
equal numbers the last input span. Goose retains each occurrence's original
span; the test checks those exact spans and complete equality across Goose
worker counts instead of normalizing away their identities.

The same regression now executes the actual `zalstok`/`favoukstun` callable
path with zero, one and several body forms, metadata and nested syntax quote.
Its cached preparation exports the canonical helper unchanged, including its
captured runtime, `rt.core.list`, `rt.symbol` and `.call`. Goose executes both
the factory and returned function; the retained compiler bootstrap service
continues to supply code and resolution. Complete V8 forms are compared after
teardown at 1/2/4/8 workers. The concurrent compiler fixture covers both form
and callable helpers, retaining one producer for eight consumers.

`callable.goose` uses the checked snapshot codec for a versioned local callable
publication. It validates the entire producer code/heap image, including dead
nested functions, checks the frozen base code and reachable runtime records,
and appends the complete helper code suffix with relocated function, constant,
child, instruction and source indices. It installs only reachable immutable
closure/cell/form records. Runtime handles reconnect to the consumer's own
checked base; occurrence, metadata and capture aliases remain distinct where
required. Plain private objects/arrays, mutable or uninitialized cells, lazy
maps and cycles through forms reject before installation. Closure/cell cycles
are admitted. String/source pools are copied, including unused ranges; this is
bounded copying, not a compacting collector. The full publication is retained
once by the coordinator under its existing 64 MiB budget. Validation and staged
copies use the existing ten-million-work and 64 MiB byte controls; no quota
was increased.

Each private heap caches an imported callable by the authoritative helper unit
key. A second root name retains the same function identity, including after
another helper suspension. Import runs no JavaScript and does not replay the
producer. Tests preserve mutations across nested helper frames, caught throws
and repeated private calls, and check cancellation and full pool teardown.
Callable classification covers ordinary closures and the existing finite
keyword/symbol/vector/map/set forms; ordinary functions retain `.call` receiver
semantics. Regex, protocol and multimethod invocation remain explicit native
boundaries with emitted instruction context.

Validated with Goose `v2026.3-267-g369c7a2c` / runtime `3025e5810379`,
Rust 1.99.1, Node 26.12.1 and Babashka 1.12.218 using `correctness.bb`
(including `regenerate.bb --check`, smoke, 45 typed rows per mode, 35 compiler
tests per mode and three QuickJS bridge tests). The final compiler command
also covers callable import accounting. Engine all-target and default/QuickJS
Goose feature checks pass. Strict Clippy still rejects unchanged engine/root
code, including three additional engine test diagnostics (`mutable_key_type`,
`chunks_exact_to_as_chunks`, unused `dep::Wakeups`); the existing root formatter
differences remain. The full compiler/engine suites and full 200k build are
separate from this bounded callable check.

[Runtime design decisions](docs/runtime-design.md) trace the captured closure and
persistent-form requirements through QuickJS/Pants and explain the indexed
storage choices. The smoke command includes `layout_probe.goose`, a bounded
layout/lifetime probe; it does not execute macros or measure performance.

## One job worker per job

Since light-cljs-784 the live `compiler_session.goose` runs every dispatch (a
macro start, a helper production, a resume of a saved continuation) in a
contained job worker spawned for it by the coordinator, which owns the session
image: the realm, the base snapshot and the prepared definition snapshots. A
job worker starts with empty VM tables, restores the flat snapshot its spawn
copied, and its exit frees everything the job allocated; nothing is reset or
reused between jobs. `limit + retention` reapers only join job workers and
report how each ended; the coordinator never has more workers live than
reapers, so every join is taken at once and a contained abort is reported
while its peers run. Total native threads are the coordinator, the input
thread, the reapers and one per job ever spawned; at most `2 + 2 * (limit +
retention)` are alive at once.

At most `limit` workers execute. A dependency wait releases its execution
slot. Within the retention budget (`BBP_RETAINED_WORKERS`, default 32, at
most 128) the worker blocks with its state in its own thread program until
the coordinator answers it through the `Answer` queue (routed by worker
address; a worker puts back an answer for another). Over the budget the
coordinator tells it to save a flat continuation and exit, as every wait did
before; a fresh worker restores it later, without replaying the suspended
body's mutations. Ready continuations/helpers have admission priority; new
jobs fill remaining slots. Existing VM node/value/property/string/work/step/
stack budgets, 64 MiB snapshot and coordinator retained-state budgets, 16 MiB
bridge budget, 128 waiters and 1024 queued requests/cached definitions bound
live and retained data.

Tracing modes print a final `JOBS` line (retention, job workers spawned,
final publications, spawn, start and join nanoseconds, spawn-argument and
restore bytes, peak live and blocked workers, waits, saves, answers, and live
and blocked workers left, which must be zero) and `THREADS` (created, peak,
joined). Instrumented runs add one `CENSUS` line per worker exit (table rows,
restored rows and unreachable rows; `census.goose`) and a `TRIM` line. The
measurements are in [the 200k evidence](evidence/generated-200k-performance.md),
"One job worker per job".

A contained worker abort poisons the whole session: its reaper reports an
owned diagnostic, and the coordinator stops admission, cancels survivors
(running or blocked) between bounded VM slices, releases the aborted worker's
execution or retention slot, and joins every worker and reaper. No failed
worker is replaced and no effectful job is replayed. Normal close and
cancellation also reap every worker; the Rust process watchdog remains the
backstop for failures outside Goose's containment contract.

See [expanded results](evidence/expanded-results.md) for exact predecessor comparisons
and the unchanged actual compiler fixture. The older results below describe
the earlier per-chunk execution implementation.

## Intent retrospective and demonstration (2026-11-03)

The [original conversation](codex://threads/01a19f3b-37e9-79e3-8799-4751884be0f0)
proposed a small replacement design after Pants failed its performance goal:
initialize a representative macro environment once, branch into private local
VMs, reconnect immutable results, and compare complete costs with one-thread V8.
The experiment now exercises that bounded design through the actual compiler,
but branching still loses on the measured corpus. This is evidence about this
implementation and workload, not a conclusive rejection of Goose designs.

The lean branch/worktree and indexed storage follow that intent. light-cljs
still reads/resolves CLJS, emits JavaScript, orchestrates macro expansion and
analyzes results. The existing Rust JS frontend exports code; Goose executes
the selected emitted macro/helper bodies and owns their VM heaps. Compiler core
syntax/bootstrap services remain in the compiler. Closures, collection/form
construction and traversal, contextual metadata/provenance, and actual helper
fan-in are demonstrated. Returned forms survive session teardown. Explicit
snapshots copy allocator/state tables into isolated heaps; immutable publication
relocates IDs rather than merging arbitrary private mutations. See the
[live adapter](#live-compiler-source-macro-adapter) and
[artifact boundary](#immutable-branch-artifacts) for exact contracts.

The earlier completion summary was too confident: isolated mechanism tests did
not establish that initialized-definition reuse and progress-aware scheduling
had reached the live compiler path. The intent audit found both gaps:

- The adapter captured a booted empty base but re-exported, loaded and executed
  each macro factory on every call. `initializations == 1` counted base boot,
  not factory reuse. The [definition reuse follow-up](evidence/branching-results.md#initialized-definition-reuse)
  now exports each exact compiler-owned definition and prepares each macro once
  per compilation, including concurrent demand. Each call restores the
  initialized callable into a private heap and executes its real body; contexts,
  captured mutation and fresh sessions remain isolated.
- The live coordinator slept after useful progress and admitted work one turn
  at a time, unlike the isolated scheduler. The [dispatch follow-up](evidence/branching-results.md#ready-work-drain-in-the-live-compiler-session-2026-11-03)
  drains ready commands/completions, fills bounded execution slots and backs off
  only when idle. It retains one native thread per execution chunk. Reentrant
  helper demand saves explicit continuation state and joins the execution owner
  before the callback, allowing one-worker nested calls without replay.

The latest paired larger/eight-slot compiler wall improves from **143.48 to
114.05 ms**, versus **53.63 ms V8** and **95.70 ms Pants at eight slots**;
retained one-heap Goose serial takes **61.26 ms**. These are complete process
medians from the dispatch report, including setup and cleanup. Each follow-up
retains 450 measured comparisons in revision pairs and a separate 45-cell
allocation run. The corrected two-batch fixture includes six namespace drain
barriers; its timings must not be spliced into the initial branching matrix.
Historical reports/raw samples remain intact, phase costs overlap, and
coordinator sleep cannot be subtracted from wall time. No engine win is promised.

For a practical demo, from the repository root with Babashka, the validated
Goose compiler, Rust/C toolchain, Node and the required patched V8 cache:

```sh
bb crates/big-boy-pants-js/scripts/compiler.bb
```

This builds debug/native runners and executes the 35 focused tests per mode:
actual corpus expansion against V8, deterministic application JS/output,
provenance, reuse/isolation, one-worker reentrancy, 1/2/4/8 execution slots and
failure/cleanup checks. Read the [committed results](evidence/branching-results.md) for
performance; the full quiet benchmark matrix is unnecessary for this demo.
This remains an opt-in finite JS/runtime subset: unsupported selected jobs fail
explicitly; live shared compile-time effects,
complete ECMAScript/cljs.core, general tracing GC and production migration are
outside its surface. Isolated effect/artifact tests do not extend that surface.
Prior strict-lint/root-format and full-suite wasm limitations remain documented
in the report; this documentation change reuses the existing execution evidence.

The protected original-checkout note at
`/Users/admin/src/light-cljs/crates/big-boy-pants-js/rust/docs/goose-prototype.md`
was absent for an unknown reason. It was restored using exclusive creation from
the recovered assistant response the user said they copied verbatim in the
linked conversation. Byte equality with that recovered text was verified; the
note remains untracked, with original `master` HEAD and all other status
unchanged. This establishes recovery from the conversation, not an independent
proof of the missing file's historical bytes.

## Corpus and coverage

`capture.rs` runs the actual compiler with test-only hooks at deferred/native
unit emission and successful expansion. Normal builds have neither the hooks
nor recorders. Factories come from `MacroCode.unit`, with the exact
`rt => rt.core.macroReaderFunction((UNIT)(rt))` wrapper used at
`source_macro_function`; helper factories come from the emitter. Reference
engines invoke the macro bodies. Compiler-owned expansion values and provenance
are compared after both hosts and their engine-owned values have died. Node
separately checks application output.

| Source | Reference work | Goose coverage |
| --- | --- | --- |
| `churn.cljs` | Collection-heavy `benchmarks/native-macros-k.bb` loop at 0/16/100: vector conj, map assoc/get, seq traversal, rest/count; `0 168 5250` | Unchanged emitted Pants bodies execute natively across reused calls |
| `closure.cljs` | `mapv` invokes a helper closure capturing a computed offset; `[[1 11] [2 12] [3 13]]` | Source callbacks execute with explicit native iterator continuations |
| `branches.cljs` | Independent expansions initialize two definitions; a consumer demands both; exactly two producer factories execute | All three macro bodies and both helper factories execute in one native session |
| `context.cljs` | `&form`, `&env :ns :name`, metadata, nested symbols/keywords/maps, distinct equal scalar occurrences | Native expansion/provenance matches references; returned input and metadata aliases checked |
| `effect.cljs` | One declared compiler-gensym effect; result proves it returned a symbol in references | Serial calls reject without authority; declared coordinator execution returns `(quote [7 true])` |
| Historical arithmetic/control source | Results 27.5 and 7307 | Source-derived entries execute debug/native |
| Generated wide source | 300 constants/locals, code and branch addresses above 255; result 299 | Executes debug/native |
| Generated Unicode source | UTF-8 emoji and a lone UTF-16 surrogate | Exact UTF-16 source execution and owned form round-trip |

Historical scripts are copied unchanged from
`7ba4bd6f:tests/fixtures/js/{arithmetic,control_flow}.js`, retaining their
comments. The existing parser's final-expression span inserts an entry-function
`return`; both original and adapted sources are retained. `compile_function`
here proves entry-body execution only. Bootstrap invariants adapt `cc764dbe`,
`88fc62d3`, `99d4f9fb` and `1ddd5633` (repository-root paths at those commits).

**Parsed, exported and executed are distinct.** Per-factory inventories mark
parsed/exported true and Goose-executed false. Export walks every function,
including dead functions, and exports every opcode; an operation the Goose VM
does not implement, such as a constructor, fails only when it runs, with
`Fault.Unsupported` naming it (a direct eval of the global `eval` names
`direct_eval`).
These flags describe the export stage. `macros.bb` separately executes the
supported Pants macro images and compares structured results. The legacy scalar
adapter still rejects them. The exporter accepts source alone and cannot
evaluate jobs or compute expansions. Reference generation supplies independent
expectations.

The captured syntax surface includes arrow/ordinary function expressions,
closures, lexical bindings, labeled while/continue, if/ternary/nullish and logical
branches, assignments/comma expressions, member/index reads, ordinary/method
calls, arrays and scalar/string literals. Native form constructors and form
selectors lower directly. Per-factory inventories enumerate the resulting
operations and full function/instruction counts.

## Runtime and native boundaries

`runtime.edn` records conservative roots found mechanically in compiled string
constants. Core names resolve through `RuntimeManifest`, other exported names
through its exports; `reached_by_uses(Call)` selects transitive units. A data
string matching a runtime name can retain extra units. The broad `core` hub
is excluded as a root. `runtime.js` preserves selected original bodies in
initialization order and is independently instantiated in V8. Unit byte ranges
refer to the hashed `src/js_runtime.js`. Native counterparts are retained in the hashed
`src/bbp_runtime.js` and `src/bbp_reader.js`, named explicitly in the inventory.

The Pants profile uses explicit form-operation leaves (listed in `runtime.edn`)
in place of JavaScript private stores/brands. Remaining core calls, including
`mapv`, `inc`, `into`, `vec`, `gensym`, symbol testing and the reader wrapper,
are identified by selected core bindings. Dynamic reader services and unresolved
definition demand remain explicit boundaries; gensym requires the declared
coordinator contract described below. The serial
slice below supports already-resolved roots and owned source provenance.
No host evaluates a Goose job.
Selected runtime source is reference material, **not executable Goose export**;
its classes, private stores, protocol/reader machinery and host hooks are outside
the scalar VM. Complete ECMAScript/cljs.core are excluded.

## GSP version 3

Integers are little endian. An image owns one exact immutable UTF-8 source
buffer (source ID 0), fixed-stride tables and variable payload pools. There are
no host pointers, owning roots or runtime handles. Runtime strings/names are
exact UTF-16 code-unit ranges, including lone surrogates, separate from source
UTF-8. Source spans are half-open **byte** ranges. Diagnostic line/column work
is outside ordinary execution.

The 32-byte header is `GSP\0`, `u32 version=3`, then six u32 lengths: source
bytes, functions, constants, child IDs, instructions, UTF-16 units. Sections
follow in that order:

| Table | Record | Fields |
| --- | --- | --- |
| Functions | 13 × u32, 52 bytes | instruction start/count; constant start/count; child start/count; params; locals; captures; name-unit start/count; JS length; flags |
| Constants | 3 × u32, 12 bytes | tag and two words: undefined=0, null=1, boolean=2, f64 bits=3, UTF-16 range=4, well-known Symbol index=6, BigInt UTF-16 range=7 (5 is never used) |
| Child IDs | u32 | Absolute function IDs; function operands index their parent's child range |
| Instructions | 5 × u32, 20 bytes | descriptor; operand; source-byte start/end; original function-code byte offset |
| UTF-16 | u16 | Runtime string/name code units |

Descriptor bits 0..7 are the engine opcode, 8..15 the operand category (none,
constant/name, local, byte target, child, capture, selector/count), 16..23 the
original operand width (0/2/4); bit 24 marks the call of a direct-eval site
and bits 25..31 are zero. Every opcode of the shared registry
(`rust/src/vm/instr.rs`) exports, implemented by the Goose VM or not
(light-cljs-783); one the VM does not implement fails the job when it runs,
with the uncatchable `Fault.Unsupported` naming it, and a dead one never fails.
Flags contain strictness in bit 0, the constructor kind in bits 8..15 (none,
ordinary, base class, derived class, default derived class), generator in bit
16 and async in bit 17 (the function's code contains `GeneratorStart`,
`AsyncStart` or `AsyncGeneratorStart`). A Symbol constant is a well-known
symbol, its first word the index in `export_well_known_symbol` (the frontend
emits only `Symbol.unscopables`); a BigInt constant's range holds the sign (0
or 1) then the magnitude in little-endian 16-bit digits with no high zero
digit. Those stayed version 2: new values of existing fields, which earlier
readers refuse explicitly. Version 3 (light-cljs-823) adds descriptor bit 24:
the exporter sets it on the call at each direct-eval site's offset, which is
always `call`, `call_method`, `call_spread` or `call_method_spread`
(`export_direct_eval_call`); the sites' scope descriptors are not exported.
The VM decodes each marked call to `Ins.DirectEval`, which
fails `Fault.Unsupported` naming `direct_eval` when its callee is the realm's
%eval% and is an ordinary call otherwise.
`export_schema.goose` is compiled reader code generated from the registry
(descriptors, selectors, name operands, direct-eval call opcodes, each
opcode's stack effect and control flow, opcode and well-known symbol names);
an input image cannot supply its own schema. No source behavior is dropped.

The existing lowerer supplies u16 constant/local/capture/child indices and u32
byte addresses. GSP table IDs/ranges are u32. Budgets: 8 MiB source, 65,536
functions, 1,048,576 records per other pool, function-tree depth 256. Constants
and child tables retain the lowerer's 65,536-entry bound; per-function
locals/parameters/captures are at most 65,535. IDs survive table growth.

The Goose inspector checks framing, known descriptors/selectors, UTF-8 and span
boundaries, ranges/references, child ownership/depth, instruction coverage and
byte offsets, and branch boundaries before installation. Failure preserves
every byte of an already valid image. Inspection is separate from execution:
engine lowering validates stack flow; the scalar adapter additionally runs the
Goose scalar VM's own stack validator before execution.

## Companion forms, definitions and jobs

Each `*.jobs.edn` is a version-2 metadata owner containing:

- `sources`: exact UTF-8 bytes once per compiler source identity; IDs are assigned
  by first occurrence and process-local SourceIds never escape.
- `data`: indexed occurrence nodes, flat child-ID and UTF-16 pools; tags preserve
  i64 integers, f64 bits, ratios, characters, names, compiler literals and metadata.
  Equal values do not erase occurrence IDs.
- `definitions`: macro name/generation, source/span and factory;
  `definition-uses`: analyzed/emitted generations;
  `producer-executions`: actual compile-time producer executions.
- `jobs`: correlated IDs, macro names, call/environment datum IDs, argument IDs
  into the input form table (after the operator), the form/env ABI, and separate
  reference expectations. Hidden arguments are the full input form and environment.
  Form records retain root IDs, flat child ranges,
  metadata/payload IDs, source IDs, byte spans and origins.
- `helper-factories` and `declared-effects`: demanded helper count and ordered
  compiler-gensym authority requirement. The original branch source declares
  the producer definitions and the consumer that reads them.

`regenerate.bb` validates references/ranges, acyclic bounded form/data graphs,
scalar types, argument occurrence IDs, source boundaries and UTF-16 values.
Negative cases reject cycles, unknown tags and overflow before narrowing.
Metadata budgets are 100,000 datum nodes, depth 128 and 1,048,576 child
IDs/code units. Records are compiler-owned independently of any evaluator or
Goose session. The manifest pins the starting compiler revision and content
hashes; current Git HEAD is deliberately excluded so committing regeneration
outputs does not change them.

## Scalar migration and ownership

Version 2 widens scalar constant/local operands to **u16** and branches to
**u32**; code is bounded at 16 MiB, tables at 65,536. `emit` checks wide integers
before narrowing or mutation. `load(..., version=1)` explicitly rejects obsolete
encoding. `encoding.goose` migrates only the historical hand-assembled test
notation and never loads source images.

The 17 bootstrap operations retain numeric/identity/resumption/effect behavior.
Source smoke adds division/remainder, negation, explicit numeric checks and
comparisons. String/object coercion fails explicitly. `scalar.goose` rejects
calls, closures, strings/forms and source host selectors before installation;
bootstrap effect opcode 17 cannot alias a source host-operation selector.

Program owns instructions/constants; Job borrows it immutably and owns locals,
operands, effects, PC, terminal status, result, fault and claim. Budget-zero
checks cover all fields and the allocator. Round-robin runs preserve exactly-once
effects and terminal-failure isolation. Session object IDs are monotonic and
meaningful only in their originating session. This slice has no collector,
snapshot publication, cross-session identity, concurrent jobs or macro timing claim.

## Serial source VM

The GSP reader now feeds `source_vm.goose`, extending the bootstrap's execution
model with explicit frames and flat indexed storage. Numeric operations share
`vm.goose`'s primitive dispatch; terminal outcomes and the single-owner claim
protocol retain the bootstrap contract. `scalar.goose` remains the narrow legacy
adapter, and all original saved-state/ordered-effect tests still run.

Build and run a native command from this directory:

```sh
goose build execute.goose -o .execute
./.execute corpus/serial/mutations.gsp
./.execute corpus/synthetic/unicode.gsp
rm .execute
```

Output is typed and deterministic: `boolean:true`, `number:27.5`,
`number:-0`, `number:NaN`, `utf16:[55357,56832,55296]`, or
`identity:SESSION:INDEX:KIND`. Ordinary finite numbers use Goose's round-trip
binary64 decimal rendering. Object identities are meaningful only in their
session; this output is a diagnostic, not a form bridge. Exceptions produce
exit status 1 with a typed fault, function/instruction ID and exact source-byte
span. Bad framing/version and flow errors also exit 1. `smoke.bb` exercises the
native command, error exit status and bounded output, alongside debug/native
execution of `serial_tests.goose` and every earlier regression. It reports a
missing Goose compiler explicitly; `GOOSE` can select its executable.

`corpus/serial/*.js` are original small JavaScript tests, not emitted macros.
The existing Rust parser/lowerer exports their complete function trees. The
exporter test independently checks their expected values/errors in Node/V8 and
Pants; `regenerate.bb` regenerates/checks their images and the manifest. Goose
executes nested ordinary/receiver calls, immutable and shared mutable captures,
cell TDZ reads, left-to-right mutation, arrays/own data properties, numeric
NaN/signed-zero/remainder/truth behavior, operand-returning short circuiting and
exact UTF-16 strings. A synthetic `NewFixedCell` opcode variation verifies
immutable-cell writes; direct const writes and throw/try remain positioned
export errors. Recursion is not required by this corpus. Existing source
inspection tests retain exhaustive truncations, bad indices/descriptors,
versions, branch boundaries and transactional image installation. New execution
validation independently checks child capture arities, stack underflow/joins,
fallthrough and per-function stack limits before any execution.

Global reads distinguish a missing binding from an own property whose value is
`undefined`. Initialization installs `globalThis` as an own data property of
the global object; property reads, identifier reads and replacement through an
alias observe that same property. Exported source regressions check these
cases against Node/V8 and Pants, including the missing-binding error.

Every captured macro/helper image also passes source flow validation and native
entry execution to instantiate its factory closure. Supported Pants macro bodies,
form operations and helper factories execute through the serial runtime below. Globals
requiring declarations, prototype/native methods, string-to-number coercion,
object coercion, catches/throws, constructors and other unsupported semantics
fail explicitly. Export/inspection support is still wider than execution;
there is no foreign evaluation fallback. C `exit` supplies only CLI exit status.

`Code` owns the decoded source bytes, UTF-16 literals, functions, child IDs and
operations after the input image dies. A `Heap` owns fixed-stride node,
property, capture-value, constant-value and UTF-16 pools. Plain mutable JS
objects/arrays are separate from persistent form nodes. Closures
copy capture values/IDs into their own range; cells own initialization,
immutability and payload state. Frame return reclaims operands and locals but
retains all reachable captured values. Property chains contain integer indices.
Heap indices append until whole-session scope exit: no slot recycling, rewind
or collector. A serial admission authority assigns monotonically increasing
session IDs and rejects exhaustion; every external string/object value checks
its session and range, so an old ID cannot name a replacement in a new session.
The authority is outside snapshot state.

Default cumulative session limits are 100,000 nodes, 1,048,576 properties,
1,048,576 capture values, 1,048,576 constants, 1,048,576 UTF-16 units and
10,000,000 work units. Instructions, allocations, local/argument initialization,
string copying/comparison and property/index scans charge work. A job has at
most 1,000,000 instructions, 256 frames, 65,536 live locals and 65,536 operands;
its diagnostic write trace also has a property-count bound. Program decoding
retains GSP's separate framing limits. Limit failures are terminal and never
replayed. A failed job retains its explicit frame/fault state for inspection;
its scratch disappears at job scope exit. Earlier heap writes/allocations and
cumulative charges remain. The next job can run on the same session if budgets
permit. `serial_tests.goose` compares all frame PCs, operands, locals, results,
ordered writes, heap tables and allocator cursors at every instruction cut
against single-step execution, including zero budgets and calls/branches.

The **checkpoint boundary** is admission with no live job: after initialization,
or after every preceding job has been explicitly retired with `source::retire`.
`start` registers the owner; terminal execution retains its diagnostic state
until retirement. Goose scope exit has no VM destructor hook, so a forgotten
retirement conservatively blocks capture. Retiring a suspended job abandons it
with a terminal `InvalidState`; retiring a claimed job fails. Initialized helper
closures and retained heap values remain live. Sliced execution retains one
job's explicit state in place. Snapshots reject even an operand-only object
across a yield, preserving every saved frame/local/operand and the heap.

## Initialized private snapshots

`snapshot.goose` captures a version-2 flat `Snapshot`: owned component arrays
containing code bytes/functions/constants/children/instructions/UTF-16, every
heap node/capture/property/UTF-16/constant/source-byte/source-range pool, and a
sorted definition/root table of `(id, generation, value)`. Occurrence origins,
source spans, namespaces, metadata, captured cells, object cycles, aliases,
initialization/producer counters, limits and cumulative work survive. Private
mutable objects are valid. This does not apply the immutable-publication check.

Allocator version 1 is the existing append-only arena. All fourteen end cursors
must exactly match component lengths. There is no slot or span free list to
serialize and no per-slot recycling. Appending after restore starts at the saved
end. Whole-arena scope exit releases storage; the next arena has a fresh session
identity, so reused object slots and string spans cannot be authorized by stale
handles or retained definition entries. Cell immutability is initialized anew.
The serial identity allocator fails at exhaustion instead of wrapping.

Capture validates the borrowing source view before allocating its copy. Restore
validates every pool and code/graph link before touching empty destination
owners. It checks version, allocator/cursors, generations, code stack flow,
property-chain termination, ranges, source UTF-8/spans, limits, and budgets.
An unrealized mapSeq memo's writable value range must be disjoint from every
other memo, collection, cursor, argument and closure-capture range. Immutable
ranges may alias each other. This uses one byte per value slot only when a
nonempty memo exists, with initialization and range visits charged to validation
work and scratch charged against the byte budget. The code and value bitmaps
have separate lifetimes, so validator scratch peaks at their maximum size.
Arbitrary object cycles use integer links and need no recursive copy. Code and
form pools are independently owned in every restored worker. A worker receives
one fresh session ID reserved **on the coordinator** with `reserve_identity`;
restore relocates every private value to it, while definition IDs/generations
and the base/checkpoint identity remain distinct. Capture reserves a unique
checkpoint ID from the same non-reusing coordinator namespace after validation.
Workers never call the serial
allocator. Producer claims, waiters, shared-effect authority and publication or
import-cache authority have no fields in this schema. Restore invokes no code.

The optional `encode`/`decode` codec uses Goose's checked flat-array framing,
followed by the same semantic validator. This local image is tied to the
installed compiler's packed record ABI; it is not a portable transport format.
Malformed lengths/tags/framing, truncations and invalid semantic fields are
typed failures. Failed capture/restore installs no partial snapshot or heap.

`Control` records snapshot bytes, copied snapshot-component bytes, validator
scratch peak, capture/restore nanoseconds and validation work. Component copying
counts capture once, the explicit full record temporary plus serialization in
`encode`, both the decoded owner and returned copy in `decode`, and each restored
pool. Scalar field accesses and unrelated form conversion are outside that copy
counter. Native worker reports add both full spawn copies: the compiler builds
the packet, then the runtime copies it into worker-owned storage; the finished
array entry thunk views that packet. There is no hidden third array copy.

The finite `snapshot_workers.goose` probe reserves private bytes and execution
slots before spawn and releases them after join. It runs 32 jobs in batches of
two native workers (one coordinator plus two execution threads), with bounded
heap pools, work/steps, frames/operands/locals, output conversion and result queue.
Reservations include both spawn packets, restored code/definitions, maximum
heap pools, the validation bitmap and a conservative scratch/output allowance.
Reports include actual retained component bytes, nodes/work, copy bytes, times
and peak reservations. These are logical storage bounds, excluding Goose's
reserved virtual stack address space and process RSS. Per-job private owners die
before the report is queued; `thread_wait` releases worker/packet storage before
the admission reservation is returned. Queue capacity covers every admitted
report; joins precede nonblocking receives, so a contained failure cannot strand
a receive. Exhaustion is explicit. This is an isolation/lifetime probe, not the
later execution scheduler or immutable artifact publication.

`smoke.bb` formats/checks the snapshot sources, runs serial snapshot tests in
debug/native modes, and builds/runs the threaded probe both with native
`--debug` and normal native compilation. It checks divergent parents, sibling
mutation/allocation, closure-only values, cyclic objects, retired producer
results, exact form/source ownership, malformed metadata, every codec truncation,
operand/local yield rejection, stale reset handles/classifications, identity
exhaustion, bounded repeated jobs and cleanup. Historical lifetime obligations
adapt `158f0cb7`/`d87ac453:tests/gc.rs` and `8e0e6219:src/heap/mod.rs` without
introducing their shared heap, collector or weak-reference API. The real emitted
churn/closure macros and two helper producers initialize once before branching;
every copy retains boot=1, initializers=2, producers=2. Printed timings are
diagnostic correctness-run measurements, not a performance comparison.

## Immutable branch artifacts

`artifact.goose` implements a separate publication boundary over the initialized
private snapshots. `Artifact` schema 1 owns flat node, capture/child, exact UTF-16
and source-byte/range arrays. Links inside the artifact are zero-session local
indices. A source-index map retains a single owner for occurrences from the
same source. Definition ID and positive version identify one immutable delivery in
one base checkpoint. A changed version or changed payload for an installed
definition is an explicit conflict; installing a newer generation requires a
new base/import context. The local `to_bytes`/`from_bytes` codec is checked and
compiler-ABI-specific, like the snapshot codec, rather than a wire standard.

Each node also carries `(arena, slot)` provenance. Original snapshot slots use
the checkpoint identity; new branch slots use the fresh worker identity. An
import map binds those keys to consumer slots, and re-export preserves imported
keys across further branches. Equal occurrences keep different keys. Base keys
map to existing restored slots only after their complete record and edges match
both the frozen base and destination. Other reused keys require exact canonical
record signatures with provenance links, text and source bytes. Thus signatures
ignore relocated table offsets without interning by value. The current bounded
contract requires identical preloaded code in every worker; code growth is a
context error and function IDs do not require a second relocation scheme.

Supported values are primitives, persistent forms and metadata, namespace/name
strings, scalar occurrences, sequence cursors, native function leaves, closures
and initialized immutable capture cells. Every value, namespace, metadata, map
key/value and capture edge is checked. Plain JS objects/arrays and mutable or
uninitialized cells reject, including inside immutable containers. Callable
closure/cell cycles are supported; cycles involving compiler-form containers
reject. A failed walk returns a typed error plus node IDs along the offending
path: source heap IDs for export, artifact IDs for import. Iterative Tarjan DFS
visits each distinct reachable node and edge once, retaining completed DFS
siblings until their full strongly connected component is classified. Any
cyclic component containing a form rejects; a form may contain a callable cycle
without belonging to it. Provenance sorting and bounded table scans also
consume the work budget. Node/pool/byte/work limits bound
validation and installation. There is no shared mutable reference exception.

`Imports` is external to snapshots and is bound to exactly one base checkpoint
and consumer session. Import first validates the complete graph, stages all
links and exact signatures, detects definition/provenance conflicts and checks
pool/cache budgets. Only then does it append heap records, publish the local
definition entry and update import caches. A failed operation changes none of
those owners, including when an earlier graph sibling was valid. There is no
positive classification cache, waiter state or scheduling/effect authority in
this module. Duplicate delivery preserves the original root and installs zero
records; validation and private serialization staging still have a cost. Exact
artifact and base signatures check and charge each framed record or text/source
slice before appending it, including repeated references to one shared string.

The normal `smoke.bb` command checks, debug-runs and natively executes
`artifact_tests.goose`. It forks from a base containing the unchanged emitted
branch helper/consumer code. Independent A/B helpers deliberately allocate
colliding slots; C imports both and invokes the original consumer to obtain
`(quote [2 3 9 16])`. Both import orders produce identical compiler-form output.
Further cases cover independent relay diamonds and direct/duplicate delivery,
immutable captured closures and cycles, equal distinct occurrences, source
provenance/metadata/namespace/UTF-16 ownership after producer and consumer
teardown, invalid graphs and every import work cut through the exact successful
budget. Overlapping cycles cover a form reached through a completed callable
sibling. Compact shared-string graphs exercise every signature byte/work cut,
exact successful limits and unchanged heap/caches after rejection. A 40-level
shared diamond has 41 nodes and 203 graph edges, rather than
exponentially many path visits. Publication rules follow the invariants in
`629899ae:src/value/share.rs` and `tests/publication.rs`, with explicit copies.

Import reports `copied_nodes`, `copied_bytes`, `staged_bytes` and `import_ns`.
Copied bytes count installed heap pools, source owners and cache records; reused
nodes are excluded. Staged bytes count the delivery owner/serialization and
canonical-signature payloads constructed privately. These are explicit logical
payload writes, excluding allocator capacity/metadata and implicit scalar
temporaries; they are not peak RSS or whole-process allocation measurements.
Unused value/text ranges appended for reused nodes remain charged. Elapsed
time includes context/graph validation, staging, comparisons and installation;
it excludes source preparation, snapshot restore, producer/consumer execution
and codec/file I/O. Native test output prints raw per-order samples and diamond
import costs. Microsecond clock granularity and cold first-call work make these
diagnostic measurements, not speedup or worker-scaling evidence.

Five fresh native processes on 2026-11-03 UTC, macOS 15.7.3 ARM64, Goose
v2026.3-267-g369c7a2c/runtime 3025e5810379 and Babashka 1.12.218, with no other
build/test from this run active, produced these raw nanoseconds. Every process
runs A-then-B first, then B-then-A, with no additional warmup. Each join installs
12 branch nodes and 13,052 heap/cache bytes, constructing 14,556 private staging
bytes. The subsequent same-consumer diamond import installs 10,583 bytes,
including charged redundant pool ranges for already known nodes. These order
and cache effects are visible rather than averaged into a throughput claim.

| Process | A-then-B import | B-then-A import | First diamond import | Second diamond import |
| --- | --- | --- | --- | --- |
| 1 | 24000 | 4000 | 7000 | 3000 |
| 2 | 13000 | 3000 | 7000 | 3000 |
| 3 | 13000 | 2000 | 8000 | 3000 |
| 4 | 11000 | 4000 | 8000 | 3000 |
| 5 | 11000 | 4000 | 8000 | 3000 |

Reproduce from the prototype directory (each execution includes all artifact
assertions; run the executable five times for the table above):

```sh
goose build artifact_tests.goose -o .artifact-tests
./.artifact-tests
rm .artifact-tests
```

## Dependency-ready native worker pool

`scheduler.goose` owns one coordinator on the caller thread and exactly the
configured 1–8 persistent execution threads, with no nested workers. The
compiler supplies a sealed `Plan`: correlated request IDs, already-lowered Code
entry indices, scalar invocation arguments, and exact prerequisite keys and
root installation names. The coordinator performs execution scheduling;
light-cljs retains name resolution and compilation. The live compiler adapter
and arbitrary form argument transport remain the separate adapter boundary.
`Run.unresolved` returns exact compiler requests absent from the sealed plan;
they settle as `Missing`, so an incomplete plan cannot wait indefinitely.

`dependency.goose` adapts the single-assignment invariants from
`be0e53e7:tests/dependency.rs` and `27d517f4:src/dep.rs` as ordered coordinator
messages. Keys contain compilation, session, definition/query ID, definition
version (`generation`), and result kind. First demand issues one producer token;
repeated demands retain one recorded edge and one terminal delivery per
consumer/key, including ready reads. Completion requires the exact key/token
and rechecks the pending cell after artifact validation. Success, failure and
cancellation remain immutable. Each cell also supplies a publication ID local
to the initialized checkpoint: distinct versions and kinds of one definition
can coexist in the artifact import table without aliasing their public keys.
Claims, subscriptions, delivery logs and publication IDs belong to the
coordinator and never enter a private heap snapshot.

Every accepted work packet receives a fresh private heap identity. A worker
restores the initialized snapshot, validates/imports all successful prerequisite
artifacts and installs the requested compiler roots while no VM Job exists,
then enters the emitted function/factory/macro. Publication occurs after all VM
Jobs retire. A missing emitted `cljReadRoot` now reports `DependencyNeed`, mapped
to `MidJobNeed`; it cannot silently return undefined or trigger replay. A
present root whose value is undefined remains a valid read. Explicitly declared
mid-job requests fail before dispatch. Failed producers settle their waiting
consumers as `Prerequisite`; unrelated work continues. Once no active work can
make progress, remaining cycles (including their blocked dependents) settle as
`Cycle`.

Work and result queues have independent types and configurable capacities 1–8;
workers deliberately consume a shared work queue. Admission counts both queued
and executing jobs and never exceeds the worker limit. Failed puts retain the
work/result for retry. On cancellation the coordinator stops admission and
installs cancellation for pending cells, then drains the already admitted,
budgeted executions. Installed terminal winners remain unchanged. Shutdown
retries one stop per worker and joins every owned thread after all results have
been drained. Cancellation messages include compilation/session and have a
capacity of eight. Exactly one coordinator owns these process-wide queue types.
Native process crashes remain process failures.

Plans are bounded to 128 jobs/1,024 prerequisite records, with caller-specified
cell/edge capacity checked before mutation. Each invocation has a step cap and each job has a
work cap; private heap growth is additionally capped to 8,192 nodes, 32,768
values, 4,096 properties and 65,536 UTF-16 units beyond the base, with 64 frames
and 1,024 stack entries. The defaults bound each result's artifact payload to
1 MiB, a prerequisite packet to 8 MiB, and retained artifacts to 64 MiB.
Equal per-job artifact quotas reserve the retention budget before workers start,
so budget winners cannot depend on arrival order. Import caches use the packet
budget. Queue capacities count flat owned packets; all copies, VM storage and
validation scratch still incur their existing snapshot/artifact bounds.

`scheduler_tests.goose` executes chain, fan-out/fan-in and diamond graphs at
1/2/4/8 workers with reversed delays. Chain/fan-out edges are explicit protocol
fixtures executing unchanged real helper bodies. The diamond consumes actual
compiler-emitted `branches.cljs` helper results using the emitted root reads;
its base contains no precomputed helper results. Each run traces producer
starts and artifact installations and asserts every prerequisite installation
precedes consumer admission. Reordered runs compare ordered diagnostics and
exported expansion graphs (`(quote [2 3 9 16])`). Physical completion traces
retain their actual order; `output` emits terminal diagnostics (including fault function/PC) in submission
order, and correlated results carry the request IDs. Additional native cases
exercise same-definition versions/kinds, missing/cyclic dependencies, undeclared
reads, unrelated success, scoped cancellation, VM/packet/artifact exhaustion,
capacity-one backpressure for jobs/results/stops, and complete thread joining.
`dependency_tests.goose` deterministically exercises both demand/completion
orders, duplicate first demand, and two validated completions awaiting install.
Snapshot restore tests retain one original claim/waiter and reject VM identities
as completion tokens.

Run the bounded correctness checks from the repository root:

```sh
bb crates/big-boy-pants-js/scripts/smoke.bb
```

Smoke includes dependency check/debug/native execution and native debug/optimized
pool execution (Goose's JIT cannot link thread-local storage). The pool test
reports actual coordinator/worker/admitted counts and asserts cleanup within a
30-second correctness bound. These are correctness runs with deliberate delays;
they do not supply worker-scaling or compiler end-to-end performance evidence.

## Declared shared effects

The compiler marks effect-connected `Spec.ordered` jobs and supplies `Plan.effects`
in source order. Each `effects::Grant` names a unique logical occurrence ID,
owner request ID, zero-based per-owner occurrence sequence, native operation
and logical resource ID. Plan job order is the compiler's source order; grants
for an owner must be contiguous in that order. Malformed grants reject before
claims, queues or threads. Grants describe an exact executed effect trace;
conditional or dynamic effect counts need a compiler plan that resolves those
choices before admission. Unconsumed declarations fail the job explicitly.

The coordinator executes ordered jobs one at a time, alongside the existing
bounded pool of pure workers. Each ordered job still restores a private heap;
the sole `effects::Authority` owns sixteen scalar counter resources and a
logical-ID receipt log for the sealed plan. Neither shared values nor authority
enter the initialized snapshot, input artifacts or worker messages. A subsequent
ordered job waits for prior ordered jobs to succeed; a failure suppresses its
effects, while unrelated pure jobs continue. Arbitrary mutable heaps are never
merged. Private JS allocations and mutation remain permitted, and transitive
artifact validation rejects mutable captures with `Error.Mutable` and a node
path. Native operation handles carry no counter state: reading through one in
a pure job still requires authority and cannot read a stale snapshot value.

Supported leaves are gensym (native ID 5), scalar counter increment (6), and
scalar counter read (7). The compiler installs counter capabilities explicitly;
`boot_runtime` continues to install gensym. Counter leaves take no arguments.
Gensym accepts zero arguments or an unqualified UTF-16 prefix up to 128 units,
appends the resource's monotonic integer, and returns an immutable symbol. Its
default prefix is `G__`. Counters stop at the largest exact JS integer. This is
a narrow fresh naming resource, excluding compiler initializer naming scopes,
collision registries, prefix coercion and namespace-bearing prefixes; the live
compiler adapter must provide those services before admitting such jobs.
External effects (test leaf 8) and unknown leaves remain explicit failures.
No file, process or network operation is implemented. Declarations and IDs
currently belong to one sealed dispatch; the live adapter remains responsible
for a compilation's longer-lived naming and request identity.

An enabled `source::Job` saves a pending request at the call continuation,
returns `Yielded`, and releases its execution claim before the host handles it.
While pending, run calls consume no instructions or heap work and cannot claim
the job. `effects::service` requires released ownership, checks operation, owner
and sequence, then commits through the authority. Exact duplicate logical IDs
return the original receipt, including errors; conflicting or out-of-order
delivery changes no state. `source::deliver_effect` consumes the continuation
once. A delivery error, including local result allocation failure after an
effect commits, makes the job terminal. There is no restart/replay. Without an
enabled host contract the serial runtime retains its `NativeBoundary` error.

The suspension API permits the compiler to defer a response while a prerequisite
is pending and service correlated helper jobs after the parent's claim releases.
It has no blanket host nonreentry ban. The sealed scheduler still resolves its
prerequisites before admission and rejects undeclared dynamic `cljReadRoot`
requests; this change does not add name resolution or a live compiler transport.
The lower-level regression drives actual dependency cells during a suspended
response, both successful and failed, without replaying the preceding effect.

`effects_tests.goose` adapts the instruction-budget/ownership obligations from
`99d4f9fb:tests/resumable.rs` and `1ddd5633:src/task.rs`. It checks 1/2/4/8 workers,
small instruction quanta, every cut in the three-effect fixture, duplicate and
conflicting delivery, pure read/write rejection, unsupported external effects,
source-ordered failure suppression, host errors, nested helper work, native
mapv callback suspension and mutable-capture rejection. The counter fixture is
synthetic protocol evidence. Separately, the unchanged emitted `effect.cljs`
macro executes with a declared gensym and produces `(quote [7 true])`, matching
its captured V8/Pants expectations. Smoke includes native debug and optimized
effect checks; no Rust compiler/production backend path changes.

The optional native diagnostic measures serialized effect work separately:

```sh
goose build crates/big-boy-pants-js/src/effects_tests.goose -o .goose-effects
./.goose-effects --measure
rm .goose-effects
```

Each worker limit gets one unreported warmup and five raw samples. A sample
executes sixteen ordered three-effect counter jobs and 48 independent private
mutation jobs from the unchanged JS `serial/mutations` corpus; it checks every
terminal result and the 48 receipts/counter total 32. `ordered-ns` includes
private restore/import/execute/publication for serialized jobs; `service-ns` is
its authority-delivery subset. `pure-worker-sum-ns` sums restore/import/execute/
publication time across execution threads and is not wall time. Dispatch wall
includes thread creation, queued work, result validation, stops and joins. The
separate final row also includes authority/result cleanup (and the sample's
reporting). Source preparation, initial boot/checkpoint capture, native build
and process launch/teardown are excluded. These are diagnostic workload timings,
not macro scaling, live compiler or cross-engine speedup evidence.

Raw native nanoseconds, 2026-11-03 UTC, macOS 15.7.3 ARM64 on Apple M3
Ultra (Virtual), Goose v2026.3-267-g369c7a2c/runtime 3025e5810379. No other
build/test processes were active during sampling; no allocation profiling.
Short subphases have microsecond clock resolution. The worker sum is reported
separately from serial authority work; thread/queue overhead dominates this
small workload's dispatch wall, and these samples show no wall-time scaling.

| Workers | Sample | Ordered | Service subset | Pure worker sum | Dispatch through join | With cleanup/report |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 1 | 127000 | 14000 | 808000 | 98287000 | 98330000 |
| 1 | 2 | 113000 | 10000 | 765000 | 95442000 | 95470000 |
| 1 | 3 | 117000 | 12000 | 757000 | 95885000 | 95912000 |
| 1 | 4 | 115000 | 11000 | 737000 | 94796000 | 94821000 |
| 1 | 5 | 115000 | 12000 | 750000 | 90600000 | 90626000 |
| 2 | 1 | 122000 | 8000 | 941000 | 101131000 | 101160000 |
| 2 | 2 | 116000 | 14000 | 1037000 | 94012000 | 94083000 |
| 2 | 3 | 173000 | 14000 | 1366000 | 98680000 | 98746000 |
| 2 | 4 | 260000 | 20000 | 2664000 | 93945000 | 94011000 |
| 2 | 5 | 223000 | 21000 | 2375000 | 94836000 | 94954000 |
| 4 | 1 | 301000 | 21000 | 6522000 | 85466000 | 85563000 |
| 4 | 2 | 317000 | 23000 | 4253000 | 92884000 | 92992000 |
| 4 | 3 | 395000 | 31000 | 6016000 | 81734000 | 81836000 |
| 4 | 4 | 373000 | 26000 | 4749000 | 92948000 | 93056000 |
| 4 | 5 | 392000 | 30000 | 5218000 | 94897000 | 94963000 |
| 8 | 1 | 140000 | 12000 | 2656000 | 111604000 | 111670000 |
| 8 | 2 | 174000 | 14000 | 3417000 | 107159000 | 107211000 |
| 8 | 3 | 240000 | 20000 | 5511000 | 109566000 | 109672000 |
| 8 | 4 | 320000 | 25000 | 4414000 | 107107000 | 107169000 |
| 8 | 5 | 264000 | 26000 | 4038000 | 102617000 | 102703000 |

## Early native source diagnostic

Run `bb crates/big-boy-pants-js/scripts/measure.bb` from the repository
root. It checks tools, builds in a temporary directory inside the checkout and
removes it on every exit. It compares the unchanged historical arithmetic and
control-flow function sources with their exported images: one execution thread,
100 warmup calls, then 1,000 checked calls, five fresh-process samples per
engine/workload. The checksums are 27,500 and 7,307,000. Goose includes job
creation/disposal and budget checks; V8 can optimize these tiny pure functions.
The load/boot column includes file I/O and binary validation/decoding for Goose,
and file I/O plus parsing/compilation for V8. Export and native compilation are
excluded. Process wall time additionally includes launch, output and session
teardown; it cannot isolate VM speed. No allocation instrumentation is enabled.

This is a diagnostic on source execution, **not macro performance**, compiler
end-to-end performance, snapshot copying or worker scaling. The initial V8
comparison uses Node's V8, not light-cljs's embedded V8 build. Different runtime
surfaces and process boot costs must remain visible in later comparisons.

Samples taken 2026-11-03 UTC on macOS 15.7.3, ARM64 Apple M3 Ultra
(Virtual), Goose v2026.3-267-g369c7a2c/runtime 3025e5810379, Babashka
1.12.218, Node v26.12.1/V8 14.6.202.34-node.40. No other builds/tests from
this run overlapped these samples. All numbers below are raw nanoseconds.

| Source | Engine | Sample | Load/boot | Warmup | Execute/dispose | Internal total | Process wall |
| --- | --- | --- | --- | --- | --- | --- | --- |
| arithmetic | goose | 1 | 74000 | 48000 | 426000 | 556000 | 3487250 |
| arithmetic | goose | 2 | 51000 | 46000 | 408000 | 515000 | 3077459 |
| arithmetic | goose | 3 | 48000 | 43000 | 395000 | 494000 | 2904750 |
| arithmetic | goose | 4 | 43000 | 43000 | 381000 | 473000 | 2596583 |
| arithmetic | goose | 5 | 45000 | 43000 | 370000 | 464000 | 2540333 |
| arithmetic | v8 | 1 | 194583 | 10334 | 148416 | 2103958 | 49514166 |
| arithmetic | v8 | 2 | 135625 | 11125 | 113625 | 1845541 | 49440959 |
| arithmetic | v8 | 3 | 135000 | 8917 | 117333 | 1898375 | 48759333 |
| arithmetic | v8 | 4 | 148459 | 8833 | 119583 | 1854417 | 51186583 |
| arithmetic | v8 | 5 | 119125 | 8542 | 116375 | 2033917 | 48847667 |
| control_flow | goose | 1 | 55000 | 511000 | 5478000 | 6055000 | 8637167 |
| control_flow | goose | 2 | 52000 | 501000 | 5138000 | 5698000 | 8591000 |
| control_flow | goose | 3 | 53000 | 497000 | 4991000 | 5549000 | 7946125 |
| control_flow | goose | 4 | 44000 | 496000 | 5049000 | 5602000 | 7825583 |
| control_flow | goose | 5 | 47000 | 496000 | 5265000 | 5816000 | 8165375 |
| control_flow | v8 | 1 | 133166 | 73500 | 131375 | 1888750 | 49411583 |
| control_flow | v8 | 2 | 124000 | 74417 | 154500 | 1889083 | 48026000 |
| control_flow | v8 | 3 | 137042 | 83208 | 141792 | 1967750 | 49390375 |
| control_flow | v8 | 4 | 127292 | 73375 | 159291 | 1943458 | 48618209 |
| control_flow | v8 | 5 | 128083 | 73875 | 142292 | 1910291 | 48385041 |

Verification on this slice: `regenerate.bb --check` passes (compiler capture,
independent Node/Pants references, exporter tests, manifest checks, format,
Goose checks/debug/native smoke). `cargo build --locked -p big-boy-pants-js
--all-targets` passes with `CARGO_BUILD_JOBS=3`, Rust 1.99.1. Strict engine
Clippy retains 11 existing diagnostics: deprecated atomic fetch_update,
chunks_exact, byte_char_slices, mutable_key_type and unused Wakeups. Engine
`cargo fmt -- --check` retains existing formatting differences in src/job.rs,
lib.rs, object/array.rs, object/map.rs, object/weak_map.rs, object.rs, runtime.rs,
task.rs, vm/frames.rs, vm/proxy.rs and tests/gc_roots.rs. These paths are byte
identical to d64d9045; no lint suppression or unrelated formatting was added.
The changed exporter passes standalone rustfmt; `git diff --check` passes.
Full compiler/engine suites were not run: the production evaluator and engine
dispatch are unchanged, and this slice uses focused exporter/capture and native
Goose verification.

## Serial macro environment and compiler forms

Run `bb crates/big-boy-pants-js/scripts/macros.bb` from the repository
root. It builds the native `macro_run.goose` runner inside this checkout, imports
compiler-captured forms/environments from the existing `.jobs.edn` files through
GFB2 binary packets, and removes temporary files on every exit. Each corpus case
uses one heap and one boot. Every emitted macro factory executes once; both
branch helper producers execute once before their consumer is admitted. The
entire job sequence then repeats on those same closures. Counters assert one
boot, one initializer per definition and exactly two branch producer calls.
The runner executes complete source bodies, not precomputed expansions or
Goose translations of macros. Production evaluator selection is unchanged;
this is the compiler capture/fixture adapter, not a live compilation service.

The Rust capture test now compiles the original `context.cljs` fixture in both
reference hosts, preserving the same emitted factories and compiler-owned form
records as earlier corpus cases. Its two inputs exercise metadata on `&form`
and an unchanged nested argument, `&env` namespace/name lookup, symbols,
keywords, maps and equal numbers at different positions. `macros.bb` compares
all supported expansion values, ordered children, metadata, origin tokens and
source bytes/spans with the captured references, and separately checks live
Node/V8 values. It explicitly checks that returned `x` is the same output node
as `&form`'s argument, that returned metadata aliases `&form`'s metadata, and
that equal leaves retain distinct occurrence IDs. Outputs are compiler-owned
Babashka values after the native process/session has exited.

The selected native surface is deliberately small:

| Leaf | Supported semantics |
| --- | --- |
| `NewFormList/Vector/Map/Set` | Immutable ordered child ranges; ordinary maps keep the first equal key and last value, sets keep the first equal member; plain mutable JS arrays/objects remain separate |
| `Keyword`, `Symbol`, `Kind`, `Value` | Exact UTF-16 names and scalar occurrence unwrapping; ambiguous names reject |
| `Count/CljCount`, `Nth/CljNth/CljNthOr`, `Get/CljGet/CljGet2` | Bounded vector/list/string/nil access and ordered map lookup; invalid kinds/bounds reject where required |
| `Assoc/CljAssoc`, `Conj/CljConj` | Vector assoc in `[0,count]`, ordered map assoc, vector append/list prepend; old versions and untouched child handles survive |
| `Equals`, `CljTruth` | Structural Clojure form equality, sequential list/vector equality, nil/undefined equivalence, numeric NaN/signed-zero behavior, metadata-independent equality; occurrence identity remains separate |
| `Meta/CljMeta`, `WithMeta/CljWithMeta` | Metadata maps or nil; unsupported receivers reject; namespace components are separate from metadata |
| `CljSeq/First/Rest/Next`, `LiteralMap`, `ReaderCollection` | Immutable range cursors over selected sequential forms; ordered positional map operands |
| `CljReadRoot` | Already-resolved own data properties; nonempty dynamic root contexts reject |
| Runtime `inc`, `mapv`, `vec`, `into` | Native collection operations; `mapv` invokes original source callbacks through explicit resumable frames |
| Runtime `macroReaderFunction` | Identity adapter for this corpus's directly emitted form ABI; dynamic reader/namespace services are not admitted |
| Runtime `gensym` | Declared coordinator authority below; serial/pure jobs reject before mutation |
| Every unlisted form operation | Explicit `NativeBoundary` failure; no effect, replay or foreign evaluation fallback |

Flat persistent containers copy child handles on update. This implements the
layout decision in `runtime-design.md`; it does not reproduce the historical
trie. Historical `0881cf65:tests/collections.rs` and
`7c7f314a/15cbfbde:tests/collection_intrinsics.rs` informed the alias, occurrence,
assoc-boundary and failure checks. The selected runtime uses no mutable
collection builder, transient or freeze operation: none can be imported or
exported as an immutable form. General builders, lazy realization, protocol
callbacks, dynamic helper requests and production runtime coverage remain explicit
boundaries; the declared coordinator contract below supports narrow effects.

`form_bridge.goose` owns flat graph nodes, child IDs, UTF-16 units, source
entries and root IDs. GFB2 has a 32-byte header (magic/version and six
pool counts), 48-byte tagged node records, 32-bit child/root IDs, 16-bit code
units and 12-byte source entries, all little endian with IEEE binary64 Numbers.
A source entry is an identity, a start and a byte count. The compiler sends
each source as its identity and length with no text, and checks every
returned span against its own source, UTF-8 boundaries included; Goose
range-checks spans, keeps the identity in its heap rows and snapshots, and an
export names only the sources its forms came from (light-cljs-817). An entry
with identity -1 carries its text in the graph's source bytes; only Goose's
fixtures use one, and the compiler refuses it. GFB1, which carried every
source's text, is refused.
Node records retain ordered child/text ranges, optional namespace, metadata
edge, opaque compiler origin token, source index and byte span. `-1` denotes a
new expansion origin, resolved by the caller's current macro identity; imported
origin tokens are copied without value-based interning. Reader integer spelling
is separate from runtime Number payloads. Nil, booleans, Numbers, strings,
symbols, keywords, lists, vectors, maps and sets are supported; compiler literal
payloads and mutable/executable objects reject explicitly.

Import prevalidates every node/metadata edge, all ranges and IDs, cycles,
longest DAG paths, node/depth/byte/work limits and cancellation before changing
any heap pool or installing a root. The append-only heap owns copied source
bytes as well as runtime data. Export bounds its retention walk (including
shared edges and metadata), constructs a private graph, validates it and only
then commits an independently owned result. Encode/decode also commit only on
success. Default conversion limits are 100,000 nodes, depth 128, 16 MiB and
10,000,000 work units. Cancellation is a sticky typed outcome, independently
of stdout. Failed conversions install/publish nothing. The compiler's current
compatibility rule for newly computed bare primitives can recover the latest
argument span by value; explicit occurrence handles bypass that fallback, so
unchanged equal scalars keep their own identities and spans.

`smoke.bb` now checks/runs `bridge_tests.goose` and `macro_tests.goose` in debug
and native modes. These cover complete graph ownership after heap teardown,
exact empty/non-ASCII/supplementary/lone-surrogate strings and named components,
Number bits for signed zero/NaN/infinities, metadata and child order, persistent
aliases and assoc boundaries, every callback instruction cut, callback failures
followed by reuse, and rejected effectful source. The bridge tests exhaust every
cancellation work cut and binary truncation for the small structured fixture,
including retention and metadata walks, and test transactional node/depth/byte/
work limits, malformed IDs/names and cycles. Existing serial tests continue to
cover private mutation, returned object aliases, exact saved state and errors.

## First native macro diagnostic

Run `bb crates/big-boy-pants-js/scripts/macros.bb --measure`. It first
runs the correctness comparisons, then records five fresh-process samples of
the unchanged emitted churn macro at `0,16,100,0,16,100` after one initialization.
There is one execution thread, no allocation profiler, no extra warmup and no
worker scaling. Every sample checks the six results and native provenance;
repeated calls expose reuse explicitly. The checksum is 10,836. Each job emits
separate import/invocation/export rows; the table sums those phases over six
calls. Boot/factory/helper counters remain observable in the raw command output.

Goose load/boot includes GSP file I/O, validation, relocation, heap boot and
source factory invocation. Import includes GFB file I/O, decode, validation and
heap installation. Export includes the bounded provenance walk, graph copying,
validation, binary encoding and file write. Node/V8 uses the actual V8-emitted
macro with the full `src/js_runtime.js`, imports structured values and writes
structured JSON results; boot includes source parsing/initialization and request
JSON parsing. V8 has its full JS collection runtime; Goose implements the
selected native leaves above. Node's conversion lacks Goose's explicit source
occurrence payloads, and the wire formats differ. These are matched expansion
jobs with visible runtime/transport differences, not equal internal work.
Process wall includes launch, output and teardown. Compiler source preparation,
export and native compilation are excluded. This is not embedded-V8, live
compiler end-to-end, snapshot, parallel or whole-project performance evidence.

Raw nanoseconds on 2026-11-03 UTC, macOS 15.7.3 ARM64 Apple M3 Ultra (Virtual),
Goose v2026.3-267-g369c7a2c/runtime 3025e5810379, Babashka 1.12.218 and
Node v26.12.1/V8 14.6.202.34-node.40. No other builds/tests from this run
were active during these samples. Short native phases have microsecond clock
resolution; zeros in individual raw rows are not zero-cost claims.

| Engine | Sample | Load/boot | Import | Invoke | Export/write | Process wall |
| --- | --- | --- | --- | --- | --- | --- |
| Goose | 1 | 77000 | 148000 | 605000 | 467000 | 4244583 |
| Node/V8 | 1 | 8998458 | 768460 | 4772916 | 708833 | 65375792 |
| Goose | 2 | 78000 | 150000 | 604000 | 354000 | 4577417 |
| Node/V8 | 2 | 9152792 | 858332 | 4840669 | 705666 | 67403583 |
| Goose | 3 | 77000 | 134000 | 568000 | 376000 | 4557792 |
| Node/V8 | 3 | 8651875 | 763417 | 4885042 | 748666 | 66950375 |
| Goose | 4 | 92000 | 132000 | 584000 | 393000 | 4748500 |
| Node/V8 | 4 | 8996875 | 809126 | 5042374 | 813668 | 66234875 |
| Goose | 5 | 81000 | 146000 | 595000 | 427000 | 4605750 |
| Node/V8 | 5 | 8754750 | 787000 | 4862375 | 770417 | 65917959 |

Source capture/export regeneration and native/reference commands pass on this
slice. Strict root Clippy (Rust 1.99.1, default and QuickJS-only, all targets)
retains dead-code diagnostics for `program_fragment::{published,expr,
append_published}` and `items_after_test_module` in `analyzer.rs`. Root formatting
retains differences in unchanged `src/js_syntax.rs` and the engine files listed
above. These paths are byte-identical to the base `b38e840f`; the changed capture
file passes standalone rustfmt. No lint suppression or unrelated formatting
was added. Full compiler/engine suites were not run: production execution paths
are unchanged; focused compiler capture/export and native bridge/runtime checks
cover the changed boundary.

## Bounded serial cost investigation

`macros.bb --measure` now compares current embedded V8 and Pants with native
Goose at one execution worker, with explicit warmup and five alternating rounds.
It separates binary form conversion from result file writing. `macros.bb
--profile` builds a separate native counter probe; its timings are excluded from
the clean comparison. See [serial-costs.md](evidence/serial-costs.md) for the two measured
costs, scope limits, decisions and commands, and [serial-samples.edn](evidence/serial-samples.edn)
for raw phase samples and counters. Earlier Node-only tables above retain their
original diagnostic scope and protocol and are not reused for this comparison.

### Live compiler source-macro adapter

`bb crates/big-boy-pants-js/scripts/compiler.bb` builds the native
`compiler_session.goose` runner and runs the ignored `goose_compiler` tests with
`--features goose-macros`. The feature is opt-in. An embedder explicitly creates
`GooseSession` and calls `MacroHost::use_goose_source_macros`; ordinary engine
selection is unchanged. This bounded adapter executes user source declarations
and their demanded helper factories in Goose. The compiler's existing core
syntax/bootstrap service still compiles those declarations; it is not a port of
the full canonical runtime. Unsupported source jobs fail without another engine.

The compiler exports each definition's GSP2 code once per compilation and emits
fresh GFB2 structured form arguments for every call. The cache uses the compiler's
resolved `CodeIdentity`, with separate anonymous and helper-unit identities;
equal source text does not merge distinct definitions. Conflicting source under
one macro code identity is terminal. A helper's immutable catalog unit includes
its resolved declaration context: equivalent native/canonical emission before
and after application IR release reuses its first export. Compiler callbacks
still run on every demand before that reuse. The declaring evaluator and foreign
worker views use the same definition identity. GCS1 correlates requests,
dependency demands and results over one long-lived native
process. Compiler callbacks resolve helpers from the real declaration catalog.
Returned forms own their data, occurrences and source spans independently of
Goose jobs and the session. The pipeline test compares forms, provenance,
deterministic emitted application JavaScript and application output against a
separate V8 compilation of the original `.cljs` corpus.

On first demand, the coordinator restores the booted base, loads and executes
the emitted factory, validates its callable and captures the initialized runtime
and function. Later calls restore that checkpoint into private heaps and execute
the real body with fresh inputs. Captured mutable objects/cells therefore start
from the initialized definition on every call. Preparation admits no compiler
callbacks or shared effects; body/helper demand still uses the resumable protocol
below. Prepared checkpoints remain scoped to one compilation and count against
the existing 64 MiB retention budget; at most 1,024 definitions are exported.

The runner uses one input thread, one coordinator and at most the explicitly
selected 1–8 execution threads. A dependency wait copies the complete private
heap/code and explicit VM frames, locals, operands, handlers and mutation trace
into a flat continuation, retires the VM owner, and joins its execution thread
before sending `NEED`. Reentrant helper analysis can then submit another macro
at one execution worker. `RESUME` restores the saved continuation; it never
re-evaluates the earlier body. The `reentrant.cljs` fixture retains a local
mutable object across a delayed callback and checks unchanged write counts.
Shared compile-time effects remain outside this bounded adapter's surface.

The real compiler supplies helper identities and code in its saved declaration
context. A helper's finite immutable form result or supported callable is
executed/validated once and retained by that exact unit identity, then imported
into each consumer. Other non-form results fail explicitly. Request files are unique,
helper replies publish atomically, command/event queues are bounded, and the
coordinator caps cumulative retained checkpoints/continuations/helper data at 64 MiB. Each
VM also keeps its existing bounded instruction/heap/work controls. Evaluator
views share the compilation's terminal deadline/poll control; nested form
conversions share their outer call's conversion budget. The caught-limit test
first succeeds standalone at the same limit, fails during nested conversion,
runs the emitted catch handler, and still rejects the compilation. A fresh
compilation succeeds.

Caught `TypeError`, `ReferenceError` and compiler callback failures bind an
opaque object with its own identity, so null comparisons and truthiness retain
JavaScript semantics. Explicit `throw` values remain unchanged. Error property
inspection/coercion and error form transport fail explicitly; the adapter does
not implement Error prototypes, messages or stacks. Snapshots preserve these
opaque objects, and allocation failure while creating one remains terminal.
Macro and helper factory failures report the failing job's function, PC and
emitted-source span before any form export can replace that diagnostic.

`SessionStats` separately reports exported definitions and initialized macro
preparations, as well as one subprocess/base initialization, Rust spawn/export/
bridge nanoseconds, native base load/boot/snapshot nanoseconds, native job load/
bridge nanoseconds, transferred form bytes and total elapsed session time.
These are diagnostic inclusive phase costs, with filesystem form transfer in
Rust bridge costs and flat checkpoint transport in elapsed time. They are not a
VM-only performance claim. The native job-load counter includes complete
once-only definition preparation (restore/load/factory/capture) plus helper load;
it is not a pure loader comparison with earlier measurements.
`PREPARED/START/NEED/PRODUCE/PRODUCED/RESUME/FINISH/DONE` traces
prove physical overlap, once production and no pre-wait mutation replay. The
compiler command checks both debug-native and optimized-native Goose runners.

Canonical comparisons include datum kinds as well as values. Unchanged input
occurrences restore their reader numeric flavour after checked structural and
metadata comparison; computed ECMAScript Numbers retain their f64 bits,
including negative zero and NaNs. The input registry keeps occurrence identities
independent of value equality, and retains environment data without inventing
source locations for it.

GCS1 control messages are newline-delimited ASCII records with numeric request
IDs; GSP2/GFB2 payloads are owned flat files in a private session directory. EOF,
malformed control/form replies, failed jobs, cancellation and deadlines retire
the compilation, reap the process and its threads, join Rust transport readers,
and remove the directory. Handles carry distinct evaluator/session ownership
and non-reused logical slot versions. Compiler-owned forms, nested metadata,
source occurrences and rendered diagnostics survive complete session teardown.
Attach a fresh session for each new compilation, including when reusing a host.

The existing root verification limitations still apply: default/QuickJS strict
Clippy reports `program_fragment::{published,expr,append_published}` dead code and
`analyzer.rs` items after a test module; the full default suite's playground wasm
lint encounters those same unchanged dead-code errors. Feature-enabled strict
Clippy also encounters the existing engine deprecated/chunk/byte-string lints
and `namespace_publication.rs` single-element loop. Command-line allowances for
those existing diagnostics permit a separate audit of the adapter; source lint
suppressions were not added. Existing root rustfmt differences remain outside
the changed code.

## Bounded correctness manifest

Run `bb crates/big-boy-pants-js/scripts/correctness.bb` from any working
directory. It requires Babashka, Goose, Cargo/the installed V8 cache, and Node;
a missing tool fails before execution. It runs these separate layers without
benchmark or allocation-profile flags:

- `regenerate.bb --check`: actual source capture and independent V8/Pants
  expectations, exporter checks, then synthetic validator/VM, source-derived
  execution, and snapshot/artifact/dependency/scheduler/effect tests. Every
  semantic driver runs debug and native; threaded drivers use native builds in
  both modes. Existing historical fixture attribution remains in the tests.
- Generated source v1: eight seeds `0 1 17 42 99 255 1024 24301`, each with
  original/alpha-renamed/single-evaluation identity variants. The scope-aware
  generator uses distinct binding IDs, depth at most three and at most forty
  expression nodes (closed numeric literals, addition, conditionals, lexical
  arrow bindings and ordered observations). Twenty-one additional typed rows
  make 45 V8/Goose comparisons per debug/native mode. These include returned
  and thrown primitives, signed zero, NaN/infinities, empty/escaped/unescaped
  multibyte strings and lone UTF-16 surrogates, and effects preceding errors.
  Comparator self-checks reject missing/duplicate/reordered IDs, changed status
  or value, reordered effects, extra stdout, and malformed protocol records.
- `macros.bb`: serial emitted macro execution, owned expansion/provenance,
  distinct equal occurrences, aliases, initialization and producer counts.
- `compiler.bb`: six actual `.cljs` cases (`churn`, `closure`, `branches`,
  `context`, `reentrant`, `catch`) compare V8, Pants and Goose at 1/2/4/8
  execution workers, separately checking expansion data, metadata/origins/spans,
  emitted JavaScript and application stdout after session teardown. Concurrent
  compiler jobs also check exactly one helper producer, bounded execution
  concurrency, saved writes without replay and correlated completion. Lifecycle
  tests cover worker/producer errors, injected native worker abort, cancellation,
  deadline, malformed replies and repeated sessions. The Rust form bridge is
  additionally checked in the QuickJS build.

Scheduler perturbation seeds are `17` and `42`, with at most sixteen jobs per
shape and queue capacity one. A separate deterministic receiver hook holds
completed source-derived producer packets and explicitly installs both diamond
orders using seeds `0 1 17 42`; the consumer remains blocked after the first
install and duplicate completion. Queue saturation uses refusal acknowledgments
before draining. Sleeps supplement coverage; they are not evidence that a
critical ordering occurred. Each dispatch verifies all owned workers joined;
compiler cleanup verifies the process, readers, pending calls and directory are
gone. Repeated sessions check that queues and identities do not leak.

Program/snapshot/form codecs test every byte truncation of a valid fixture.
Corrupt lengths/tags/IDs/source spans, deep values (257 levels), wide shallow
values (256 children), independent node/depth/byte/work budgets and invalid UTF-8
source text reject before installation, preserving committed graphs/caches.
Source spans must end at UTF-8 boundaries; runtime UTF-16 remains a distinct
payload. Explicit uncaught primitive throws retain their value in the terminal
job; host/VM infrastructure faults are never converted to passing thrown rows.
Unsupported surface tests expect explicit rejection and are not supported
comparison rows.

The watchdog bounds each generated execution to 10 seconds, each build or
focused Rust command to 180/300 seconds, and each composite capture/compiler
stage to 900 seconds. Individual compiler abort tests have a five-second guard;
the host closes a native process whose worker abort leaves its input thread
waiting. These are termination bounds, with no performance assertions. The
entry point records descendant process handles while running and kills/waits
for owned survivors on every exit path. On a failed comparison it retains the
exact version/seed/source and exported inputs in the printed
`.goose-correctness-*` directory, removing the native executable. Successful
runs remove that directory. All generated builds stay in this worktree.

The broader `--lib --features goose-macros -- --test-threads=3` check also
encounters two existing failures on the starting `925c964f` tree: namespace
publication reports `namespace writer needs a native engine`, and the capture
fixture's environment `generation` is `215` rather than `1` when run after unrelated
compiler tests. The filtered capture command above passes. QuickJS-only library
checks retain the namespace-publication failure. Strict all-target Clippy on
Rust 1.99.1 stops at eight existing engine deprecated-atomic, constant-chunk and
byte-string diagnostics; root formatting still differs in unrelated files.
These broad checks are limitations, not successful correctness layers or skipped
comparison rows. The bounded command uses the explicit focused checks above.

## Final bounded branching comparison

Run `bb crates/big-boy-pants-js/scripts/benchmark.bb --check`, then
`--measure` on a quiet machine; `--allocations` is a separate instrumented
invocation and `--summarize` prints all-cell medians/ranges and phase summaries.
The driver compares source-derived VM calls and actual namespace compilation,
with V8 at one execution worker and Pants/Goose at 1/2/4/8, serial and rebuild
controls, seven fixed work/form shapes and five alternating rounds per cell.
The compiler path runs two dependent source namespaces in one compilation:
the second batch reuses the same macro/helper session. Its rebuild control
uses a fresh whole compilation per source job, including helper demand and
cleanup. The one-slot V8/Pants serial controls use their ordinary serial
compiler paths; Goose's serial control retains one heap without snapshots.
`GOOSE_BENCH_DEBUG=1` with `--check` also verifies all cells in a debug native
build. Form checks include the complete scalar datum and UTF-16 pools.

The [measured report](evidence/branching-results.md) records a no-go for a compiler
speedup at this scope. It links the raw samples, storage/allocation invocation,
copy/validation controls, setup-inclusive ratios, thread/load observations and
limitations. The earlier [serial diagnostics](evidence/serial-costs.md) remain preserved.

### Expanded actual compiler scaling fixture

`benchmark.bb --expanded-check` verifies 15 substantive macro-using namespaces
and 120 consumed results, with independent waves, a downstream join and reuse.
`--expanded-measure` runs five quiet alternating rounds; `--expanded-instrument`
records readiness and actual worker/VM intervals separately, and
`--expanded-limits` diagnoses the heavier retained serial quota failure.
The [expanded report](evidence/expanded-results.md) includes results, runtime ceilings,
raw samples and repeat commands. Historical modes and datasets above remain
the small overhead diagnostic.

### Initialized definition comparison

`benchmark.bb --initialization-comparison` limits the actual compiler comparison
to small/medium/larger and pairs the current executable with the exact
`f29175c4496a25ca77812dcad4a10eb0aff4263e` predecessor. Build that revision in an
owned detached worktree. From that worktree's root, build:

```sh
GOOSE_HOME="$PWD/target/goose" goose build crates/big-boy-pants-js/src/compiler_session.goose -o compiler-session-baseline
CARGO_TARGET_DIR="$PWD/target" CARGO_BUILD_JOBS=3 LIGHT_CLJS_V8_CACHE=require cargo test --release --locked --features goose-macros --lib final_benchmark --no-run --message-format=json
```

Set
`GOOSE_PREDECESSOR_ROOT`, `GOOSE_PREDECESSOR_BINARY` (the Cargo JSON executable),
and `GOOSE_PREDECESSOR_RUNNER` to those paths. Keep both Cargo targets in their
respective worktrees; `GOOSE_HOME="$PWD/target/goose"` also keeps Goose C
intermediates inside the current worktree. Then run the comparison from the
current tree. The predecessor's tracked tree must remain unchanged.

Every logical cell has five adjacent predecessor/current pairs, reversing the
first revision each round, after one warmup per revision/cell. Ordinary source
namespace dependencies place the two measured batches across the compiler's
six-namespace deferred drain lag; the batch completion assertion remains
enabled. All supporting sources, byte counts and hashes are in the manifest.
The complete process interval includes compiler setup, source reading/emission,
definition preparation, real body calls, result conversion and cleanup.

`benchmark.bb --initialization-allocations` runs the same current compiler cells
once with separate Rust allocation/native storage instrumentation. Results go to
`initialization-samples.edn.gz` and `initialization-allocations.edn.gz`, preserving
the earlier branch files. The [initialized-definition results](evidence/branching-results.md#initialized-definition-reuse)
state the exact revisions, scope, raw history, counters and remaining measured
loss.
