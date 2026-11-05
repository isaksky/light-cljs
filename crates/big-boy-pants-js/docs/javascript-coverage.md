# Essentially complete JavaScript on the Goose VM: architecture decisions

November 5, 2026 (light-cljs-761). This document fixes how bbp's Goose VM
(`src/`) grows from the macro subset it runs today to essentially complete
JavaScript, measured case by case. It records seven decisions, the evidence
for each and the alternatives rejected. The foundation and harness pellets
(light-cljs-782 to -790, group `goose-javascript`) implement them; the area
planners (light-cljs-762 to -775) build on them. Pellets holds the work; this
file holds the reasons.

Verified with Goose `v2026.3-267-g369c7a2c` (runtime 3025e5810379). Every
Goose rule relied on below was checked with a small `goose check` or run
program (the probes, summarised in [the appendix](#appendix-probes)), not
taken from documentation alone.

## Starting point

- The Rust frontend (bbp's Rust implementation, `rust/`) parses and compiles
  JavaScript to the bytecode of `rust/src/vm/instr.rs`: 198 opcodes in an
  append-only registry (0x01 to 0xC6). Its Goose exporter
  (`rust/src/frontend/goose.rs`, `supported`) accepts 93 of them and refuses
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
with a fault that names the opcode. Such a fault is a VM outcome, never a
JavaScript exception, so `catch` cannot swallow it and the harness always
sees it as explicit-unsupported; the same holds for any unimplemented part of
a built-in. Constants gain Symbol and BigInt tags and
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

*Amended by light-cljs-792:* a job worker now serves many jobs and is reset
to the image before each one; see
[State isolation](#state-isolation-comparison-with-shadow-cljs). The bullets
below describe the one-worker-per-job design that light-cljs-784 built.

- A **session worker** owns the session image: the realm (decision 6), loaded
  namespaces and their initialized definitions, in its tables. They only grow
  for the session's life. (The memory pellet may instead give this role to
  today's coordinator; what matters is that the image owner is the program
  that spawns the job workers.)
- Each JavaScript job runs in a fresh **job worker** spawned by the session
  worker. The spawn copies the session worker's tables into the job worker
  (probe P10: a job worker spawned by an image-owning worker sees its rows
  and links, mutates its own copy, and leaves the owner's rows unchanged):
  that copy is the snapshot restore. It is typed and valid by construction,
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
  compacted image and sends it to a program whose VM tables are empty (the
  coordinator when a session worker owns the image, otherwise a dedicated
  clean spawner), then exits. That program spawns the successor, which starts
  with empty tables and restores the image through the checking conversion
  below. The old worker must not spawn its own successor: the spawn would copy
  its full, unshrinkable tables, garbage included. During evacuation the old
  heap, the image and the successor's copy coexist, and that peak is charged
  to the retention budget. Images from elsewhere (below) likewise go only to
  workers spawned by a program with empty VM tables. The memory pellet
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

**Implemented (light-cljs-784, `src/compiler_session.goose`).** Measurements
are in [the evidence](../evidence/generated-200k-performance.md), section
"One job worker per job".

- *Image owner: the coordinator* (the runner's `main`), not a dedicated
  session worker. It already holds the realm, the base snapshot, the prepared
  definition snapshots and the identity authority, and P9 requires the
  spawning program to own the tables; a separate session worker would add a
  hop to every dispatch. It is the only program that spawns job workers.
  Probe P11 shows a spawn copies only the globals the spawned thread program
  reaches, so when light-cljs-785 makes the realm the coordinator's flat
  global tables, the input thread and the reapers (which reach none of them)
  copy nothing, and each job worker copies the image as its restore.
- *One contained job worker per dispatch* (`run_job`): a start, a helper
  production and a resume of a saved continuation each get a fresh worker
  with empty tables, which restores today's flat `snapshot::Snapshot` from
  its spawn arguments. Nothing is reset or reused between jobs.
- *Reapers* (`supervise`, `limit + retention` of them) only join job workers
  and report how each ended. The coordinator keeps at most that many workers
  live, so every worker's join is taken at once and a contained abort is
  reported while its peers run. Abort, cancellation, the input/readiness
  protocol and the queue limits are as before.
- *Dependency waits block in place by default.* A worker that needs a helper
  publishes the wait and blocks with its state in its own thread program; the
  coordinator answers it through the `Answer` queue, routed by worker address
  with the re-queue pattern of `cancelled()`. A blocked worker counts against
  the *retention budget* (`BBP_RETAINED_WORKERS`, default 32, at most 128);
  over it the coordinator answers "save", and the worker writes its
  continuation and exits as before. Measured on the 200k project, blocking
  beats saving at 1/2/4/8 workers and copies 154 MB instead of 996 MB per
  session.
- *Accounting*: at most `limit` workers execute; blocked workers do not, but
  hold a retention slot until answered, cancelled or aborted; live workers
  never exceed the reaper count. The peak thread count is `2 + 2 × (limit +
  retention)` at most (274 at limit 8 and retention 128), below the 354-worker
  cap `goose check --stacks` reports. The `JOBS` trace line reports the
  totals, and the tests assert no worker or slot outlives the session.
- *Spawn cost*: a job worker has 46 data stacks, and Goose sets up and tears
  down each region per thread (probe P12: about 7.5 microseconds a data stack,
  345 microseconds for a 44-stack worker against 26 for one stack). That, not
  the 17 microseconds of P8, is the price of the scope, and it makes the 200k
  project slower; see the evidence. light-cljs-791 works on that cost.
- *Collection*: the census (`src/census.goose`, instrumented runs only)
  found no target workload near its job limits, so no evacuation is built.
- *`trim_memory()`*: measured after each idle batch in instrumented runs
  only; see the evidence for why nothing calls it otherwise.

**Why it fits compile-time use.** The 200k project runs 5,226 source macro
entries per session. At 17 microseconds a spawn that is about 0.09 s of
spawning, against 0.15 s of today's restore intervals, and the copy replaces
today's 995 MB of copying and identity relocation. The copy grows with the
session image, so the memory pellet measures image size and spawn cost on the
200k project before the old restore path is removed. Long-running JavaScript
(full programs, the harness) is the case evacuation exists for.

**Rejected.** *Long-lived workers that reset between jobs*: index-typed
tables cannot shrink (fact 3), so a reset is impossible without raw IDs.
(Withdrawn by light-cljs-792: a reset rewrites rows and moves an allocation
top back, and nothing shrinks.)
*A per-session append-only heap shared by all jobs*: jobs would see each
other's mutations. *Copy-on-write of realm objects into a job overlay*: two
kinds of object link and a lookup on every access. *Reference counting or a
tracing collector over the tables*: ruled out by the user.

## State isolation: comparison with shadow-cljs

November 5, 2026 (light-cljs-792). Does one fresh job worker per job
(light-cljs-784) isolate more state than light-cljs needs? **Yes.** Every job
must start from the clean image, but nothing needs a new thread program per
job, which is what costs 5,283 sets of region setup a 200k session. This
section amends decision 3; light-cljs-795 and -796 implement it, and -785,
-787, -788, -790 and -791 carry "Amended by light-cljs-792" notes.

### What shadow-cljs shares

Paths are under `research/shadow-cljs/src/main/shadow/` (3.5.5) unless named.

- **Macro namespaces load once per process.** `build/macros.clj`
  `load-macros` (86-126) requires each macro namespace under one global
  `require-lock` (84), only if it is not yet in the process-wide
  `active-macros-ref` (20): "since we can't isolate macros they will be
  reloaded for all builds" (13-19). Watch mode reloads a changed macro file
  once for every build (`cljs/devtools/server/reload_classpath.clj` 66-108).
- **Macro state survives everything.** Expansion calls the macro's var in
  the one heap of the Java virtual machine (`research/clojurescript/src/main/
  clojure/cljs/analyzer.cljc`, `macroexpand-1*`, 4293-4309), so
  macro-namespace vars, atoms and `defonce` values persist across calls,
  namespaces and builds. `gensym` is the process-wide counter `RT.nextID`
  (`research/clojure/src/clj/clojure/core.clj` 607-614,
  `src/jvm/clojure/lang/RT.java` 274 and 500); shadow's `reify` type names
  use it (`build/cljs_hacks.cljc` 784).
- **One compiler environment per build, shared by threads.**
  `with-compiler-env` binds one atom (`build/cljs_bridge.clj` 123-142);
  `par-compile-cljs-sources` (`build/compiler.clj` 1278-1361) gives one task
  per namespace to a pool of `availableProcessors` threads
  (`cljs/devtools/server/common.clj` 76-83), conveying that atom with
  `bound-fn` (1338); a task polls every 5 ms for its requires (1239-1262).
  Per namespace only thread-bound analyzer variables are rebound
  (`do-compile-cljs-resource`, 731-775: `*file-defs*`, warnings, and
  `*unchecked-if*` "so it is thread bound, since the analyzer may set! it").
- **Determinism is partial.** Outputs are reassembled by resource
  (1353-1360) and warnings kept per namespace, but nothing orders macro side
  effects or the gensym counter across threads. Namespaces that rely on macro
  side effects, and their direct dependents, skip the cache
  (`:cache-blockers`, default `clara.rules` and `clara.macros`: `build/api.clj`
  110-117, `build/compiler.clj` 933-942). Upstream ClojureScript's
  `:parallel-build` also shares one environment, with a global `load-mutex`
  (`cljs/analyzer.cljc` 639, `cljs/closure.clj` 1048-1100).

So shadow-cljs has no clean world per namespace, still less per macro call:
one world per process for macro code and state, one compiler environment per
build, and a thread pool over namespaces.

### What light-cljs requires

1. **Byte-identical output at every worker and slot count**, which
   shadow-cljs does not promise. A job's result may depend only on the
   session image, its arguments and the helpers resolved for it, never on
   which worker ran it or what ran there before
   (`docs/compiler-concurrency.md`, "Semantics and determinism"). The V8 path
   kept persistent isolates and declared a macro's compile-time side effects
   visible only in the isolate that made them (light-cljs-429), which keeps
   output identical only for pure macros. Goose today keeps it for every
   macro, because no job sees another's writes. Runtime `gensym` and counter
   effects, which would need sharing, are not serviced on the compiler path:
   `execute` in `src/compiler_session.goose` serves only root reads and
   faults the rest with `NativeBoundary` (test
   `goose_compiler_assembly_leaf_boundaries`); compile-time generated names
   stay compiler-owned. `src/effects.goose` is the scheduler's separate,
   coordinator-ordered implementation.
2. **Root visibility as V8 had it** (light-cljs-776). The compiler resolves
   `__lightCompilerRoots[name]` at the job's visibility cursor and the job
   writes the answer into the roots object. A world reused across jobs may
   cache a helper by its unit key, never by name.
3. **Contained failures.** A VM fault, a limit or a cancellation ends one
   job; a contained Goose abort ends the session (`supervise` in
   `src/compiler_session.goose`).

The minimum is therefore: **every job starts from the pristine image, and
nothing it writes or allocates is reachable from a later job.** Nothing in
light-cljs requires a fresh thread program for that.

### What Goose's memory model requires

- No collector: garbage goes only when a scope ends. A 200k job leaves 98.7%
  of its nodes unreachable (median 5,841 rows; evidence, "One job worker per
  job"), so a world never reset grows without bound; the retained serial
  control's cumulative session work quota already failed at the fifteenth
  call of the heavier expanded workload (`evidence/expanded-results.md`).
- An index type names only a global array (P13), which never shrinks (P4,
  P14), so a nested Goose scope cannot free a job's rows; a thread program's
  exit can, and so can rewriting them in place later (P14).
- Every growable array a thread program reaches, global tables included, is
  a data-stack region mapped and unmapped at each spawn and exit (P12, P15);
  making the tables globals (light-cljs-785) keeps the region count.

A fresh thread program per job is a design choice. Decision 3 rejected
long-lived workers because "a reset is impossible without raw IDs", but a
reset needs only rewritten rows and an allocation point moved back.

### Options

Costs on the 200k project: 5,283 dispatches a session (5,226 macro starts and
57 helper productions) from light-cljs-784's JOBS lines, and per job from
P16 (native build, 1,000 jobs, workers of 44 to 48 regions).

| Option | Persists across jobs | Determinism and failures | Spawns a session | Isolation cost a job |
| --- | --- | --- | --- | --- |
| (a) a fresh worker per job (784) | nothing | by construction; exit frees all | 5,283 | 361 µs at 1 worker (132 spawn to first instruction + 229 last publication to join), 2.7 ms at 8; P16 388-397 µs |
| (a') (a) with fewer regions (791) | nothing | as (a) | 5,283 | about 7.5 µs a region, and every table and job-local array is one (P15), so still over 100 µs a job for the tens of tables decision 2 adds |
| (b) a long-lived worker per slot, job tables in a nested scope | the image | needs the image rows restored | at most limit + retention | P16's truncated control: 6-24 µs; but index types cannot name local tables (P13), so links become raw integer IDs, which decision 2 excludes |
| (c) a worker per namespace or batch | the image and the batch's garbage | carrying state across a namespace's jobs is deterministic only if they all run in order in one worker, which the scheduler does not promise (deferred units, helper productions); with a reset it is (d) with more spawns | about 500 (199,129 lines at the generator's 400 a namespace) | a tenth of (a), plus garbage that grows with the batch |
| **(d) a reused worker with a job arena** | the image | the image is restored before every job, so each job sees what a fresh worker would, at the same rows; a VM fault's state goes at the next reset; an abort ends the session as now | at most limit + retention (784 measured peak live workers 2/3/8/16), plus retirements | P16: 13 µs (the queue round trip), plus the prefix restore (about 17 µs for a 1.28 MB image); pages reused |
| (e) one shared world without reset (shadow-cljs) | everything | macro state differs between workers and with history; garbage unbounded | at most limit | none, but it breaks requirement 1 and memory |

The 784 regression (+1.945/+1.419/+0.664/+1.366 s at 1/2/4/8 workers; system
CPU 3.03 to 5.76 s at one worker and 4.88 to 22.14 s at eight) is the cost in
row (a). Under (d) the projected isolation cost is about 5,283 × 15 µs,
roughly 0.1 s a session, against about 1.9 s; light-cljs-795 measures it.

### Decision

Decision 3 is amended. The thread program still owns an image and frees its
tables when it exits, but a job worker serves many jobs, and **a job is
reclaimed by resetting a job arena**:

- The coordinator, the image owner, spawns contained job workers on demand,
  at most `limit + retention` live, and gives each job to an idle one. Its VM
  tables do not change after `READY`, so every worker, whenever spawned,
  inherits the same image (P10, P11).
- A worker copies its image prefix (each table's rows up to the image
  length, and one worker-global VM state struct) into local pristine twins
  once. Before each job it restores both from the twins and sets every
  table's allocation top back to the image length; the job appends at the
  top, overwriting rows a previous job left (P14, P16), and within a job
  nothing is reused. Frames, operands, bindings and bridge buffers are
  locals of the per-job function and go when it returns.
- Limits count the rows a job added above its starting tops. A worker retires
  after a job once its tables exceed a byte cap, and the coordinator spawns a
  successor from its unchanged image. A cap of zero retires a worker after
  every job, which is (a) and serves as the differential control.
- This is reclamation by the job's scope, not a heap allocator: the top is a
  bump pointer reset when the job ends, like a Goose data stack; no free
  list, per-object reuse or sweep, and links stay index types. A job's rows
  start at the coordinator's image length in every worker, so its heap
  layout is a fresh worker's, and a saved continuation restores into any
  reset worker without rebasing (its checking conversion validates links
  against the tops).
- **What may persist:** the image only (the realm, the runtime leaves, and
  the roots and helper-cache objects in their initial state). Prepared
  definitions and installed helpers may stay loaded in a worker if they are
  keyed by definition or unit key, out of JavaScript's reach, and added
  between jobs; light-cljs-796 measures whether that pays. Such an extended
  prefix differs between workers, so two rules keep jobs independent of
  history: a cache hit charges the job the rows and work its installation
  recorded, so limit outcomes match a cold install; and a saved continuation
  carries every row above the coordinator's image that it reached, cached
  ones included, with its cache bindings, and resumes in a worker reset to
  the coordinator's image, whose own extension the next reset restores.
  Nothing keyed by root name, no write a job made to image rows and no row a
  job added survives the job.
- **Rules for the representation pellets** (light-cljs-785, -787, -788,
  -790): every piece of VM state a job can change is a table row, a field of
  the one worker-global state struct, or job-local; no lookup structure
  (interned keys, shape transitions) keeps a job's additions anywhere the
  prefix restore does not cover; the VM reads tops, not physical table
  lengths. In tests a reset check compares the prefix and the struct with the
  twins after every reset (light-cljs-795).

Unchanged: light-cljs-794 (its premise, one image per thread program for its
whole life, holds), -786 and -789 (they assume nothing about worker lifetime).

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
4. A step a built-in does not implement yet (species, a Proxy trap, a
   locale) is a named unsupported VM fault, never a placeholder `TypeError`
   that user code could catch (decision 1).
5. Self-hosting a built-in in JavaScript is an exception an area must justify
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
the tables. Job workers inherit it through the spawn copy (decision 3, probe P10), so
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
  A Pants fixture is one case, so a fixture that tests many methods cannot
  show a foundation pellet's partial progress; bbp-authored cases under
  `crates/big-boy-pants-js/conformance/cases/`, labelled as such with the
  fixture lines they derive from, cover those slices, and the full fixture
  stays in the ledger for its area.
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

With one job worker per job (light-cljs-784), `goose check --stacks` on the
runner: main 51 data stacks plus 1 global stack (52 regions), `run_job` 46,
`supervise` (now a reaper) 2, `input` 2; the thread cap is 354 workers. A
runner built with `-DGS_STACK_STATS=1` compiled the 200k project once at
each of 1/2/4/8 workers (November 5, 2026, plain mode, retention 32): every
run's main program reported 51 data stacks and 52 regions, each of its 5,283
job workers 46 stacks and 46 regions (21,132 reports), and each input thread
and reaper (147 reports) 2. At most 2 + 2 × (limit + retention) threads are
alive at once, at most 274 with the caps, so nothing comes near
`GS_MAX_STACKS` or the thread cap and the defaults stand. The spawn-per-job
model does pay per region at every spawn and exit (probe P12); a smaller
reserve saved only a fifth of that, so it does not justify a change either.

The conformance harness (light-cljs-782, decision 7) runs one Goose runner
process per case, `src/conformance.goose`, at most `--jobs` (default 4) at
once. `goose check --stacks` on it: the main program uses 25 data stacks plus
1 global stack, 26 regions, and starts no workers, so it sets no thread cap.
A runner built with `-DGS_STACK_STATS=1` ran the whole ledger once
(`conformance.bb --stack-stats`, November 5, 2026: 181 cases, 147 run on
all three engines, 34 excluded). Only 11 cases got past the exporter; each of
those runner processes reported the same 25 data stacks and 26 regions. The
other 136 were refused at export by name and never started the runner, so
these figures describe today's VM only; `--stack-stats` measures them again
once more cases run. Nothing came near `GS_MAX_STACKS`; the defaults stand.

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
- **The job arena reset** (light-cljs-792). Any VM state the reset misses
  would leak one job's writes into the next on the same worker. The reset
  restores whole table prefixes and one state struct rather than logging
  writes, and a reset check compares them with their pristine twins in tests.
- **Per-spawn image copies** grow with the session image (since
  light-cljs-792 once per worker, with a prefix restore per job). If it reaches
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
- **P10.** A worker that owns an index-typed table (two rows, one linking to
  the other) spawns a job worker. The job worker sees both rows and the link,
  changes a row and appends one; after it exits the owner still has its two
  rows, unchanged.
- **P11** (light-cljs-784). A spawn copies only the globals the spawned
  thread program reaches: with an 88 MB global array in main, spawning and
  joining a worker that never names it took 23.6 microseconds, one that reads
  it 17.2 ms (an 8 MB array: 17.0 microseconds and 1.39 ms).
- **P12** (light-cljs-784). Spawn-and-join cost grows with the worker's data
  stacks: 26 to 33 microseconds for a contained worker with one data stack,
  340 to 351 microseconds for one with 44 (200 cycles, three rounds, native
  build). With `--stack-reserve 64M` the 44-stack worker still took 278
  microseconds, so the cost is per region, not per reserved byte.
- **P13** (light-cljs-792). An index type over a local array is rejected:
  "the array of an index type must be a global variable; a local or
  parameter has no name where a type is declared (§3.11)". A job heap in a
  nested scope cannot be linked by index types.
- **P14** (light-cljs-792). `clear()` and `pop()` on an index-typed global
  table are rejected ("cannot clear objects: an index type names objects, so
  its length never goes down (§3.11)"), but rows can be overwritten: three
  jobs each wrote image rows, linked one to the row they appended, and
  restoring the two image rows from a pristine local copy before each job
  undid every write, while the table kept all five rows.
- **P15** (light-cljs-792). `goose check --stacks` on a worker that reaches
  20 global growable tables and keeps 24 job-local arrays: "24 data stacks +
  20 argument and global stacks = 44 regions". A global table costs a region
  per worker, as a local array does.
- **P16** (light-cljs-792). 1,000 jobs of 64-byte rows, native build, three
  rounds; microseconds a job, including the queue round trip. A fresh
  contained 44-region worker per job: 388-397 with no rows, 410-414 appending
  6,000 rows (384 KB), 470-510 appending 18,000 (1.15 MB), 481-490 with a
  1.28 MB image. One 48-region worker reused, restoring a 28-row image prefix
  before each job and letting rows accumulate: 13.3-13.5, 41, 102-126. The
  same with a job arena (allocation tops reset to the image length, rows
  overwritten, in an index-typed table): 13.0-13.3, 22.5-22.9, 51-52, and
  28.7-33.2 restoring a 1.28 MB prefix. Tables without index types truncated
  after each job, option (b): 5.6-6.0, 10.7-12.9, 20.9-24.2.
