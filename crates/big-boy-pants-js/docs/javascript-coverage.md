# Essentially complete JavaScript on the Goose VM: architecture decisions

November 5, 2026 (light-cljs-761). This document fixes how bbp's Goose VM
(`src/`) grows from the macro subset it runs today to essentially complete
JavaScript, measured case by case. It records seven decisions, the evidence
for each and the alternatives rejected. The foundation and harness pellets in
group `goose-javascript` implement them; the area planners (light-cljs-762 to
-775) build on them. Pellets holds the work; this file holds the reasons.

Verified with Goose `v2026.3-267-g369c7a2c` (runtime 3025e5810379). Every
Goose rule relied on below was checked with a small `goose check` or run
program (the probes, summarised in [the appendix](#appendix-probes)), not
taken from documentation alone.

## Starting point

- The Rust frontend (bbp's Rust implementation, `rust/`) parses and compiles
  JavaScript to the bytecode of `rust/src/vm/instr.rs`: 198 opcodes in an
  append-only registry (0x01 to 0xC6). Its Goose exporter
  (`rust/src/frontend/goose.rs`, `supported`) accepts 93 of them (not the
  "about 105" of the group context) and refuses
  the whole program on any other, even on a dead path. It also refuses Symbol
  and BigInt constants, direct `eval`, and every constructor kind except
  ordinary functions.
- `src/source_vm.goose` (1,119 lines) runs that bytecode. A value is a 40-byte
  struct (`tag`, `number`, `a`, `b`, `session`); a heap node is a 202-byte
  struct addressed by an `i64` index; a property is an 88-byte record in a
  linked list threaded through one global table by `i64` index; array
  elements are string-keyed properties found by walking that list; an
  instruction is a 48-byte struct dispatched by `match` on an integer. Frames,
  handlers and operands are already data on job-owned stacks, so a
  JavaScript call does not recurse in Goose.
- Every value carries its heap's session identity, so restoring a snapshot
  rewrites every value (`snapshot::restore`, `relocated`). On the 200k project
  restores copy 995,474,716 bytes per session
  ([evidence](../evidence/generated-200k-performance.md)).
- The empty `src/{intl,regexp,object,...}` directories left by the move are
  not coverage.

## The Goose rules this design follows

From the user's requirement in light-cljs-761 and the Goose references:
memory follows scopes, links are typed and position independent, data stays
inline and compact, dispatch is enums and `match`, the interpreter loop does
not recurse per JavaScript call, workers share nothing and exchange flat
values, performance is checked with Goose's own tools, and stack settings are
recorded. Four facts about the current compiler decide most of what follows:

1. **Pool-relative links are not available to workers.** A `thread_fn` may
   reach only flat globals; a global array whose elements hold
   `T&<u32 in pool>` links is rejected ("thread programs may access only flat
   globals ... objects holds references", probe P2), and root structs such as
   `slot_pool` cannot be worker globals at all. bbp runs JavaScript on
   workers, so `&<u32 in pool.items>` cannot link its heap.
2. **Self-relative links do not survive an interpreter's value flow.** They
   work for straight-line code over one array of variable-mode cells (probe
   P5: verified `to_bytes`/`from_bytes` across a worker, in-place relinking
   inside a payload). They do not fit a fixed-mode value (P1: "a fixed-mode
   Cell binds its payloads only by value"), and in a probe interpreter whose
   values pass through operand stacks, frames and helper functions the
   checker could not prove the exact root that a relinking store needs: a
   store into an object reached through a value popped from the operand stack
   and a copy of links into a regrown run were both rejected ("this
   reference's root is not known exactly, only that it outlives heap", P7),
   and a helper taking a cell reference as a parameter cannot relink at all
   ("rooted at target"). Variable-element arrays also cannot be truncated.
3. **Index types fit, and tie reclamation to the thread program.**
   `type ObjId = u32 in objects` over a flat global table works inside a
   worker, each worker gets its own copy of the tables at spawn, reads through
   an `ObjId` need no bounds check (P3, P7 `--bce-lines`), and `from_bytes`
   verifies arrays of index types against their table (P8). An array named by
   an index type can never shrink (P4: "an index type names objects, so its
   length never goes down"). So the scope that frees such a heap in one step
   is the thread program that owns it.
4. **Spawning is cheap.** Spawning and joining an empty worker costs about
   17 microseconds (P8, native build).

## Decision 1: bytecode and the exporter

**Decision.** The exporter exports every opcode the Rust compiler emits, with
its existing resource bounds and source provenance; the `supported` filter is
removed. The Goose loader validates control flow for all opcodes from a
stack-effect table generated from the Rust registry into
`src/export_schema.goose` (as `export_descriptor` and `export_form_arity` are
today), then decodes each instruction row into a fixed-mode `Ins` enum with
jump targets resolved to instruction indices. An opcode the VM does not
implement decodes to `Ins.Unsupported { opcode }` and fails when executed
with a fault that names the opcode. Constants gain Symbol and BigInt tags and
functions gain the class and generator constructor kinds as their areas need
them; direct `eval` stays a named refusal until the functions area plans it.

**Form opcodes.** The light-cljs collection opcodes (`NewFormList`,
`FormOp`, `NewVector` and the rest) stay in the one append-only registry and
decode into the same `Ins` enum; their execution stays in the form module. If
Goose ever needs an opcode of its own, it is appended to `instr.rs` like any
other, and the Rust VM gets only a refusal arm (bbp's AGENTS.md forbids
extending that VM). There is no Goose-private numbering.

**Evidence.** The probe interpreter decodes the real exported map benchmark
(114 instructions, 23 opcodes) into 1,026 bytes, 9 bytes an instruction,
against 5,472 bytes (48 each) for the same program in today's `Op` table.
Dispatch is `match` on the enum (rule 4). Instruction fetch keeps one bounds
check, which a `Pc = u32 in ins` index type removes (rule 7).

**Rejected.** *Growing the filter*: one unsupported opcode anywhere, even
unreachable, refuses the whole file, so the harness would report whole files
as unsupported and hide which cases already pass, and the filter is a second
list to keep in step with the VM. *A Goose bytecode of its own*: a second
compiler, which the group forbids. *Failing at load*: correct programs whose
unimplemented paths never run would be refused. Failing at execution is still
explicit, with no fallback; the 200k project's macros use only implemented
opcodes, so its path does not change.

## Decision 2: values and objects

**Decision.** A JavaScript value is a fixed-mode enum of 9 bytes:

```goose
type ObjId = u32 in objects;   // and StrId, SymId, BigId, ShapeId, EnvId ...
enum Value { Undefined, Null, Bool { v: bool }, Number { v: f64 },
             String { s: StrId }, Symbol { s: SymId }, BigInt { b: BigId },
             Object { o: ObjId }, Empty }   // Empty: holes and TDZ, never escapes
```

The heap is a set of flat global tables in the thread program that runs the
job, linked only by index types:

- `objects`: fixed-size object headers holding the prototype (a `Value`), a
  shape, four inline property slots, an optional overflow run in `slots`, and
  the object kind as a fixed-mode enum whose payload carries the kind's
  internal slots (array length and element run; function ID and environment;
  bound target; Proxy target and handler; typed-array buffer, offset, length
  and element type; Map/Set entry run; Date time value; RegExp program;
  Promise state and reaction run; generator continuation; module namespace
  bindings; error data).
- `shapes`: immutable layout records (parent shape, key, attributes, slot)
  with transition entries, so an object stores its keys once per shape.
  Writable, enumerable and configurable live in the shape entry. Objects
  with deletions or very many keys switch to dictionary mode (a key and value
  run of their own). An accessor property's slot holds an index into an
  `accessors` table of getter and setter values.
- `slots`: value runs for properties and dense array elements. Holes are
  `Empty`. Sparse or huge arrays use dictionary mode.
- `strings` and two unit tables: a string header holds its encoding
  (Latin-1 or UTF-16), start and length in `latin1: u8[>..]` or
  `utf16: u16[>..]`. Property keys are interned once per image, so shapes and
  lookups compare key IDs. Exact UTF-16 strings stay separate from source byte
  spans, as bbp's AGENTS.md requires. The string area may add a rope variant.
- `symbols`, `bigints` with a `limbs: u32[>..]` table, `envs` (closure
  environments: parent and slot run), `accessors`, `conts` (detached
  continuations, decision 4).

A closure is data: a function object whose kind payload is the function ID
and its environment (rule 4). Object identity is the `ObjId`.

**How it follows the rules.** Values are a compact fixed-mode enum (rule 3);
links are validated index types, position independent because they count
from their table's start (rule 2); every table is flat, so the image crosses
workers by copy (rule 6). Object kinds dispatch by `match` on the kind enum
(rule 4). Reads through an index type are free of bounds checks: in probe P7,
`objects[r.o]` lines report "2 elided, 0 kept", while slot-run reads keep one
check until runs use an index type too (rule 7).

**Evidence.** Probe P4 measures the value enum at 9 bytes (three values, 27
bytes); today's `source::Value` is 40. The probe interpreter (P7) runs the
real exported bytecode with this model; its map benchmark result is in
decision 5. Today's VM needs 1,073 nanoseconds an element on the same
benchmark at 100 elements, because every element access builds a decimal key
string and walks the object's property list (38,419 string units and 10,101
property records for one 100 by 100 run).

**Rejected.** *Pool-relative links* (`&<u32 in pool.items>`): unusable from
workers (fact 1). *Self-relative links in one image array*: verified for
straight-line code but rejected by the checker in interpreter-shaped code
(fact 2), and a fixed-mode value cannot hold them. *Plain `u32` or `i64`
indices into job-local root structs*: truncatable, but unchecked by the type
system, the raw-ID heap the user ruled out. *Plain 8-byte references*: not
position independent, so snapshots would need relocation. *NaN-boxing or a
separate small-integer tag*: the enum is 9 bytes whatever the number payload,
and Goose has no unions to box into.

## Decision 3: memory

**Decision.** Memory is reclaimed by thread program.

- A **session worker** owns the session image: the realm (decision 6), loaded
  namespaces and their initialized definitions, in its tables. They only grow
  for the session's life. (The memory pellet may instead give this role to
  today's coordinator; what matters is that the image owner is the program
  that spawns the job workers.)
- Each JavaScript job runs in a fresh **job worker** spawned by the session
  worker. The spawn copies the session worker's tables into the job worker
  (the reference: copies "are taken from the spawning program at the moment
  of the spawn"; `compiler_session.goose` already spawns workers from a
  worker): that copy is the snapshot restore. It is typed and valid by construction,
  so it needs no verification and no relocation, and there is no session
  identity in any value. The job appends above the copied image; its results
  leave as flat messages (forms); its exit frees everything it allocated in
  one step.
- Within a job nothing is reused: tables are append-only and an object whose
  run outgrows its capacity gets a new run (the probe regrew element runs 707
  times in one job without a free list). `slot_pool` and `slice_pool` remain
  the tool where reuse is needed in a non-global root, such as scheduler
  tables; they cannot be worker globals (fact 1).
- Images that arrive from elsewhere (a continuation resumed on another
  worker, a cached realm) are loaded as plain twin rows with `from_bytes`,
  then converted to index types in one checking pass with forward links
  patched after all rows exist. Probe P9 shows why: `from_bytes` checks a
  self-linked index-typed table against the live, empty destination table and
  refuses it. The pass checks every link once; it rewrites nothing.
- **Collection inside a job is evacuation, only if measured.** If a job's
  tables exceed its budget, the worker writes its live graph (reachable from
  frames, the promise job queue, the global object and the realm) as a
  compacted image, spawns a successor with it and exits. The memory pellet
  first measures unreachable rows at job end on the 200k project and on the
  harness; evacuation is built only if a target workload needs it. There is
  no mark and sweep over a global table.
- **Accounting.** Each table has a row limit and the job a byte limit
  (today's `Limits`, per table). A job worker reports its peak rows and bytes.
  Waiting job workers count against a retention budget below the thread cap;
  over it, the session worker asks a waiting job to write its continuation
  image and exit.
- **`trim_memory()`.** Job workers never call it: their exit returns their
  stacks. The long-lived session, input and supervisor programs call it after
  a namespace batch only if the memory pellet measures a benefit. bbp calls it
  nowhere today.

**Why it fits compile-time use.** The 200k project runs 5,226 source macro
entries per session. At 17 microseconds a spawn that is about 0.09 s of
spawning, against 0.15 s of today's restore intervals, and the copy replaces
today's 995 MB of copying and identity relocation. The copy grows with the
session image, so the memory pellet measures image size and spawn cost on the
200k project before the old restore path is removed. Long-running JavaScript
(full programs, the harness) is the case evacuation exists for.

**Rejected.** *Long-lived workers that reset between jobs*: index-typed
tables cannot shrink (fact 3), so a reset is impossible without raw IDs.
*A per-session append-only heap shared by all jobs*: jobs would see each
other's mutations. *Copy-on-write of realm objects into a job overlay*: two
kinds of object link and a lookup on every access. *Reference counting or a
tracing collector over the tables*: ruled out by the user.

## Decision 4: control flow

**Decision.** Everything the interpreter suspends or unwinds is data in the
job worker, and the dispatch loop is one non-recursive function (rule 5):

- **Frames** (function, pc, local base, operand base, receiver, `new.target`,
  construct flag) live on job stacks. Native built-ins that run JavaScript
  push a native frame with explicit state and return to the loop (decision 5).
  The call depth limit is a VM limit, not the native stack: 10,000 frames and
  1,048,576 values, the Pants engine's `VmLimits::DEFAULT`
  (`rust/src/vm/frames.rs`), failing as that engine does.
- **Exceptions and finally.** The handler stack holds catch and finally
  entries (frame, pc, operand height). `finally` follows the Pants opcodes:
  `PushFinally`, `PopFinally`, `EndFinally`, `CallFinally`,
  `DropCompletion`, `PendingThrow`, `PendingException`, with completion
  records as data. Unwinding crosses native frames, which clean up their own
  state.
- **Generators and async functions.** A yield or await detaches the
  activation: its frames, locals, operands and handlers are copied into a
  continuation record (`conts` plus a slot run); resumption copies them back.
  This is the Pants engine's "movable VM activation" (`vm/generator.rs`,
  `DetachedActivation`), and today's helper-suspension continuation
  generalised. The record is flat and index-linked, so it moves with the
  image.
- **Promise jobs.** A FIFO table of reactions in the job worker, drained when
  the frame stack empties. A job completes when its entry returns and the
  queue is empty; an entry that returns a promise completes when that promise
  settles.
- **Dependency suspension.** A job that needs a compiler dependency saves its
  continuation in place and blocks on its queue; its state stays in its own
  thread program. Over the retention budget it writes a continuation image
  instead (decision 3). Message routing reuses the existing re-queue pattern
  (`compiler_session.goose`, `cancelled`).
- **Cancellation** arrives at poll points (budget slices) and unwinds as an
  abrupt completion that `catch` cannot intercept but that runs `finally`
  blocks and `using` disposal, the Pants `PendingCancel` contract; then the
  worker exits.

**Rejected.** *Goose recursion per call with `exception::try` for throw*:
the native stack is 8 MB on the macOS main thread, a handler cannot be
re-entered recursively, and suspension would need the native stack saved.
*Running generators in separate worker programs*: a message round trip per
`next()` and a heap split across programs.

## Decision 5: built-ins

**Measurement.** Array map built both ways on the probe interpreter (P7),
over the real Rust-frontend bytecode of this function, with a callback
`(x, i, o) => x * 2`:

```js
function map(a, f) {
  const n = a.length; const out = [];
  for (let i = 0; i < n; i++) out[i] = f(a[i], i, a);
  return out;
}
```

| Engine, map variant | 100 elements | 1,000 elements |
| --- | ---: | ---: |
| Probe, map in JavaScript on Goose | 47.1 ns | 43.2–43.7 ns |
| Probe, map as a Goose native frame | 20.3 ns | 17.0–17.4 ns |
| Today's source VM, map in JavaScript | 1,072.8 ns | not run (quadratic) |
| V8 (node 24), map in JavaScript | 8.64 ns | 3.35 ns |
| V8 (node 24), `Array.prototype.map` | 6.19 ns | 5.58 ns |

Nanoseconds an element, each run 100 rounds of map; the best run of three
(Goose at 100 elements) or five (Goose at 1,000, V8) in one process, the
1,000-element probe rows giving the range over three processes. Goose rows
are native release builds, the probe with one job worker per run; the load
average was about 1.5 on an otherwise idle machine, so these are diagnostic
figures, not a quiet measurement. Both probe variants return checksum 199,800 (1,000) and
19,800 (100). The native frame calls the same JavaScript callback through the
same dispatch loop. Writing map in JavaScript costs 2.5 times the native
frame, because its loop runs about 24 interpreted instructions an element
around the callback.

**Decision.** Built-ins are written in Goose. Each built-in is a variant of
a `Builtin` enum dispatched by `match` or case functions (rule 4); a built-in
function object is an object whose kind payload is its `Builtin`, with
`name` and `length` installed by the realm builder. The rules areas follow:

1. A built-in that can run user code (callbacks, getters, `valueOf` and
   `toString`, Proxy traps, iterator protocol, species constructors) is a
   resumable native frame with explicit state, re-entering the dispatch loop.
   It never calls the interpreter recursively.
2. A built-in that cannot run user code is a plain Goose function over the
   tables.
3. Port semantics from the Pants engine's native built-ins
   (`rust/src/vm/native.rs` and `vm/native/`), whose `NativeState` enum
   already is this resumable design, and cite the QuickJS case that tests it.
4. Self-hosting a built-in in JavaScript is an exception an area must justify
   with a measurement and a reason (for example a large, cold algorithm); it
   needs the private intrinsic opcodes that self-hosted code would require,
   which the exporter does not have today.

**Rejected.** *Self-hosting by default*: 2.5 times slower on the measured
case; it needs private intrinsics hidden from ordinary code, a second compile
path for the realm, and `Function.prototype.toString` and stack traces that
pretend the code is native; and it would rewrite semantics that the Pants
engine already specifies as resumable native code.

## Decision 6: realm setup

**Decision.** The realm builder is Goose code that runs once, in the session
worker, before any job: the global object, `Object.prototype`,
`Function.prototype`, the Error constructors, `Array.prototype`, the
well-known symbols and every intrinsic the areas add, as the first rows of
the tables. Job workers inherit it through the spawn copy (decision 3), so
"build once and snapshot" costs one table copy a job, consistent with today's
initialized-snapshot design. Large immutable data (Unicode and Intl tables)
is static `let` data rather than table rows, so it is never part of the
mutable image; whether static data is shared rather than copied at spawn is
unverified, and the realm pellet measures it. The realm pellet records the
realm's rows, bytes and build time, and the spawn cost they add.

**Rejected.** *Building the realm lazily per job*, as the Pants engine does
on first use: every job would pay for it. *Building it in JavaScript*:
decision 5.

## Decision 7: conformance harness

**Decision.** A Babashka runner, `crates/big-boy-pants-js/scripts/conformance.bb`,
runs each case on Goose, the Pants engine and QuickJS and prints a per-case
ledger grouped by the 14 areas of bbp-34 to bbp-47 (light-cljs-762 to -775):

- **Cases.** The Pants engine corpus (`rust/tests/fixtures/js`, 73 files and
  `quickjs_modules/`); its QuickJS-derived files, labelled from their headers
  ("Adapted rows: none" is unmodified text with an added header); and
  unmodified `~/src/quickjs/tests/test_*.js` files, run one test function at
  a time with the file's own `assert` helpers. `test_std.js`, `test_worker*.js`,
  `test_rw_handler.js` and `test_bjson.js` are excluded as QuickJS host
  facilities, following bbp-33; `microbench.js` is a benchmark. Pants fixtures
  that exercise its scheduler host (`concurrency_*`, `need_*`, `fold_*` and
  the like) are labelled host-specific with their reason, not counted as a
  JavaScript area. Every exclusion is listed in the ledger with its reason.
- **Area map.** A checked-in EDN file assigns each case to one area, taken
  from the case lists in bbp-34 to bbp-47.
- **Outcomes.** Goose: pass, fail (throw, wrong result or disagreement with
  the oracles) or explicit-unsupported (the named opcode or built-in, at
  export or at execution). Pants engine and QuickJS: pass or fail. A case that
  the oracles disagree on is flagged rather than scored.
- **Regression check.** `--check` compares Goose passes with a checked-in
  list and fails on any regression, so every area pellet can name the cases
  its acceptance requires. The ledger itself is generated output in `target/`,
  not a backlog.

**Rejected.** *A test262 campaign now*: a later, separately queued tier.
*A Rust test runner*: harness scripts are Babashka (AGENTS.md).

## Stack settings

Defaults are kept: 2 GB reserved per data stack (`--stack-reserve`), at most
1,024 data stacks per thread program (`GS_MAX_STACKS`), 32 TB for all thread
programs (`GS_STACK_BUDGET`), 1 MB guard gap. Reservations are address space
until used.

`goose check --stacks` on today's runner (`src/compiler_session.goose`):
the main program uses 50 data stacks plus 1 global stack, `worker` 47,
`input` 2, `supervise` 2; the thread cap is 347 workers. The probe
interpreter (P7) needs 18 regions for its main program and 8 for a job worker
(cap 2,044).

On the 200k project, a runner built with `-DGS_STACK_STATS=1` ran the
`compare.bb --backend all --phase check --runs 1` gate (every check passed;
Goose 1/2/4/8 took 14.7/10.2/7.2/7.3 s and V8 5.9 s, not a timing run). In
this Goose version the report gives stacks and regions per thread program,
not bytes used. The 8 runner processes (warmup and sample of each Goose cell)
reported 50 data stacks and 51 regions for the main program, 30 job workers
of 47 stacks (1 + 2 + 4 + 8 per process pair) and 38 workers of 2 stacks; no
thread program came near `GS_MAX_STACKS` or the 347-worker cap. Peak memory
in bytes is the measurement phase's job (`time -l`), not this report's.

The design adds no Goose recursion per JavaScript call (decision 4), so the
8 MB native main-thread stack bounds only the remaining recursive functions
(form equality, validation), all depth-limited. Nothing measured justifies a
non-default setting. A job worker's own stacks are released when it exits, so
the spawn-per-job model raises the number of reservations made, not the
number held; the memory pellet records the counts for job workers and the
harness pellet records the harness's peaks.

## Un-Goose risks

Parts that still resemble a conventional heap, and why they stay:

- **Index-typed tables are object IDs.** They are Goose's validated index
  types, checked at construction and free of bounds checks on use, and the
  only typed link that works on workers (facts 1 to 3). Within a worker a
  stale ID cannot exist, because tables never shrink; across images the
  checking conversion pass is the trust boundary.
- **Append-only tables are a bump allocator per job.** Garbage is freed only
  at worker exit. Long-running JavaScript grows until evacuation, which is a
  whole-image copying step into a fresh worker, built only if measured.
- **Shapes and dictionary-mode objects** are the conventional engine
  structures; they keep keys once per shape and make property order and
  attributes explicit. Inline slots keep small objects in their header.
- **Weak references and finalization.** No collection happens inside a job,
  so a `WeakRef` target never dies and a `FinalizationRegistry` callback
  never runs during one. The specification permits this, but tests that
  expect collection will differ; light-cljs-769 owns the classification.
- **Per-spawn image copies** grow with the session image. If it reaches
  hundreds of megabytes, the copy dominates job cost; static data and a
  measured image budget are the mitigations.
- **Process-wide typed queues** need routing of answers to the right job
  worker; the existing re-queue pattern is kept, not a mailbox layer.

## Appendix: probes

Throwaway programs, run with the Goose above; each line gives the rule
checked and the result.

- **P1.** A fixed-mode enum with a `Cell&<u32>?` payload is rejected: "a
  fixed-mode Cell binds its payloads only by value (§3.5): use it in variable
  mode (Cell..), whose payloads bind by reference, or make the references
  `in pool`".
- **P2.** A worker using a global `Obj[>..]` whose elements hold
  `Obj&<u32 in objects>?` is rejected: "thread programs may access only flat
  globals, and `let` globals whose references lead only to static data
  (§11.2): objects holds references and is a var".
- **P3.** `type ObjId = u32 in objects` over flat globals works in a worker;
  the worker sees its own copy of main's rows taken at spawn plus rows it
  restores with `from_bytes`; main's table is unchanged.
- **P4.** Three `Value`s (f64 payload) take 27 bytes; an enum with an `i32`
  payload takes 5. Truncating a flat array to a mark works;
  `objects.resize(mark)` on an index-typed table is rejected: "an index type
  names objects, so its length never goes down (§3.11)".
- **P5.** One `Cell..[>..]` image with self-relative links: built in main,
  sent as `to_bytes` bytes, verified with `from_bytes` in a worker, extended
  with cells linking into the restored prefix, relinked in place inside a
  payload's slot run, returned and verified again. `heap.resize` on it is
  rejected: "Cell..[>..] is sequential".
- **P6.** A runtime-length run of slots holding self-relative links can be
  built by a function returning a pushed local (`resize` with a fill is
  rejected: "copying a value of type Slot, which contains self-relative
  references, is not supported").
- **P7.** The probe interpreter: `source::load` decodes the exported
  benchmark, a fixed-mode `Ins` enum and `Value`, frames as data, a native
  map frame, one job worker per run. With self-relative links every
  relinking store in helpers and operand-stack flows was rejected ("rooted at
  target", "root is not known exactly, only that it outlives heap"); with
  index types it compiles and runs. `--bce-lines`: 38 of 90 index checks
  elided; every `objects[id]` line keeps none.
- **P8.** `from_bytes` of a table of index-type links verifies them in the
  worker; 200 sequential spawn-and-join cycles of an empty worker average
  16,860 ns.
- **P9.** `from_bytes` of a table whose rows link into the same table, into a
  worker whose table is empty, returns false, so cross-worker images are
  restored through plain twin rows and a checking conversion.
