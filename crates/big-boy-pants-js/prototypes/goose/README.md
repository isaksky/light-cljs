# Goose source export experiment

From the repository root, run:

```sh
bb crates/big-boy-pants-js/prototypes/goose/regenerate.bb
bb crates/big-boy-pants-js/prototypes/goose/regenerate.bb --check
```

The first command regenerates deterministic source artifacts, independent
V8/Pants expansion and output expectations, bytecode, metadata and SHA-256
manifest. The second checks without changing artifacts. Both validate metadata,
test the Rust exporter, and run Goose format/check/debug/native checks and
inspection of every binary. They require Rust and the installed V8 cache, Node,
Babashka and Goose. Builds stay inside this checkout; temporary native outputs
are removed on success or failure. For the fast Goose-only command:

```sh
bb crates/big-boy-pants-js/prototypes/goose/smoke.bb
```

`inspect.goose` accepts `.gsp` paths as arguments. Scripts resolve their own
locations and also work from the engine package as `prototypes/goose/...`.
Validated Goose: `v2026.3-267-g369c7a2c (runtime 3025e5810379)`.

[Runtime design decisions](runtime-design.md) trace the captured closure and
persistent-form requirements through QuickJS/Pants and explain the indexed
storage choices. The smoke command includes `layout_probe.goose`, a bounded
layout/lifetime probe; it does not execute macros or measure performance.

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
including dead functions: a parsed but unsupported operation, such as a throw
in an unused function, is a positioned error naming its operation and function.
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

## GSP version 2

Integers are little endian. An image owns one exact immutable UTF-8 source
buffer (source ID 0), fixed-stride tables and variable payload pools. There are
no host pointers, owning roots or runtime handles. Runtime strings/names are
exact UTF-16 code-unit ranges, including lone surrogates, separate from source
UTF-8. Source spans are half-open **byte** ranges. Diagnostic line/column work
is outside ordinary execution.

The 32-byte header is `GSP\0`, `u32 version=2`, then six u32 lengths: source
bytes, functions, constants, child IDs, instructions, UTF-16 units. Sections
follow in that order:

| Table | Record | Fields |
| --- | --- | --- |
| Functions | 13 × u32, 52 bytes | instruction start/count; constant start/count; child start/count; params; locals; captures; name-unit start/count; JS length; flags |
| Constants | 3 × u32, 12 bytes | tag and two words: undefined=0, null=1, boolean=2, f64 bits=3, UTF-16 range=4 |
| Child IDs | u32 | Absolute function IDs; function operands index their parent's child range |
| Instructions | 5 × u32, 20 bytes | descriptor; operand; source-byte start/end; original function-code byte offset |
| UTF-16 | u16 | Runtime string/name code units |

Descriptor bits 0..7 are the explicitly allowed engine opcode, 8..15 the operand
category (none, constant/name, local, byte target, child, capture, selector/count),
16..23 the original operand width (0/2/4). Flags contain strictness in bit 0 and
constructor kind (none/ordinary) in bits 8..15. `export_schema.goose` is compiled
reader code generated from the exporter's checked opcode/selector surface; an
input image cannot supply its own schema. Eval sites, unsupported operations,
Symbol/BigInt constants fail explicitly. No source behavior is dropped.

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

`snapshot.goose` captures a version-1 flat `Snapshot`: owned component arrays
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
bb crates/big-boy-pants-js/prototypes/goose/smoke.bb
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
goose build crates/big-boy-pants-js/prototypes/goose/effects_tests.goose -o .goose-effects
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

Run `bb crates/big-boy-pants-js/prototypes/goose/measure.bb` from the repository
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

Run `bb crates/big-boy-pants-js/prototypes/goose/macros.bb` from the repository
root. It builds the native `macro_run.goose` runner inside this checkout, imports
compiler-captured forms/environments from the existing `.jobs.edn` files through
GFB1 binary packets, and removes temporary files on every exit. Each corpus case
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

`form_bridge.goose` owns flat graph nodes, child IDs, UTF-16 units, source byte
owners/ranges and root IDs. GFB1 has a 32-byte header (magic/version and six
pool counts), 48-byte tagged node records, 32-bit child/root IDs, 16-bit code
units and source ranges/bytes, all little endian with IEEE binary64 Numbers.
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

Run `bb crates/big-boy-pants-js/prototypes/goose/macros.bb --measure`. It first
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
the clean comparison. See [serial-costs.md](serial-costs.md) for the two measured
costs, scope limits, decisions and commands, and [serial-samples.edn](serial-samples.edn)
for raw phase samples and counters. Earlier Node-only tables above retain their
original diagnostic scope and protocol and are not reused for this comparison.

### Live compiler source-macro adapter

`bb crates/big-boy-pants-js/prototypes/goose/compiler.bb` builds the native
`compiler_session.goose` runner and runs the ignored `goose_compiler` tests with
`--features goose-macros`. The feature is opt-in. An embedder explicitly creates
`GooseSession` and calls `MacroHost::use_goose_source_macros`; ordinary engine
selection is unchanged. This bounded adapter executes user source declarations
and their demanded helper factories in Goose. The compiler's existing core
syntax/bootstrap service still compiles those declarations; it is not a port of
the full canonical runtime. Unsupported source jobs fail without another engine.

The compiler emits fresh GSP2 code and GFB1 structured form arguments. GCS1
correlates requests, dependency demands and results over one long-lived native
process. Compiler callbacks resolve helpers from the real declaration catalog.
Returned forms own their data, occurrences and source spans independently of
Goose jobs and the session. The pipeline test compares forms, provenance,
deterministic emitted application JavaScript and application output against a
separate V8 compilation of the original `.cljs` corpus.

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
context. A helper's finite immutable form result is executed/validated once and
retained by that exact unit identity, then imported into each consumer. Function
or other non-form helper results fail explicitly. Request files are unique,
helper replies publish atomically, command/event queues are bounded, and the
coordinator caps cumulative retained continuations/helper data at 64 MiB. Each
VM also keeps its existing bounded instruction/heap/work controls. Evaluator
views share the compilation's terminal deadline/poll control; nested form
conversions share their outer call's conversion budget. The caught-limit test
first succeeds standalone at the same limit, fails during nested conversion,
runs the emitted catch handler, and still rejects the compilation. A fresh
compilation succeeds.

`SessionStats` reports one subprocess/base initialization, Rust spawn/export/
bridge nanoseconds, native base load/boot/snapshot nanoseconds, native job load/
bridge nanoseconds, transferred form bytes and total elapsed session time.
These are diagnostic inclusive phase costs, with filesystem form transfer in
Rust bridge costs and flat checkpoint transport in elapsed time. They are not a
VM-only performance claim. `START/NEED/PRODUCE/PRODUCED/RESUME/FINISH/DONE` traces
prove physical overlap, once production and no pre-wait mutation replay. The
compiler command checks both debug-native and optimized-native Goose runners.

GCS1 control messages are newline-delimited ASCII records with numeric request
IDs; GSP2/GFB1 payloads are owned flat files in a private session directory. EOF,
malformed control/form replies, failed jobs, cancellation and deadlines retire
the compilation, reap the process and its threads, join Rust transport readers,
and remove the directory. Handles carry distinct evaluator/session ownership
and non-reused logical slot versions. Compiler-owned forms, nested metadata,
source occurrences and rendered diagnostics survive complete session teardown.
