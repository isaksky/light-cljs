# Architecture

A concise map of the runtime's boundaries as implemented so far. Requirements
are in [`design.md`](design.md); the supported/deferred list and exact commands
are in the [README](../README.md). This note describes what exists and labels
planned shapes as planned.

JavaScript source in the documented subset ([`grammar.md`](grammar.md)) is
parsed and compiled to VM bytecode (`frontend::compile_script`,
`frontend::compile_function`; see "Source compilation" below). Programs built
with the `Assembler` are hand-assembled bytecode: an early VM test tool, not
JavaScript source support.

## Boundaries

```
 embedder threads (its own scheduler; the runtime owns none)
     │  Task::run_with(budget, &mut host) ──► RunOutcome
     ▼
 ┌─────────── Runtime (Arc, Send + Sync) ────────────────────────────┐
 │ shared Heap: object table, roots, scopes, root sources; mark/sweep│
 │ task id counter, world                                            │
 └───────────────────────────────────────────────────────────────────┘
 ┌ Executable (Arc<Code>) ┐      ┌ Task (Arc) ─────────────────────────┐
 │ validated instrs,      │◄─────│ id, world, Executable handle        │
 │ constants, max stack;  │shared│ atomic state word = execution claim │
 │ immutable, no heap refs│      │ Mutex<TaskBody>: Ready(VmState)|Fin.│
 └────────────────────────┘      └─────────────────────────────────────┘
 Rooted (persistent root) / Local<'s> (scoped, cannot escape Runtime::scope)
 Host (embedder trait object, borrowed for one run; leaf effects only)
```

- **Runtime** (`src/runtime.rs`): the shared heap and task bookkeeping. It
  spawns tasks and opens heap scopes; it never runs anything by itself.
- **Executable** (`src/vm/executable.rs`): code validated once at load and then
  shared read-only by every task running it (`Executable::same_code`). It holds
  no heap references, so it is not tied to one runtime.
- **Task** (`src/task.rs`, `src/vm/frames.rs`): all execution state lives in the
  task's `VmState`: an explicit stack of call frames (each with its code,
  saved instruction position and base in one task-owned value stack holding
  every frame's locals and operands), resource limits and diagnostic context;
  see "Calls and frames" below. A worker thread only borrows it for one run.
- **Handles** (`src/heap/roots.rs`): embedders never see raw values. `Rooted`
  is a persistent root (how results are returned); `Local<'s>` is valid only
  inside `Runtime::scope`; `WeakHandle` names a value without keeping it
  alive. A Rust local holding a raw value is not a root.
- **Host** (`src/host.rs`): the narrow embedder trait for leaf effects, passed
  as `&mut dyn Host` to one run.

## Run protocol

1. `Task::try_begin_run` claims the task with one CAS on its atomic state
   word (`Runnable → Executing`). It never blocks: a second runner gets
   `RunError::AlreadyRunning` (a parked task `RunError::Waiting`), so a task
   can never execute on two workers at once.
2. `RunGuard::run_with(budget, host)` runs the dispatch loop
   (`src/vm/dispatch.rs`) on the task's `VmState`.
3. The run ends with one `RunOutcome`:

| Outcome          | Task afterwards | Meaning |
|------------------|-----------------|---------|
| `Complete(root)` | `Finished`      | returned a value, delivered as a persistent root |
| `Failed(error)`  | `Finished`      | runtime failure with executable/task/world/instruction diagnostics |
| `Yielded`        | `Ready`         | the instruction budget ran out (or a park found its wake already delivered); state saved in the task |
| `Waiting(key)`   | `Waiting`       | parked by `RunGuard::park` on a pending need; only the completion's wake makes it `Ready` (below) |

4. The guard is released when the run returns. No Rust stack frame, borrow, or
   lock survives between runs, so the next run may happen on any thread.

**Budget and yield points.** `Budget::instructions(n)` bounds the instructions
one run executes; `Budget::UNLIMITED` (used by `Task::run`) does not. The
budget is checked only between instructions. Before an instruction executes the
loop advances the saved position, so on `Yielded` the position names the next
instruction (possibly a branch target just jumped to) and the operands and
locals are exactly what that instruction expects. Every instruction therefore
executes once, in exactly one run: a resumed run never replays an instruction
or its effect. A zero budget yields without executing anything.

## Leaf host effects

`HostEffect op` (opcode `0x11`) pops one primitive argument, calls
`Host::effect(HostCall { task, op, arg })` synchronously, and pushes the
returned primitive. This is the "leaf native operation" of `design.md` §10:

- it may not reenter the VM: while an effect runs, starting a run on that
  thread fails with `RunError::InsideHostEffect` (a guard acquired earlier
  fails its task instead), and `Runtime::scope` panics;
- it may not allocate or hold heap values across suspension: it sees only
  root-free primitives (an object argument fails the task with
  `UnsupportedOperand` before the host is called) and gets no heap access;
- it may not suspend: it can only return a value or an error (the error fails
  the task with `VmErrorKind::HostEffect`).

`Task::run` supplies no host; effects then fail the task.

Resumable natives that invoke JavaScript or wait on dependencies need a
task-owned native continuation instead of a Rust call; that is pellet
clj-quickjs-12.

## Heap and collection

The heap (`src/heap/`) is one object table per runtime, shared by all tasks,
collected by a precise, nonmoving, stop-the-world mark/sweep
(`Runtime::collect`, `src/heap/gc.rs`).

- **Identity.** An object is named by a crate-private `ObjectId` (slot index +
  generation). Each object is its own `Box` allocation that never moves.
  Sweeping a slot bumps its generation before reuse and every lookup checks
  it, so a stale id resolves to nothing rather than to a newer object; a slot
  whose generation would wrap is retired. Embedders see this as
  `HandleError::Reclaimed` from `WeakHandle::upgrade`.
- **Roots** (`RootCategory`): `Rooted` handles, open-scope locals (stored in
  the heap's scope table, not on the Rust stack), and registered root
  sources. A `RootSource` registers a `Weak` and is traced while alive; tasks
  register themselves and report their saved frames and their executable's
  constants. Heap objects report their own edges through `Trace`; a `Native`
  object's payload is a Rust value whose `Trace` reports its edges (the
  analogue of a QuickJS class `gc_mark`).
- **When it runs.** At a rendezvous where every other thread is outside heap
  access or stopped at a safepoint (below). `collect` refuses with
  `CollectError::MutatorActive` instead of waiting; `request_collection`
  waits. During the pause it holds every heap lock, so runs, scopes,
  allocations and root changes on other threads wait briefly. No lock is held
  across bytecode execution. Lock order: `gc` → `roots` → `scopes` →
  `sources` → task bodies → `objects` (a run holds its own body while taking
  `gc`; the collector never locks an executing task's body).
- **Ownership.** A task's allocations are not owned by the task: completion
  drops its frames, and what remains reachable (the returned `Rooted` and
  anything it references) survives. `Executable` code is independently owned
  immutable `Arc` storage outside the heap; its constants are primitives in
  this slice (strings included: a string is immutable `Arc` storage outside
  the heap too, see "String values").
- **No unsafe code.** Objects are reached only under the `objects` lock;
  reclaimed objects are dropped after all heap locks are released.

## Safepoints and mutator registration (clj-quickjs-17)

`src/heap/gc.rs` (registry, rendezvous, contract), `src/vm/dispatch.rs`
(poll points), `Scope::released` / `Scope::safepoint` (`src/heap/roots.rs`).
Design: `design.md` §11 ("how native/host code participates in
safepoints"). This replaces QuickJS's one-thread-per-runtime rule
(`research/quickjs/doc/quickjs.texi`, "Runtime and contexts", revision
7d1c13c5d8f8).

- **Registration.** Heap access is a per-thread registration in the `gc`
  lock's registry: `Access::Run` (a `RunGuard`, acquisition to drop),
  `Access::Scope` (an open scope), `Access::Op` (an atom operation, which
  has no safepoint inside). Tokens are not `Send`. Every allocation path
  reachable from outside the crate goes through one; a released scope
  cannot be reached (the release closure must be `Send`).
- **Stopped.** A thread is stopped when it holds no registration, is parked
  at a VM poll with its running frames reported, or is blocked in its own
  request. A collection needs every other thread stopped and the collector
  to hold no run guard or operation (its own scopes are roots).
- **Request.** `Runtime::request_collection` raises a pending count (and an
  `AtomicBool` mirror that polls read with `Acquire`, written under the lock
  with `Release`), waits on the `gc` condition variable until the rendezvous
  holds, collects, and withdraws the request. Threads *entering* heap access
  wait until a pending request is served; nested registrations never wait
  (the request is waiting for them). Waiters continue once their request is
  served even if another is pending (a served-request ticket), so
  back-to-back requests cannot starve them.
- **VM polls** happen at instruction boundaries only: run entry, backward
  branches, `Call`, `Return` and every instruction that may allocate. A run
  parks only if it is its thread's only run guard and no operation is in
  progress. The running task's root source skips its body (held by the
  parked guard): the runner reported those frames as `TaskFrame` roots.
- **Scopes** are the no-GC access scope for host code: a requested
  collection waits until a scope closes or is released. Releasing
  (`Scope::released`, or `Scope::safepoint` when a request is pending)
  drops the thread's scope registrations for the closure, then reenters
  like a new entry. Locals stay in the scope table, so they are roots and
  remain valid. A scope never spans a task suspension.
- **Atoms.** No collection completes while an atom operation runs, so the
  epoch-based grace period of replaced boxes still holds.
- **Deferred:** automatic collection triggers, worker-local allocation
  buffers, releasing access around long leaf host effects (a request waits
  for an effect to return), and parking a thread that holds two run guards.
  Worker integration: clj-quickjs-25 (below).

## Root inventory (clj-quickjs-31)

Every place a heap value can be held so that the collector must keep it
(design.md §11, "The collector must see"), the `RootCategory` it is
reached through, and its retention test (the value survives forced
collections while reachable only through that holder) and reclamation test
(released, it is reclaimed and every handle to it is rejected as stale,
also after its slot is reused). Tests are in `tests/gc_roots.rs` unless
marked *(lib)*: those are in `src/heap/inventory.rs`, for holders the
public API cannot reach.

| Holder | Root category (via object kind) | Retention | Reclamation |
|--------|---------------------------------|-----------|-------------|
| `Rooted` host handle | `PersistentHandle` (plain object) | `persistent_handle_retains` | `persistent_handle_reclaims` |
| open scope local | `ScopeLocal` | `scope_local_retains` | `scope_local_reclaims` |
| published task result | `PersistentHandle` | `task_result_retains` | `task_result_reclaims` |
| running frames (run parked at a VM safepoint) | `TaskFrame`, reported by the runner | `running_frame_retains` | `running_frame_reclaims` |
| suspended (yielded) frames | `TaskFrame`, task root source | `suspended_frame_retains` | `suspended_frame_reclaims` |
| task global object and its properties | `TaskFrame` (`PlainObject`) | `task_global_object_retains_and_reclaims` | `finished_task_releases_global_object` |
| frames suspended at a pending `__rt.need` | `TaskFrame` | `pending_need_frame_retains` | `pending_need_frame_reclaims` |
| executor ready queue | `TaskFrame` (queued task) | `ready_queue_retains` | `ready_queue_reclaims` |
| executor parked set | `TaskFrame` (parked task) | `executor_parked_task_retains` | `executor_parked_task_reclaims` |
| `finally` return/throw completions (handler stack), also across a dependency wait | `TaskFrame` | `handler_completion_retains` | `handler_completion_reclaims` |
| resumable native frames (`__rt.reduce` accumulator) | `TaskFrame` | `native_frame_retains` | `native_frame_reclaims` |
| transient builder drafts | `TaskFrame` (`Builder`) | `transient_builder_retains` | `transient_builder_reclaims` |
| dependency `Ready` value / `Failed` error | `DependencyOutcome` (`Vector`) | `dependency_outcome_retains` | `dependency_outcome_reclaims_with_its_cell` *(lib)* |
| atom current value | `PersistentHandle` (`Atom`) | `atom_current_value_retains` | `atom_current_value_reclaims` |
| atom replaced value held by a reader | `PersistentHandle` (load-to-root) | `atom_retained_old_value_retains` | `atom_retained_old_value_reclaims` |
| closure environment | `PersistentHandle` (`Function`) | `closure_environment_retains` | `closure_environment_reclaims` |
| captured mutable binding | `PersistentHandle` (`Function` → `Cell`) | `binding_cell_retains` | `binding_cell_reclaims` |
| persistent collection edges | `PersistentHandle` (`Map` → `Vector` → `Occurrence`) | `persistent_collection_edges_retain` | `persistent_collection_edges_reclaim` |
| executable object constants | `ExecutableConstant` | `executable_constant_retains` *(lib)* | `executable_constant_reclaims` *(lib)* |
| native payload edges | `PersistentHandle` (`Native`) | `native_payload_edges_retain` *(lib)* | `native_payload_edges_reclaim` *(lib)* |
| ordinary object property values (and prototype) | `TaskFrame` (`PlainObject` → `PlainObject` → argument) | `object_property_retains` | `object_property_reclaims` |
| realm intrinsics (`Object.prototype`, built-in functions and their properties) | `Intrinsic` (`PlainObject`, `Builtin`) | `realm_intrinsics_retain` | `realm_intrinsics_reclaim_with_their_heap` *(lib)* |
| realm template-site cache | `Intrinsic` (`Array`, cooked → raw) | `template_cache_retains_and_reclaims` *(lib)* | `template_cache_retains_and_reclaims` *(lib)* |

| JavaScript Map keys and values | `PersistentHandle` (`JsMap` → key and value objects) | `map_entries_are_traced` | `map_entries_are_traced` |
| array elements | `TaskFrame` (`Array` → `Array` → argument) | `array_element_retains` | `array_element_reclaims` |
| function own-property record (clj-quickjs-125) and its values, the lazy `prototype` object among them | `PersistentHandle` (`Function` → `PlainObject` → `PlainObject` → argument) | `function_properties_retain` | `function_properties_reclaim` |
| bound function target, bound receiver, bound arguments and immutable metadata (clj-quickjs-103) | `PersistentHandle` (`BoundFunction` → target closure / receiver / arguments / metadata) | `bound_function_retains` | `bound_function_reclaims` |
| concise method home object (clj-quickjs-126), including literal → method cycles | `PersistentHandle` (`Function` → `PlainObject` → argument) | `method_home_object_retains` | `method_home_object_reclaims` |
| engine Error's original internal cause, after replacing or deleting its own cause property (clj-quickjs-230) | `PersistentHandle` (`PlainObject` → cause) | `engine_error_cause_retains_and_reclaims` *(lib)* | `engine_error_cause_retains_and_reclaims` *(lib)* |

| typed-array backing buffer, ordinary properties and prototype | `PersistentHandle` (`TypedArray` → `ArrayBuffer`, property/prototype objects) | `typed_array_retains_buffer_and_properties` *(tests/gc.rs)* | `typed_array_retains_buffer_and_properties` *(tests/gc.rs)* |
| built-in iterator source internal slot | `PersistentHandle` (`PlainObject` cursor → source array) | `builtin_iterator_source_retains_and_reclaims` *(lib)* | `builtin_iterator_source_retains_and_reclaims` *(lib)* |

- **Cross-check.** `src/heap/inventory.rs` maps every `RootCategory` and
  every `ObjectKind` to its holders with exhaustive matches, so a new
  category or kind does not compile until it is mapped, and its test
  checks that every holder's two tests exist and are named in this table.
- **Multi-worker.** `every_holder_retains_while_other_workers_allocate_and_collect`
  keeps every public holder (and a scope local, released while waiting)
  alive while worker 0 runs an allocating loop without a budget and worker
  1 runs allocating tasks and requests every collection; each collection
  must report worker 0 stopped at a VM safepoint mid-run. A collection
  observer checks after each collection that nothing held was reclaimed,
  then every holder is released and everything it held is reclaimed.
  `crate_internal_holders_retain_while_other_workers_allocate_and_collect`
  *(lib)* does the same for object constants and native payload edges.
- **Not reclaimable while the runtime lives.** A dependency cell is never
  removed, so a published outcome stays rooted until the runtime and every
  task are gone; the reclamation test drops a cell table directly. Likewise
  the realm (clj-quickjs-117) lives as long as its heap; its reclamation
  test drops the realm slot directly.

## Persistent collections (clj-quickjs-3)

`src/collections.rs` (contract and embedder API on `Scope`),
`src/collections/vector.rs` (32-way radix trie), `src/collections/map.rs`
(HAMT). Design: `design.md` §§4–5.

- **Objects and bodies.** A vector, map or occurrence is one heap object
  (`ObjectKind::{Vector, Map, Occurrence}`) with ordinary stable identity.
  Its body is a tree of immutable `Arc` nodes outside the object table: the
  documented immutable-storage boundary. Nodes are shared between versions
  and have no identity of their own; the owning object's `Trace` walks its
  trie and reports every element, key and value, so all heap edges go
  through the collector's tracing boundary.
- **Updates** (`assoc`, `dissoc`, `conj`, `pop`) allocate one new object and
  path-copy O(log32 n) nodes; everything else, including nested collections,
  is shared by identity. A no-op update returns the original object.
- **Reads** take the `objects` lock once to clone the body's `Arc`, then walk
  the trie with no lock: no per-node locking. Map keys use SameValueZero
  (objects by identity) and a fixed private hash, so internal lookups never
  run user hashing, equality, getters or protocol code.
- **Equality, identity, provenance.** `===`/`same_object` is identity;
  `value_equals` is structural (iterative, provenance-blind: an occurrence
  equals its value); provenance lives only in occurrence objects, one per
  source location, never keyed by a scalar value.
- **Crate-internal layer.** Every `Scope` method wraps a `pub(crate)`
  function over raw values and `&Heap` (`new_vector`, `nth`, `get`,
  `assoc`, `index_from_value`, `value_equals`, ...). VM intrinsics
  (clj-quickjs-19, below) call these directly; their callers must be
  registered mutators (run guard or scope) so a freshly allocated result is
  stored before any collection can start.
- **Publication.** These kinds are `Persistent` in the publication rules
  (below): shareable exactly when their contents are.

## Collection intrinsics and transient builders (clj-quickjs-19)

`src/vm/intrinsics.rs` (the intrinsic set and its error contract),
`src/collections/builder.rs` (builder lifecycle). Design: `design.md` §§4–5.

- **Intrinsics are opcodes** `0x12`–`0x20`: `new_vector n`, `new_map n`,
  `count`, `nth`, `get` (strict: absent is an error), `get_or`, `assoc`,
  `dissoc`, `conj`, `pop_last`, and the builder operations
  `new_vector_builder`, `new_map_builder`, `builder_conj`, `builder_assoc`,
  `freeze`. They are not globals or properties; no JavaScript name reaches
  them and none runs user code. Operand counts are validated at load
  (`new_vector n` pops n, `new_map n` pops 2n); operand kinds, indices and
  keys are validated at run time and a rejection fails the task with
  `VmErrorKind::Collection(CollectionError)` or
  `VmErrorKind::Builder(BuilderError)`, never a panic. Operands stay on the
  operand stack (rooted by the frame) until the result exists.
- **Builders are task-owned heap objects** (`ObjectKind::Builder`): owner
  `TaskId`, kind, and a state `Open(draft) | Frozen | Released`. A vector
  draft is a plain `Vec`, a map draft a `PMap`. Only the owner's run
  mutates the draft, in place, under one object-table lock per operation;
  ownership is checked by task identity, so a builder survives yields and
  migration to another worker thread. `freeze` moves the draft out and
  allocates a new persistent object; the builder becomes `Frozen`.
- **Termination.** A task records the builders it creates in a private,
  non-rooting list in its `VmState` (pruned as it grows). Completing or
  failing releases every still-open one (`Released`, draft dropped). A
  released builder fails every later use with `OwnerFinished`; another
  task's use of a live builder fails with `NotOwner`; use after freeze
  fails with `Frozen`.
- **Tracing.** An open builder's `Trace` reports its draft's contents, so
  they are kept alive by reachability of the builder object (normally the
  owner's frame), including across yields; frozen and released builders
  have no edges. The owner's builder list is not a root.
- **Deferred.** Embedder (`Scope`) access to builders, builders seeded from
  an existing persistent value, in-place map trie editing, releasing the
  builders of a task that is dropped unfinished (they stay unusable by any
  other task). Builders are rejected at publication (below).

## Publication rules (clj-quickjs-18)

`src/value/share.rs` (categories, check, cache; full contract in its module
docs), entry point `Runtime::publish`. Design: `design.md` §4.

- **Categories.** Every heap object kind declares one, in the exhaustive
  `share::classify` match (a new kind does not compile until it does):

  | Category | Kinds | Shareable |
  |----------|-------|-----------|
  | `Persistent` | vector, map, occurrence; natives declaring it | when everything it references is |
  | `TaskOwned` | builders in every state, ordinary objects, arrays, binding cells, natives declaring it | never |
  | `ConcurrentReference` | atoms (clj-quickjs-10); reserved for any heap-object handle to a dependency cell (cells are runtime-owned, not heap objects) | always; not descended into |

  Primitives are shareable. Ordinary objects are the mutable JavaScript
  objects of §4 (clj-quickjs-114, "Ordinary objects" below): task-owned
  mutation whatever their properties hold. So are arrays (clj-quickjs-118,
  "Array objects" below), whose description path continues into their
  elements by index. A native payload declares its category through
  `NativePayload::shareability`.
- **The check** walks the value graph breadth-first with an explicit queue
  and a visited set: no recursion (depth is bounded only by memory), each
  object inspected once (cycles through natives or builders terminate;
  shared DAGs cost their distinct objects, not their paths). It reads each
  object under one short object-table lock and walks collection bodies
  with no lock held. The first task-owned object fails the check with
  `PublishError::NotShareable { path, found, owner }`, where `path` is a
  shortest path from the published value (`value[1]{7}[1]`; `{k}.key` for a
  map key, `.value` through an occurrence, `.edge(n)` through a native).
  When that object is an ordinary object, the path continues through its
  property values (`.property(name)`, first object-valued task-owned
  property in creation order, no object twice) to the innermost mutable
  object of that chain: `[{inner: {}}]` fails at `value[0].property(inner)`.
- **Cache.** Each heap slot has a sticky known-shareable bit. A persistent
  object gets it at allocation when all its parts already have it
  (`Heap::alloc_persistent`: O(parts) for an update, never a walk), and
  every object a successful check expanded gets it afterwards. The walk
  stops at known objects, so values built bottom-up from shareable parts,
  republished values, and versions derived from them are accepted in O(1).
  This is valid because persistent edges never change (a function's
  write-once property-record edge is attached only while its bit is clear,
  clj-quickjs-125) and no object's category changes; the bit is rewritten when a slot is reused.
- **Entry point.** `Runtime::publish(&Rooted)` is what the embedder calls
  before handing a value from one task to another. Nothing is copied, so
  the receiving task sees the same identities. `Runtime::spawn` does not
  check its arguments: the embedder decides what each task receives, and
  builders and binding cells stay protected by their owner checks
  regardless. Later runtime handoffs (dependency results, atom contents)
  call `share::check` themselves; closure captures are function-object
  edges, which the walk follows (`.capture(i)` in a path). A
  function's own-property record is one more edge (`.properties`;
  clj-quickjs-125, "Function own properties" below).
- **What later kinds declare.** A category. If `Persistent`: edges that
  never change after allocation (a closure's captured values never do; its
  mutable bindings are separate task-owned cells) and allocation through
  `alloc_persistent`. If
  `ConcurrentReference`: a `share::check` of every value it installs.
- **Deferred.** No VM intrinsic exposes the check (the instruction set is
  unchanged), and `spawn` arguments are not checked.

## Source compilation (clj-quickjs-22)

```
 source ─► parse_script ─► ast::Script ─► lower (resolve + emit) ─► ExecutableSpec
                                                 │ one SourceLocation per instruction
                                                 ▼
                  Executable::load_compiled = the same validation as Executable::load
```

- **Pipeline** (`src/frontend/lower.rs`): one in-order pass over the syntax
  tree (at most 256 nodes high, so recursion is bounded) resolves bindings
  and emits bytes through a private emitter. The output goes through the
  same validator as any externally built `ExecutableSpec` (operands, branch
  targets, stack effects, falling off the end); there is no unvalidated path
  into the VM. `Executable::to_spec` re-encodes loaded code, so tests can
  corrupt compiled bytecode and check that the validator rejects it.
- **Bindings.** Every `let`/`const` binding and parameter is a frame local;
  entering a statement list declares all of its bindings first (sibling
  blocks reuse slots). Redeclaration in one scope is a compile-time
  `SyntaxError` (an early error in JavaScript). Unknown names, use before
  initialization and assignment to a `const` are run-time errors in
  JavaScript, so the compiler emits `throw_error` (opcode `0x2A`, modelled on
  QuickJS's `throw_error`) at that point: the task fails with
  `VmErrorKind::ReferenceError` or `TypeError` only if that code runs.
  Initialization is decided statically by source position within the
  declaring function (no jump of the subset enters a scope past a
  declaration: exceptions, `return`, and since clj-quickjs-78 `break` and
  `continue`, only leave scopes or restart a loop body from its start, and
  the finally blocks they run since clj-quickjs-79 are entered at their
  start; see
  `docs/grammar.md`, "Explicit gaps"); bindings
  that closures capture use the VM's run-time check, binding cells, where
  needed (clj-quickjs-21, below).
- **Operators.** New number-only opcodes (`div`, `rem`, `neg`, `gt`, `le`,
  `ge`) follow the existing `add`/`sub`/`mul`/`lt` rule. The compiler applies
  JavaScript ToNumber explicitly (`to_number`, `to_number_pair`) to operands
  that are not statically numbers, after both operands are evaluated; with no
  string values that is JavaScript's semantics. `loose_eq` is `==` for the
  supported value types; `!=`/`!==` are the equality opcode plus `not`;
  `&&`/`||`/`?:` are branches. The bitwise and shift operators and `~`
  (clj-quickjs-62) are number-only opcodes `bit_and`, `bit_or`, `bit_xor`,
  `shl`, `sar`, `ushr`, `bit_not` (`0x3F`–`0x45`) after the same explicit
  ToNumber; the opcode applies ToInt32 / ToUint32 itself (a pure function
  of a Number), so the order of conversions JavaScript specifies is kept.
  `**` (clj-quickjs-63) is the number-only opcode `pow` (`0x46`) after the
  same ToNumber; it computes Number::exponentiate (IEEE `pow` with NaN for a
  non-finite exponent on base ±1, as QuickJS's `js_pow`).
  `++`/`--` on a binding (clj-quickjs-64) needs no opcode of its own: the
  compiler emits the binding's read, `to_number`, `add`/`sub` of the
  constant 1 and the binding's store (through `cell_set` for a binding in
  a cell, a `throw_error` for a `const`), with a `dup` before the
  arithmetic so a postfix update yields the converted old value.
  Compound assignment `x op= v` on a binding (clj-quickjs-84) likewise:
  the binding's read, then `v`, then exactly the instructions of the
  binary operator `x op v` (`plus` for `+=`, `to_number_pair` and the
  numeric opcode otherwise), then the binding's store. On a property,
  `o.p op= v` / `o[k] op= v` (clj-quickjs-260) puts the same operator
  instructions between the reference read and write of property
  `++`/`--` (`dup` `get_field` .. `put_field`, or `to_property_key`
  `dup2` `get_elem` .. `put_elem`), with `v` evaluated after the read.
  `typeof` (clj-quickjs-69) is the opcode `typeof` (`0x4E`), which reads
  the heap only to tell a function object (`"function"`) from every other
  object kind (`"object"`; the mapping of every value kind is in
  `src/vm/instr.rs`). Its operand is a reference when it is a name: `get_global_or_undefined`
  looks up an otherwise free name at run time (clj-quickjs-104). A binding
  in its temporal dead zone is read as usual, so it still throws.
  `void x` (clj-quickjs-133) needs no opcode of its own: the operand's
  code, then `pop` and `undefined` (QuickJS emits `OP_drop`,
  `OP_undefined`). Its operand is an ordinary expression, so an unbound
  name throws as in a plain read.
  An untagged template literal (clj-quickjs-57) is its first text part as
  a string constant, then per substitution the substitution's code, the
  opcode `to_string` (`0x55`, ToString: ToPrimitive with hint string once
  objects convert, clj-quickjs-70, unlike the default hint of `+`) and
  `plus`, then the next non-empty text part and `plus`.
- **Completion value.** A script's tasks complete with its completion value
  (what `eval` would return), kept in local 0 and reset by `if`/`while` as
  ECMAScript's UpdateEmpty rules require, the same technique as QuickJS's
  `eval_ret_idx`. `compile_function` compiles one function declaration as a
  task entry point (parameters are locals `0..n`, `return` completes the task).
- **Source positions.** Compiled code keeps the source text and one
  `SourceLocation` (byte span, line, column) per instruction, outside the byte
  format; it never affects execution. A failure's `Diagnostic::source` is the
  failing instruction's location, `Task::source_location` is the location of
  the instruction a ready (for example yielded) task will run next, and
  `throw_error` messages quote the binding name from the source. An
  operator or call instruction is located at its token (the operator, the
  call's `(`), a reference at its identifier (clj-quickjs-52; the table is
  in `docs/grammar.md`, "Errors").

## Calls and frames (clj-quickjs-4)

```
 VmState (task-owned)
   entry: Frame ─┐   calls: [Frame, Frame, …]      each Frame = { code: Arc<Code>,
                 │                                   callee, this, construct,
                                                     pc, base }
   stack: [ entry locals | entry operands | f1 locals | f1 operands | f2 … ]
                                           ▲ f1.base     (callee slot reused)
```

- **Function code** (`src/vm/executable.rs`, `src/vm/function.rs`): an
  `Executable` is the code of one function; its child table
  (`ExecutableSpec::functions`) holds already-loaded executables by `Arc`, so
  a function tree is validated bottom-up once and is acyclic. A function
  object (`HeapObject::Function`, appended to the object-kind registry) is a
  heap value holding an `Arc` of that code: `function i` allocates one per
  evaluation (distinct identities, as for function expressions), and
  `current_function` yields the running frame's own (allocated on first use
  for the entry frame, then kept by the frame). Recursion goes through
  function values, never through a code cycle.
- **Call/return** (`src/vm/frames.rs`, `src/vm/dispatch.rs`): `call n`
  resolves the callee (a non-function fails with `TypeError`), checks the
  task's `VmLimits`, then reuses the callee's operand slot as the new frame's
  base: arguments become its parameter locals (missing → `undefined`, extra
  dropped), and the loop continues in the new frame. `return` truncates the
  value stack to the frame's base, pops it, and pushes the result for the
  caller, whose saved `pc` already names the instruction after its `call`.
  The interpreter is a flat loop: the only thing it keeps across
  instructions is an `Arc` of the running frame's immutable code, refreshed
  when the frame changes, and every frame and value is reached by index, so
  `stack` and `calls` may reallocate on any call and a task may yield or be
  collected between any two instructions with any number of live frames.
  QuickJS's `JS_CallInternal` instead recurses on the C stack with
  `alloca`'d `JSStackFrame`s; only its saved-state inventory (`cur_pc`,
  `cur_sp`, argument and variable buffers) and argument binding rules are
  reused as a reference.
- **Resource policy.** `VmLimits` (per runtime, copied into each task) bounds
  live frames and reserved values (locals plus each frame's validated
  maximum operand depth). A `call` that would exceed either fails the task
  with `VmErrorKind::StackOverflow` before anything changes; Rust stack use is
  independent of JavaScript call depth (tested on a 64 KiB worker stack).
- **Tracing.** The task root source traces the whole value stack, each
  frame's `this` (clj-quickjs-116, below) and each frame's function object; a function object's `Trace` reports its code's
  constants and captured values, and the entry frame's constants are the task's executable
  constants. A callee that is no longer on any operand stack stays alive
  through its frame.
- **`this` (clj-quickjs-116).** Each frame has a task-owned `this` slot,
  set when the frame is entered: the receiver of the `call_method` that
  called it, `undefined` for a plain `call`, a resumable native's callback
  and the entry frame (QuickJS keeps `this_obj` as an argument of
  `JS_CallInternal` on the C stack; here it is frame state). `get_this`
  (`0x56`) pushes it, allocating nothing. The slot is traced with the frame,
  so a receiver that only a suspended method frame holds survives
  collections, waits and migration to another worker (`tests/need.rs`).
  Strict code reads the slot as is; sloppy code fails with
  `UnsupportedOperand` for a non-object receiver (the global object is
  clj-quickjs-104, primitive wrappers clj-quickjs-72). The compiler emits
  `get_this` for `this` in a non-arrow function. Script-level `this` uses
  `get_global_object` (clj-quickjs-104); arrows capture either by value (-98).
- **`new` (clj-quickjs-71).** `construct n` (`0x5E`) is `call n` for a
  constructor: the callee must be a function object whose code is an
  ordinary function (`ExecutableSpec::constructor`; arrows, built-ins and
  everything else are a catchable `TypeError` "… is not a constructor").
  It reads `F.prototype` (creating the lazy prototype object), allocates
  an ordinary object owned by the running task that inherits from it, or
  from `Object.prototype` when it is not an object, and enters `F` like a
  call with that object as the frame's `this` and the frame's `construct`
  flag set. Leaving a construct frame applies the return-override rule in
  one place (`VmState::leave`, which `return` and a finally block's
  pending return both reach): an object result is kept, any other result
  is replaced by the frame's `this`. QuickJS (`JS_CallConstructorInternal`,
  `JS_CreateFromConstructor`, revision 7d1c13c5d8f8) creates the object before
  a recursive `JS_CallInternal` and checks the result after it returns on
  the C stack; here the object and the flag are frame state, so a
  constructor suspended at `__rt.need` holds nothing on the Rust stack and
  its new object, reachable only from the traced `this` slot, survives
  collections and migration (`tests/need.rs`). The allocation and the
  frame entry happen in one instruction, so no collection falls between
  them.
- **Context and diagnostics.** Task identity and world are task-wide, so a
  call never changes world. A failure reports the failing frame's executable,
  instruction and source location plus `Diagnostic::call_chain`: each
  caller's `call` site, innermost first.
- **Publication.** A function object has no properties, so it is
  `Persistent` and passes `Runtime::publish` unless it captured something
  task-owned (below); another task calling it runs the same code in its own
  frames.

## Lexical closures (clj-quickjs-20)

Code: `src/vm/function.rs` (contract), `src/vm/dispatch.rs`; tests:
`tests/closures.rs` (early VM tests, hand-assembled bytecode).

- **Environment.** A closure is a function object plus the values it
  captured, fixed at creation: `ExecutableSpec::captures` declares the count,
  `function i` pops that many, `get_capture i` reads one (bounds checked at
  load). Nested closures re-capture with `get_capture`, as QuickJS's
  `js_closure2` copies a parent's `var_refs`
  (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8).
- **Value or cell.** A binding initialized before capture and never assigned
  is captured by value. Any other captured binding lives in a binding cell
  (`HeapObject::Cell`) from its declaration on: the frame's local holds the
  cell and every closure capturing the binding holds the same cell, which is
  how closures share a binding and how it outlives the frame. QuickJS's
  `JSVarRef` instead points into the live frame until `close_var_refs` copies
  the value out when the frame exits; here frames hold no references from
  the heap, so the value stack may reallocate and frames can be yielded,
  resumed elsewhere and collected with nothing to close. Each `new_cell`
  execution is a fresh binding, so per-activation and per-iteration
  bindings come from where the compiler emits it.
- **TDZ.** Cells start uninitialized; `cell_get`/`cell_set` before
  `cell_init` raise `ReferenceError`. This is the run-time check the
  compiler needs once a closure can run before a declaration.
- **Ownership and publication.** Code stays immutable `Arc` storage shared
  by all tasks; closure environments are heap objects under the publication
  rules. Function objects are `Persistent` (code and captures never change),
  cells are `TaskOwned` with an owner `TaskId`. A closure over immutable
  shareable values is publishable (since clj-quickjs-125 it is known
  shareable only once a check proved it, not at allocation); one
  that reaches a cell is rejected at it. Cell instructions from another task
  fail with `VmErrorKind::ForeignBinding`, so an unchecked `spawn` argument
  cannot reach another task's mutable bindings either.
- **Tracing.** A function object reports its captured values, a cell its
  value. A closure stored in a binding it captures is an ordinary cycle:
  kept while a frame or root reaches it, reclaimed by the next collection
  after it becomes unreachable.
- **Deferred.** (Compiling closures from source: clj-quickjs-21, "Functions,
  closures and intrinsics from source" below.) An initialize-once
  immutable cell (a `const` read by a closure before its initialization must
  use a cell, making the closure task-owned); spawning closure code directly
  as a task entry fails at its first `get_capture` (`Internal`).

## Functions, closures and intrinsics from source (clj-quickjs-21)

```
 ast ─► analysis pass (same lowering code, output discarded)
          │ per binding: assigned? captured while initialized / uninitialized?
          ▼
        emit pass ─► one ExecutableSpec per function, loaded bottom-up
                      (children in the parent's `functions` table)
```

- **Two passes, one code path** (`src/frontend/lower.rs`). Whether a
  captured binding can be copied into closures or must live in a cell is
  known only after every closure that uses it has been seen, but the cell
  must be created when the binding's scope is entered. So the lowering runs
  twice over the whole tree: the analysis pass records facts per binding
  (keyed by the declaring identifier's offset), the emit pass chooses each
  binding's representation from them. Both passes resolve every name the
  same way, nested functions are lowered inline (no per-level re-lowering),
  so the cost is linear and recursion depth is bounded by the tree height.
- **Resolution through enclosing functions**, as QuickJS's
  `get_closure_var` does: a name not found in the running function's scopes
  is resolved in the parent; if found there, it becomes a capture of every
  function in between, deduplicated per binding. The parent pushes each
  capture (`get_local`, `get_capture` or `current_function`) before
  `function i` creates the closure.
- **Representation.** A binding no closure captures is a frame local. A
  captured binding that is never assigned and is initialized whenever a
  capturing closure is created is copied (by value). Every other captured
  binding is a binding cell: `new_cell` on scope entry (parameters are moved
  into a cell on function entry), `cell_init` at the declaration,
  `cell_get`/`cell_set` at every use, so the VM performs the temporal-dead-
  zone check for closures that run early. A `let`/`const` declared in a
  `switch` clause before the last non-empty one is a cell too, captured or
  not, because control can enter a later clause past its declaration
  (clj-quickjs-83). Assignment to a cell goes through
  a scratch local (the value is evaluated first, as in JavaScript). Copying
  matters for publication: only closures without cells are shareable.
- **Self-reference.** A function's own name inside it lowers to
  `current_function` when that is equivalent: always for a named function
  expression (its name is immutable) and the `compile_function` entry, and
  for a declaration whose binding is never assigned. So a self-recursive
  function captures nothing.
- **Hoisting.** Entering a statement list declares its `let`/`const`/
  function bindings, creates its cells, then creates its function
  declarations' closures in source order; only then do its statements run.
  A declaration that uses a binding not yet initialized at that point (a
  later sibling function, or a `let`/`const` of the list) captures it as a
  cell. Within the declaring function, the static TDZ decision still holds,
  except for the `switch` bindings above, which the cell checks at run
  time.
- **`var` and var-scoped declarations** (clj-quickjs-94). Entering a
  script or function body also declares, after its parameters and
  lexical and function declarations, the names of every `var` statement in
  the body (nested statements included, nested functions excluded). A
  `var` or function declaration that repeats a parameter, a `var` or a
  function declaration of that body reuses its binding (QuickJS
  `define_var` / `instantiate_hoisted_definitions`); a function declaration
  doing so marks the binding assigned, so closures and the declarations'
  self-references go through a cell, and the last declaration wins. A `var`
  binding is initialized (`undefined`) at entry: a cell one gets
  `new_cell; cell_init undefined`, a plain local starts `undefined` in a
  new frame. A `var x = e` statement is an assignment `x = e` minus its
  value; `var x;` emits nothing. The `var` statement checks the scopes
  between it and the body for a lexical binding of its name (an early
  error), skipping a catch parameter (Annex B). Script-level `var`s are
  global object properties (clj-quickjs-104), rather than frame locals. A
  `var` head of a `for` or `for`-`in` statement (clj-quickjs-89)
  contributes its names to the same body-wide set and lowers like a `var`
  statement (`var_declarators`; a for-in head is assigned each key) inside
  the loop's head scope, so it is never part of the per-iteration copy of
  `let` heads.
- **Reserved intrinsic binding** (`src/frontend/intrinsics.rs`, the
  contract and table; `docs/grammar.md`, "Reserved intrinsic binding").
  `__rt` is a binding in the scope outside the outermost function. It cannot
  be declared anywhere, so every `__rt` resolves to it; the resolver, not the
  callee's spelling, decides that `__rt.vector(…)` is an intrinsic. It can
  only be used as the object of a direct call (`__rt.name(args)`); bare
  references, uncalled members, assignment and computed access are compile
  errors, so an intrinsic never becomes a value. Each call lowers to the
  arguments plus one existing instruction (collection intrinsics `0x12`–
  `0x20`, `host_effect`); no opcode was added. `__rt.need` lowers to the
  `need` instruction (clj-quickjs-23, below).
- **Demo.** `cargo run` compiles and runs `examples/demo.js` (any script
  path can be passed instead); `cargo run --example resumable_counter`
  compiles `examples/resumable_counter.js` with `compile_function`. Both are
  JavaScript source executed by this VM; no C engine is involved.
- **Deferred.** Method calls and function properties (so no `f.call`;
  string values run since clj-quickjs-65, object literals and named
  properties since clj-quickjs-114, `this` since clj-quickjs-116, `new` since clj-quickjs-71); `arguments`, rest
  parameters (defaults: clj-quickjs-100) (var-scoped redeclaration by function declarations runs since
  clj-quickjs-94); an initialize-once immutable cell, which would let a
  hoisted declaration capture a `const` of its own scope by value.

## Dependency waits in the VM (clj-quickjs-23)

Dependency waits reuse the same mechanism as `Yielded`: the task returns from
the run, and all state needed to continue stays in the task. The full
contract is the `src/task.rs` module docs, "Dependency waits in the VM".

- **Lowering.** `__rt.need(definition, kind)` evaluates both arguments and
  runs the `need` instruction (`0x36`, operands `def kind → v`). The world
  is the task's (execution context), never an argument. Literal arguments
  outside the identity ranges are compile errors; other bad operands fail
  with `UnsupportedOperand` at run time.
- **Dispatch.** The loop decodes the operands and returns `Exit::Need` to
  the run guard with the position already past the instruction and the
  remaining budget; the guard runs the same read-or-subscribe as
  `RunGuard::need` (record the edge, ready reads included).
- **Ready.** The shared value (same identity for every reader) is pushed
  and execution continues in the same run and budget.
- **Failed / cancelled cell.** A catchable error is raised at the `need`
  (`VmErrorKind::DependencyFailed(key)` / `DependencyCancelled(key)`), with
  its source position and call chain (clj-quickjs-27, below); uncaught, the
  run fails with it.
- **Pending.** The guard stores the pending result destination
  (`PendingNeed`: key and instruction) in the VM state, keeps any producer
  request the need claimed for the embedder (`Task::take_producer_requests`),
  and ends the run through `RunGuard::park`: `Waiting(key)`, or `Yielded`
  if the wake already arrived, or `Cancelled`. Frames, operands, locals,
  closures and builders stay in the task; no JavaScript callback, promise
  or Rust stack represents the continuation.
- **Resume.** When the result is published, the completion makes the
  parked task runnable (outside any cell or waiter lock); the host runs it
  again on any thread. The run first completes the suspended `need` (reads
  the key again and injects the value or reports the failure at that
  instruction), then continues after it: nothing before the `need` is
  replayed and no function is restarted. A spurious resume suspends again.
- **Tests.** `tests/need.rs` (a manually driven host: pre-need effect once,
  live operands and locals across the wait, collection while parked, resume
  on another thread, a VM callable published by a producer task and
  invoked after the wait, failures, cell and consumer cancellation, worlds,
  operand validation) and `src/task/need_tests.rs` (hook-forced completion
  and cancellation windows).

The reference executor is clj-quickjs-8.

## Dependency cells (clj-quickjs-6)

`src/dep.rs` (protocol, lock and ordering contract in its module docs),
`src/ids.rs` (identities). Design: `design.md` §§7–9.

- **Identities.** `DependencyKey` = `WorldId` (runtime-allocated:
  `Runtime::world`, `Runtime::new_world`; a task's world is fixed at
  `spawn_in`) + `DependencyId` (host-assigned definition/query) +
  `ResultKind` (host-assigned: discovery, callable, initializer, analysis,
  ...). `ProducerId` is runtime-allocated per claim. The consumer identity
  is the `TaskId`; its wake handle adds the subscribing run (clj-quickjs-7).
- **Cells.** One per key in the runtime's dependency table (a private
  `HashMap` over those numeric ids, created on first need or claim, never
  removed or reset in this slice). Each cell holds its claimed producer and
  either `Pending { waiters }` or one immutable terminal outcome:
  `Ready(value)`, `Failed(error)`, `Cancelled`.
- **Need** (`RunGuard::need(definition, kind)`, the native protocol behind
  `__rt.need`; the task supplies its world). In one cell critical section
  it returns the terminal outcome, or registers the task's id (once) and,
  if nobody has, claims the producer, returning `Need::Waiting { key,
  produce: Some(ProducerRequest) }`: scheduler work, not a host call. The
  key is recorded in the task's `DependencyLog` as an edge either way
  (ready reads included), and as its unresolved wait while pending; a task
  with an unresolved wait on another key is refused (`AlreadyWaiting`).
- **Completion** (`Runtime::complete(key, producer, Completion)`): only the
  claimed producer may complete (`NotProducer` otherwise, which is what
  rejects a world-2 result for a world-1 key); exactly once
  (`AlreadyComplete`); values and errors must pass `share::check`
  (`NotShareable`, cell left pending with its waiters). It returns the
  drained wakes (`Wakeups`, clj-quickjs-7) for the caller to enqueue and
  runs nothing inline.
  `Runtime::claim` lets a host produce before anyone needs;
  `Runtime::cell_status` is a snapshot.
- **No check-then-register window.** Check and registration are one
  critical section of the cell lock, the same lock completion takes, so a
  completion is ordered entirely before or after a need.
- **Locks.** Dependency table → heap `sources` (registering a new cell);
  cell lock and task log lock are leaves; none is held while rooting,
  checking publication, delivering wakes, or returning. The table is an
  `RwLock`; the collector reads outcomes lock-free (clj-quickjs-7).
  Orderings: `src/dep.rs` module docs.
- **Collection.** Each cell is a `RootSource` (category
  `DependencyOutcome`): a published value or error survives collection
  while the cell exists, after the producer drops its handle. Readers get a
  fresh `Rooted` of the same identity.
- **Deferred.** Deadlock reporting over
  unresolved waits; cell invalidation and removal. Cancellation: below
  (clj-quickjs-24).

## Parking and wakeups (clj-quickjs-7)

`src/task.rs` (state machine, linearization points and the full transition
table in its module docs), `src/task/state.rs` (the atomic word),
`src/dep.rs` (fast path, completion and wake delivery, lock and ordering
contract). Design: `design.md` §§3, 8, 10, 15.

- **Task state machine.** One `AtomicU64` per task: phase `Runnable |
  Executing | Parked | Terminal`, a `NOTIFIED` bit, and an execution number
  (runs claimed so far). Claim = CAS `Runnable(n) → Executing(n+1)`; the run
  ends by a CAS to `Runnable` (yield, guard dropped) or `Terminal`
  (complete, fail), or by `RunGuard::park`. `Task::status` reports
  `Ready | Running | Waiting | Finished` from the word.
- **Wake handles.** `WakeHandle` = process-unique runtime id + task id +
  the execution that subscribed.
  A cell stores one per task (a later run's re-subscription replaces the
  stale one) plus a `Weak` reference to the task, never a callback.
- **Park vs. wake.** `RunGuard::park` CASes `Executing(m) → Parked(m)` and
  the run returns `Waiting(key)`; if a wake for `m` already set `NOTIFIED`
  the task goes `Runnable` and the run returns `Yielded` (the runner
  requeues it). A wake for `m` CASes `Parked(m) → Runnable(m)` (returned as
  `Wake::Runnable(task)` to enqueue) or sets `NOTIFIED` on `Executing(m)`
  (`Wake::Remembered`); anything else is `Wake::Stale`. Both sides retry on
  CAS failure, so whichever is second sees the first: exactly one of them
  makes the task runnable, once per suspension, and the claim CAS gives it
  one owner. Duplicate, late or foreign handles are `Stale`.
- **Completion.** Recheck, set the outcome and drain the waiters under the
  cell lock; release it; deliver each wake (one CAS, nothing inline); return
  `Wakeups` (`deliveries()`, `task_ids()`, `into_runnable()`). Failure and
  cancellation wake exactly like success.
- **Atomic ready fast path.** The outcome is a `OnceLock` set under the
  cell lock: a need reads it with no lock and only takes the cell lock
  (rechecking) when it is unset. The collector reads it lock-free too.
- **Handoff windows** (`src/task/handoff_tests.rs`, forced with one-shot
  test hooks at `HookPoint`s, never sleeps): completion before
  registration (fast path), between the fast-path `Pending` read and the
  subscription (recheck), after subscription before park (remembered;
  also inside park between its read and CAS), after park (runnable; also
  a park inside the wake between its read and CAS). `tests/wakeup.rs`
  races many waiters, duplicate wakers and competing claimers through
  barriers.
- **Deferred.** A scheduler/executor that owns a ready queue
  (clj-quickjs-8). The VM `need` instruction parks through the same path
  (clj-quickjs-23, above).

## Simulated compiler host (clj-quickjs-9)

`src/host/sim.rs` (contract in its module docs). Design: `design.md` §§9,
12. A host-side registry over the runtime's public API, not a compiler: no
parser, analyzer, namespace or module linking.

- **Worlds.** A `WorldSpec` (definitions by `DependencyId`, each with a name
  and one `Producer` per `ResultKind`) is frozen by `SimHost::add_world`
  under a fresh `WorldId`. A new version is a new world built from an
  edited copy; old worlds and their completed cells are never changed or
  reset (no invalidation).
- **Result kinds.** `BINDING` (discovery; answered `true` by default),
  `CALLABLE`, `INITIALIZED`, `ANALYSIS`: one cell each, no coarse "ready"
  flag. A callable's producer needs only the bindings it refers to, so
  mutually recursive functions are callable before (and without) any
  initializer, and the recorded reference cycle is not a wait.
- **Producers.** Rust work (run once, after all declared needs are ready:
  never restarted, so never replayed), a published VM callable compiled by
  this runtime, or an initializer that invokes a callable through the VM.
- **Scheduling.** Each job (producer, top-level need, call) has its own
  task: its identity, wake handle and `DependencyLog`. Jobs need through
  `RunGuard::need`; a pending cell parks the job in the host (the run ends
  without `RunGuard::park`, so the task stays runnable; parked before its
  producer request is scheduled, so an immediately settled request still
  finds it), and the one `ProducerRequest` per cell schedules the one
  producer job. Completions return `Wakeups`, whose `task_ids()` move parked
  jobs back to a FIFO ready queue; nothing runs inline. The driver
  (`run_until_idle`) is single-threaded and deterministic.
- **Failures.** Unknown world, missing definition and missing result kind
  are decided from the frozen world when the request is scheduled and fail
  the cell at once (never a timeout). Producer errors, unshareable results
  and failed or cancelled inputs settle the cell as `Failed`; the published
  error value is a numeric code, the structured `SimError` stays with the
  host. A demand that would close a wait cycle fails (clj-quickjs-26,
  below). A top-level request left waiting on a cell no host job produces
  returns `Stalled` rather than hanging.
- **Calls (stand-in).** Until the VM has call instructions and function
  values (clj-quickjs-4, -20, -21, -23) a callable is a host token (a fresh
  persistent vector, checked by identity) and each activation is a new VM
  task with one argument. A callable may declare a tail callee: a returned
  number is a tail call of it (the host needs the callee's `CALLABLE`,
  recording the edge), any other value is the result. Branching, base case
  and arguments are computed by the JavaScript; activations per invocation
  are bounded.
- **Deferred.** VM-level `__rt.need` and real calls/closures; worker
  threads (clj-quickjs-8); cancelling a stalled request (its job stays parked;
  the host does not call `Task::cancel` of clj-quickjs-24 yet); invalidation.

## Consumer and cell cancellation (clj-quickjs-24)

`src/task.rs` ("Consumer cancellation" in its module docs: observation
points, terminal cleanup, transition rows), `src/task/state.rs` (the
`CANCELLED` bit), `src/dep.rs` ("Cancellation": who may cancel a cell).
Design: `design.md` §§8, 10, 15.

- **Two kinds, kept apart.** Consumer cancellation (`Task::cancel`) ends one
  task; cell cancellation is the terminal outcome `Cancelled` of a cell.
  Cancelling a consumer never changes a cell, its producer claim or its
  other waiters; a needing task that reads a cancelled cell
  (`Need::Cancelled`) is not itself cancelled.
- **State word.** Bit 3 is a sticky `CANCELLED` (execution number moved to
  bits 4..). `cancel` CASes it in: `Parked(m) → Runnable(m)+CANCELLED`
  (`Cancel::Runnable`: the canceller enqueues), or on `Runnable`/`Executing`
  (`Cancel::Requested`: the current or next run observes it). Wakes treat a
  `CANCELLED` word as stale; parks report it; claims and plain releases
  keep it (release became a CAS loop so a concurrent request is never
  lost). A cancellation and a wake of one parked execution CAS from the
  same word, so exactly one enqueues.
- **Observation and cleanup.** Only the task's own run observes the
  request (run start, `need`, `park`, budget boundary), so it can run the
  task's finally blocks first (clj-quickjs-27, below); the
  canceller never runs anything. Terminal cleanup, under the execution
  claim: withdraw the waiter entry from the cell (log lock released before
  the cell lock), clear the unresolved wait, release builders and VM state
  (their roots become reclaimable), end `Terminal+CANCELLED`. The run
  returns `RunOutcome::Cancelled`; status `TaskStatus::Cancelled`.
- **Who cancels a cell.** Its claimed producer (`Completion::Cancelled`) or
  the embedder without a producer id (`Runtime::cancel_cell`, creating the
  cell if absent); never a task. Same install path as any completion:
  recheck under the cell lock, drain, deliver wakes with no lock held, so
  waiters are woken exactly once and a racing producer completion or
  second cancellation gets `AlreadyComplete`.
- **Tests.** `src/task/cancel_tests.rs` forces every window with hooks
  (including the new `CancelBeforeCas`); `tests/cancellation.rs` races
  parking consumers, settlement and a canceller with barriers; its
  consumers run the synthetic source fixture
  `tests/fixtures/js/cancel_consumer.js`.
- **Deferred.** Polling between VM instructions. The language-level policy
  (finally blocks run, not catchable) is clj-quickjs-27, below; inside
  native frames, clj-quickjs-28.

## Dependency graph and wait cycles (clj-quickjs-26)

`src/dep/graph.rs` (contract in its module docs), used by `src/host/sim.rs`
("Wait cycles"). Design: `design.md` §9.

- **Two relationships.** Recorded uses (task used key; every need, ready
  reads included) are history and are never removed. Active waits (task
  is suspended on a pending key; at most one per task) are removed when
  the key resolves (`Ready`, `Failed` or `Cancelled`), when the waiter is
  woken or cancelled, or when the wait is refused. The per-task
  `DependencyLog` keeps the same split inside the runtime.
- **Cycles.** A producer binding links a pending key to the task running
  its claimed producer. A new wait `task → key` follows the single chain
  key → producer task → its waited key → ... and closes a cycle only if it
  returns to `task` (linear in the chain). Recursive references (callables
  needing each other's bindings) create uses, not waits, so they are never
  reported; keys without a known producer end the chain.
- **Policy: the newest demand fails.** The closing wait is not added; the
  `WaitCycle` (tasks, demand sites, world/definition/kind, producers) is
  returned. `SimHost` abandons the subscription (`RunGuard::abandon_wait`:
  withdraws the waiter entry and the log's unresolved wait, keeps the
  recorded edge), records `SimEvent::WaitCycle` and fails the job with
  `SimError::WaitCycle`. Its cell fails, waking the previous job on the
  cycle, which fails with `InputFailed`, and so on: every job and request
  on the cycle settles. One report per cycle.
- **Ordering.** Waits are recorded after the need returned `Waiting`; a
  producer resolves its key before completing it (a completing producer is
  not blocked, so no reported cycle runs through a completed key), and a
  resolved key never gets an edge again. The graph is plain data: the
  single-thread `SimHost` owns one; a worker-thread scheduler would hold
  it under one leaf lock.
- **Deferred.** Demand sites carrying VM source locations (the `__rt.need`
  instruction, clj-quickjs-23); cycle detection in `exec::Executor`,
  which has no producer bindings.

## Atoms (clj-quickjs-10)

`src/atom.rs` (contract in its module docs). Design: `design.md` §§4–6, 11.

- **Three writer contracts.** Multi-writer compare-and-set on a shared atom
  (`Runtime::atom_compare_and_set`, VM `atom_cas`); single-writer
  publication through the one non-`Clone` `Publisher` of a single-writer
  atom (CAS on it is rejected, so no CAS races the writer); and dependency
  completion (`crate::dep`), which stays single-assignment. Atoms have no
  producer, waiters, wakes or watches.
- **Handle.** An atom is a heap object (`HeapObject::Atom`, category
  `ConcurrentReference`); its handle is its generation-checked `ObjectId`,
  so a stale handle never names a later object in the same slot. Every
  value installed (creation, CAS, publication) passes `share::check` first.
- **Comparison.** SameValue: objects by identity, `NaN` matches `NaN`,
  `+0` differs from `-0`, other primitives by value. No structural or user
  equality. ABA on values is allowed (`A → B → A` matches `A`), as in
  Clojure; ABA through storage reuse is impossible (`ObjectId`s are never
  reissued; `expected` is rooted).
- **Storage and ordering.** The heap object holds an `Arc<AtomCell>`
  (independently owned storage, like collection bodies); the cell holds an
  `AtomicPtr` to an immutable `Box<Value>`. Writers install a filled box
  with `compare_exchange(AcqRel, Acquire)` or `swap(AcqRel)`; readers load
  with `Acquire`. The release/acquire pair also orders the writer's earlier
  construction of the value's graph, so readers never see a partially
  initialized node. Each atom synchronizes alone; no `SeqCst` order across
  atoms, no volatile access, and the value itself is never read or compared
  under a lock. Each operation does take two short heap metadata locks
  before touching the cell: the `gc` lock to register as a mutator (see
  below) and the object-table lock to clone the cell's `Arc`.
- **Reclamation.** Atom operations are registered heap mutators
  (`Heap::enter_mutator`, nested inside a run's registration in the VM), and
  no collection runs while an operation is in progress (it has no
  safepoint, so its thread is never stopped; see the safepoints section). A
  replaced box is
  retired with the writer's collection epoch and freed by a later write
  that observes a newer epoch, or when the cell drops. This is the only
  `unsafe` code in the crate; each block states its invariant.
- **Load-to-root.** A load roots its result inside its own mutator
  registration (`Rooted`, scope `Local`, or the task's operand stack), so
  no collection can separate reading the pointer from rooting the value.
  The value then lives until its holder releases it, regardless of later
  replacements and collections. The collector traces only the current box.
- **Failure contract.** A failed CAS installs nothing and reports the
  current value (rooted); the runtime has no `swap`-with-callback and never
  re-runs user code, macros or effects to hide a conflict.

## Reference executor (clj-quickjs-8)

`src/exec.rs` (module docs: model, events, locks, shutdown). Design:
`design.md` §§3–4, 8, 10, 13, 15.

- **Optional.** Example/host code built only on the public run API
  (`try_begin_run`, `run_with`, `need`/`park`, `cancel`, `Wakeups`). The
  runtime owns no threads and never starts it; an embedder's own pool
  drives tasks the same way (`tests/executor.rs` has a manual embedding
  next to the executor tests).
- **Workers and ownership.** N OS threads share one FIFO ready queue whose
  entries carry a placement (any worker, or one). A worker claims the task
  (`try_begin_run`, the task's atomic state word) before handing the run
  guard to the driver, and the claim ends with the run: yield (requeue),
  park (the task waits in the executor's parked set, on no worker), or a
  terminal outcome (stored for `take_outcome`). Workers are temporary
  executors; the VM state stays in the task, so a resumed run on another
  worker continues at the saved instruction and replays nothing.
  Task-owned builders follow the task.
- **No global lock.** Two short leaf mutexes (scheduling state; event
  record, innermost) are never held while a task runs or suspends, nor
  while the driver, a host, or the placement hook runs. A rendezvous inside
  two tasks' host effects proves two workers execute at once.
- **Deterministic tests.** The placement hook sees each enqueue's reason
  and the task's last worker (for example: woken tasks go to another
  worker); the event record (dispatch, release, enqueue, worker start with
  OS thread id) is what tests wait on, with bounded waits, never sleeps.
  Predicates run on a snapshot with no executor lock held. A task is never
  dispatched again before its previous dispatch has recorded its release
  (an in-flight set), so the record is a faithful order of ownership.
- **Cancellation and shutdown.** `Executor::cancel` enqueues a parked task
  its cancellation made runnable; the task's own run cleans up. Shutdown
  stops dispatch, waits a bounded time for in-flight runs (bounded by the
  budget), and returns queued and parked tasks to the embedder;
  `ShutdownTimeout` names busy workers and returns the executor for a
  retry.
- **Contrast.** `research/quickjs/quickjs-libc.c` `worker_func` gives each
  worker its own `JSRuntime` and `js_worker_postMessage` serializes
  messages between them; here all workers share one runtime and heap and
  pass tasks, not copies of values.
- **One runtime.** Task ids restart in every runtime, so an executor is
  started for one runtime and hands back tasks of others (`ForeignTasks`).
- **Deferred.** The VM-level need (clj-quickjs-23) that will let the
  default driver park on its own.

## Collection under the executor (clj-quickjs-25)

`src/exec.rs` (module docs, "Native jobs" and "Collection") on top of the
safepoint contract in `src/heap/gc.rs`. Design: `design.md` §11, §13
item 7, §15 ("collection with suspended frames and native references").

- **Registered workers, not a held lock.** Each worker holds a
  `ThreadRegistration` for its lifetime: the collector's registry knows
  it, and every collection report says how it was stopped. Registration is
  not heap access, so a worker waiting on the ready queue, or routing an
  outcome, is idle and stopped; it enters heap access only through a run
  guard (VM safepoints) or a native job's scope (no-GC access, delays a
  request until closed or released). A worker that takes a task while a
  request is pending waits in `try_begin_run` with no executor lock held.
- **Roots without an executor root source.** A task handle is itself a
  root source, so the executor's queue, parked set and in-flight locals
  root their tasks' saved frames simply by holding handles; a running
  task's frames come from its run's safepoint report; completed cells root
  their results; terminal outcomes are `Rooted`. The collector never takes
  an executor lock, and no worker holds one while entering heap access or
  requesting, so the rendezvous cannot wait on scheduling state.
- **Native jobs and worker-requested collections.**
  `Executor::submit_native` runs host code on a worker outside any run;
  `Executor::schedule_collection` is such a job requesting a collection.
- **Events.** `Runtime::observe_collections` delivers a
  `CollectionReport` (requester thread, participants with their
  `MutatorState`, stats) on the collecting thread after the rendezvous,
  with no heap lock held; the executor records it as `Collected`, naming
  workers. Events of resumed runs may precede it.
- **Contrast.** `research/quickjs/doc/quickjs.texi` ("Runtime and
  contexts", revision 7d1c13c5d8f8) forbids using one runtime from several
  threads; here several workers share one heap and the registry plus
  rendezvous decide when a collection may run.
- **Deferred.** Automatic collection triggers, worker-local allocation
  buffers, and releasing a run's access around a long leaf host effect.

## Exceptions and completion records (clj-quickjs-11)

Exception control flow is task state, not Rust control flow
(`docs/design.md` §10). The contract is the `src/vm/exception.rs` module
docs.

- **Handler stack.** `VmState` keeps one stack of entries beside its
  frames: catch handlers, finally handlers, and the *pending completion*
  of each running finally block (`Normal`, `Return(value)`,
  `Throw(exception)`, or `Jump(resume)` since clj-quickjs-79). Each entry names its frame by index and the
  value-stack height to restore. Leaving a frame drops its entries.
- **Raising.** `throw`, a re-raising `end_finally`, or a catchable failing
  instruction pops entries until a handler: frames above its frame are
  popped and the operand stack is cut to its height; a catch handler gets
  the value pushed, a finally handler becomes a pending `Throw` and its
  block runs. Pending completions passed on the way are discarded, which
  is how an abrupt finally completion overrides the earlier one. The loop
  continues in the same run and budget; a caught exception is a poll
  point. No handler left: the run fails with the exception's error.
- **Return.** `return` looks for a finally handler of its own frame: if
  there is one, it becomes a pending `Return` and its block runs;
  `end_finally` resumes the return (through the next finally handler, then
  out of the frame). The compiler emits no per-return cleanup code.
- **`break` and `continue`** (clj-quickjs-79). The compiler emits one
  instruction per `try` construct a jump leaves, innermost first:
  `pop_catch`, `call_finally` (opcode `0x47`: the finally handler becomes a
  pending `Jump` to the next instruction and its block runs; its
  `end_finally` comes back) or `drop_completion` (`0x48`: a jump out of a
  finally block discards its pending completion), then the `jump` to the
  loop. This is QuickJS's `emit_break` with `gosub`, the return address
  kept in the task-owned handler stack instead of on the C value stack, so
  the jump survives yields, waits, migration and collections inside the
  finally block. Both instructions check the handler stack and operand
  height at run time (`Internal` on a mismatch, hand-assembled code only);
  the resume point is the validated fall-through edge. A `drop_completion`
  of a pending cancellation ends the cleanup, like a `return`.
- **Catchability.** ReferenceError, TypeError, RangeError, SyntaxError and
  StackOverflow failures are raised as ordinary task-owned Error instances
  (clj-quickjs-230); other failures are uncatchable and end the run without
  catch or finally blocks, as QuickJS's uncatchable errors do. Failed or
  cancelled dependencies are catchable too (clj-quickjs-27), with the
  published cause traced by the engine slot independently of properties.
- **Diagnostics.** Each raised exception carries the `VmError` to report if
  uncaught, built where it was raised (position and call chain), and kept in
  a pending `Throw` across finally blocks. Rethrowing an error object
  reports its original error; an uncaught plain value reports
  `VmErrorKind::Thrown` at its last `throw`.
- **Suspension.** Yields and waits leave the handler stack untouched, so a
  finally block never runs because a task suspends; pending return values
  and exceptions are traced with the frames. Waits inside handlers across
  migration and collection are tested in `tests/exception_waits.rs`
  (clj-quickjs-27).
- **Tests.** `tests/exceptions.rs` (source: same-function and outer-function
  catch, return through finally, overriding, rethrow, scoping, completion
  values, catchable and uncatchable failures, diagnostics, one-instruction
  yields with a collection at every yield inside try, catch and finally),
  and early VM tests in `src/vm/dispatch.rs` (stack checking of handler
  targets, mismatched handler instructions).

## End-to-end scenario (clj-quickjs-13)

`examples/e2e/scenario.rs` (module docs list the steps), shared by
`cargo run --example e2e` and `tests/e2e.rs`. Design: `design.md` §13.

- **Composition, not substitution.** Only public API: `Runtime`,
  `exec::Executor` (two workers, placement hook, `schedule_collection`),
  VM `__rt.need`, `Runtime::complete`, `frontend::compile_function`. The
  consumers' control flow, the helper and the useful work are JavaScript
  executed by the VM; Rust supplies the input, leaf effects (record, and a
  bounded rendezvous), and scheduling decisions.
- **Driver glue.** The VM driver plus what clj-quickjs-23 requires: after
  every run it takes the run's producer requests
  (`Task::take_producer_requests`, found through a task-id registry since a
  `RunGuard` exposes only the id) and records the suspension with
  `Task::source_location`. The host holds the one request until it decides
  to run the producer task, then publishes its result and passes the
  `Wakeups` to `Executor::wake`.
- **Determinism.** Consumers are pinned to workers 0 and 1; a placement
  hook sends woken tasks to the other worker; the host proceeds only on
  recorded events (`wait_for`), terminal outcomes and the rendezvous.
  Orders that are genuinely racy (which consumer claims the request, which
  resumed consumer finishes first) are not asserted.
- **Variants (clj-quickjs-29).** `run_variant(Variant)` shares one `Stage`
  (runtime, executor, input, driver, hosts) and the same fixtures and
  steps; each variant changes one decision of the host. `failure` gives
  the producer task a slot `producer.js` throws on; the driver traces the
  failed run and the host publishes `Completion::Failed` with a value it
  builds from the failure's source position. `cancel` cancels the request's
  claimant with `Executor::cancel` right after both consumers park and
  waits for its `Released(Cancelled)`; the placement hook sends a
  cancellation's cleanup run to the other worker, like a wake. The driver
  traces `Cancelled` and `Failed` runs; the host traces its cancellation
  request (before calling `cancel`, since the cleanup run may start at
  once) and whether a publication was the helper or a failure. The
  consumer fixture's `need` sits in `try`/`catch`/`finally`, so every
  variant shows the pending finally block surviving the wait.
- **Companion (clj-quickjs-29).** `run_companion()` runs one
  `companion.js` task on the same stage: its need is inside
  `__rt.reduce`'s JavaScript callback and two `try`/`finally` blocks, so
  it parks inside a native frame, is collected, migrates and completes the
  fold.
- **Deferred.** `host::sim` integration and executor-side cycle
  detection; an uncaught dependency failure and cancellation inside the
  native fold are covered by `tests/native_unwind.rs`, not by the example.

## Resumable native operations (clj-quickjs-12)

Native operations that call JavaScript or demand dependencies cannot be
ordinary Rust calls: a callback may suspend the whole task, and no Rust frame
may survive the run. The contract is the `src/vm/native.rs` module docs.

- **Two kinds.** Leaf natives (collection, builder and atom intrinsics, leaf
  host effects) run inside one instruction, as before. Resumable natives
  (`reduce` `0x3D`, `need_all` `0x3E`, behind `__rt.reduce` and
  `__rt.needAll`) only pop their operands into a `NativeFrame` pushed on
  the task's `VmState` (`vm::native::is_resumable` is the exhaustive split).
- **Trampoline.** While a native frame is running (no call frame above the
  frame that started it), the dispatch loop takes its next step instead of
  an instruction. A step consumes the completion of the previous action
  (start, a callback's result, or a dependency's value) and returns the next:
  `Call` (callee and arguments pushed in the native's operand area, then a
  VM frame entered exactly as `Call` does; its `Return` delivers the result
  back there), `Need` (`Exit::Need` to the run guard, the same protocol as
  the `need` instruction; the value is pushed there on resume), or `Return`
  (the frame is popped and the value pushed for the caller).
- **State.** A native frame holds the arguments, progress index and length,
  intermediate value (accumulator, results so far), what it awaits, its
  result destination (its operand-area slot) and the starting instruction
  (diagnostics, suspended source location). It is traced with the task's
  frames (`RootCategory::TaskFrame`) whether the task is suspended, yielded
  or stopped at a VM safepoint, and released when the native returns or the
  task finishes or is cancelled.
- **No borrows across steps.** A step reads one element through
  `collections` (taking and releasing the heap lock inside the call) and
  stores anything it allocates in the frame before returning; nothing is
  held across a callback, a stack growth, a collection or a suspension.
- **Budget and polls.** Each step is one budget unit and a poll point.
- **Tests.** `tests/native_fold.rs` (a source callback capturing a live local
  waits for a VM helper inside the fold and resumes on another thread;
  one suspension per element plus one-unit budgets without replay;
  collection while values are held only by a suspended, yielded or running
  native frame and reclamation after; cancellation inside a fold; `needAll`
  suspending in the native; failure positions; nested folds) and
  `src/vm/native.rs` (leaf versus resumable step accounting).
- **Exceptions.** Handlers inside a callback catch as usual. A handler of
  the frame that started a pending native (or of a caller) is reached by
  unwinding through the native (clj-quickjs-28, below). A failing starting
  instruction raises before its native frame exists.

## Exceptions across waits and cancellation cleanup (clj-quickjs-27)

Contract: `src/vm/exception.rs` ("Dependency failures", "Waiting is not
unwinding") and `src/task.rs` ("Cancellation and finally blocks"). Design:
`design.md` §§7, 10.

- **Waits are suspensions.** A pending `need` saves the destination and
  parks; the handler stack is not touched, so no catch or finally block runs
  because a task waits, in any block or callee. Pending completions and their
  values are traced with the frames across waits, collections and migration.
- **Dependency failures are language errors.** `RunGuard::vm_need` turns a
  `Failed(e)`/`Cancelled` cell into `VmNeed::Raise`; the run guard calls
  `dispatch::raise_failure`, the same path a catchable failing instruction
  takes: if some handler could see it, a fresh task-owned Error instance
  with an immutable internal diagnostic/cause slot (clj-quickjs-230)
  is raised at the need; otherwise the run fails with the error (key, world,
  source position, call chain). `Rooted::error`/`error_cause` expose it to
  the embedder.
- **Consumer cancellation is a forced completion.** On observation the run
  (`RunGuard::begin_cleanup`) withdraws the waiter entry, drops the pending
  need, sets `VmState::cancelling` and calls `VmState::unwind_cancel`: catch
  handlers and pending completions are discarded up to the innermost finally
  handler, which becomes `Completion::Cancel`. `end_finally` of a `Cancel`
  unwinds to the next one; with none left the loop exits
  `Exit::CleanupEnded` and terminal cleanup ends the run `Cancelled`.
  `raise` and `return_value` refuse to discard a `Cancel` entry (the cleanup
  ends instead), and a `need` is refused (the task is cancelled), so cleanup
  can neither revive the task nor wait. A cleanup yields at budget
  boundaries like any code; the next run continues it. A cancellation that
  lands in the park window, after the body was released, yields and the
  next run starts the cleanup.
- **Tests.** `tests/exception_waits.rs` (source snippets: waits in try,
  catch, finally and callees; pending return and rethrow after a wait;
  rooting of a pending return value; caught and uncaught failures; cell
  cancellation; cancellation policy cases; budgeted cleanup; the two-worker
  executor migrating and collecting around a parked task and cancelling
  one) and `src/task/need_tests.rs` (hook-forced cancellation inside the
  need and the park windows).

## Concurrency regression suite (clj-quickjs-14)

`tests/concurrency/` (guarantees in `main.rs`, harness contract in
`harness.rs`). Design: `design.md` §15. Test code only: it composes the
production task state word, cells, executor, VM `need`, collector, atoms and
resumable natives, and adds no runtime hooks.

- **Windows without crate-private hooks.** Integration tests cannot use the
  `cfg(test)` `HookPoint`s, so windows are opened from outside: a gate inside
  a leaf host effect holds a run just before its `__rt.need` (before
  registration); a driver that needs through `RunGuard::need` and blocks
  before `RunGuard::park` holds the subscribed-but-not-parked window that a
  VM `need` closes inside one instruction; executor events mark a parked
  task; blocker tasks gated on every worker keep a woken task queued, so an
  action's outcome is decided by the actions, not by timing. The window
  inside the cell lock remains with `src/task/handoff_tests.rs`.
- **Bounded harness.** Each scenario runs on its own thread under a whole-
  scenario bound; every wait inside it (events, notes, gates, outcomes,
  shutdown) has a per-wait bound. On failure or hang the harness notes (one
  totally ordered log of run starts/ends, effects, gates, claimed requests)
  and the executor record are dumped. After the body it checks the
  invariants and that nothing is left queued or parked.
- **Seeded schedules** fix a scenario's shape and placements per seed; the
  threads still race and only interleaving-independent facts are asserted.
- **Wait cycles** use `DependencyGraph` on the test side, fed from the
  executor's real `Waiting` releases (the executor has no producer
  bindings); the settlement policy is the simulated host's: fail the
  newest demand's own result.

## Failures and cancellation through native continuations (clj-quickjs-28)

Contract: `src/vm/native.rs` ("The protocol", "Errors", "Safepoint contract
and debug borrow checks") and `src/task.rs` ("Cancellation and finally
blocks", native frames). Design: `design.md` §§10–11, 13.

- **Failure actions.** `VmState::raise` validates the innermost handler,
  then calls `native::unwind(natives, handler_frame)`: every native frame
  with `depth > handler_frame` (started by the handler's frame or above;
  depths strictly increase along the stack), innermost first, receives a
  `Failure` chosen by what it awaited (`Callback`, `Dependency`, `Step`)
  through `NativeState::fail` and answers an action; `Unwind` pops it.
  Any other answer makes the raise `Broken` (an internal failure), so no
  native resumes from a half-unwound state. Then the call frames above the
  handler's frame are popped and the value stack is cut to the handler's
  height, which lies below every passed native's operand area. Both
  current natives answer `Unwind`.
- **Producer failures in `needAll`.** The run guard's `vm_need` clears the
  pending need and raises at the native's starting instruction through
  `raise_failure`, now caught around the native; the same holds when the
  failure is delivered by the run that resumes a parked task, on any thread.
- **Cancellation.** `VmState::unwind_cancel` calls `native::discard`,
  which truncates the passed natives without running native code; the
  waiter entry was already withdrawn by `RunGuard::begin_cleanup`, so no
  cell lock or producer is held or blocked by the cleanup. With no
  finally block left the whole VM state (natives included) is dropped.
- **Debug borrow checks.** A per-thread counter (debug builds only)
  counts `ScopedBorrow`s; the trampoline holds one around each
  `NativeState::step` and `fail`. `native::assert_no_borrow` panics in
  `dispatch::execute` entry, `Heap::safepoint` (every VM poll), the
  `Flow::Need` exit, `VmState::push`, `VmState::enter`, `native::start`,
  `Task::try_begin_run` and `RunGuard::park`. Release builds have no
  counter and the checks are empty.
- **Tests.** `tests/native_unwind.rs` (callback exception and producer
  failure, both via `needAll` and via a callback's `need`, caught in the
  caller after resuming on another thread; release of what only the native
  held while the task is still alive; nested natives; migration to the
  other executor worker with a task-owned builder; cancellation inside a
  callback and inside `needAll` with finally blocks, budgeted cleanup and
  another consumer of the same producer), `src/vm/native.rs` (borrow
  check rejections, unwind/discard selection), and the updated pinned
  tests in `tests/native_fold.rs` and `tests/exception_waits.rs`.

## String values (clj-quickjs-65)

Contract: `src/value/string.rs` (`JsString`, `number_to_string`),
`src/value.rs` (`Value::String`, `Primitive::String`, `ValueView::String`),
`plus`, `to_js_string` and `loose_equals` in `src/vm/dispatch.rs`.
Design: `design.md` §§4, 11.

- **Representation: a primitive over immutable `Arc` storage, not a heap
  object.** `Value::String(JsString)` holds a thin `Arc` (one pointer, so a
  raw value stays two words) to an immutable body that owns the code
  units. Strings have no heap edges, so the body is a documented coarse
  `Arc` storage boundary like executable code and collection trie nodes:
  the collector never traces or sweeps it, and a string lives exactly as
  long as some value holding its `Arc` (frame slot, operand, capture,
  collection entry, root, constant pool, an embedder's `JsString`). This is
  Rust ownership of an acyclic string graph, not reference counting of heap
  objects: string children cannot reference JavaScript values, so no cycle
  can pass through one.
  The alternative, a new heap kind behind `Value::Object`, was rejected:
  `===`, `==`, ToBoolean and persistent-map hashing and key equality
  (`collections::map::key_eq`/`key_hash`, which by contract never read the
  heap) would all need a heap read per comparison, and `typeof` and
  publication would have to inspect the object to know a value is a string.
- **Consequence for the value model.** `Value` and `Primitive` are no
  longer `Copy` (copying a string is an O(1) `clone`, an atomic increment);
  `ValueView` stays `Copy` (`ValueView::String` carries no contents; read
  them with `Rooted::string` or `Scope::string`, which return a `JsString`
  that outlives the handle). Atoms box values as before; a reader clones
  the boxed value inside its mutator registration, and each collection now
  releases every retired box of every live atom while the world is stopped,
  so a replaced string body never outlives the grace period even if the
  atom is not written again.
- **Publication and persistence.** A string is immutable and therefore
  persistent and shareable by construction: `value::share::check` treats
  it as a primitive (no walk), a persistent collection stores it without
  copying, and another task reads the same body. Map keys compare and hash
  by content (SameValueZero), so `__rt.get(__rt.map("k", 1), "k")` is 1;
  atom compare-and-set compares strings by content (SameValue: strings have
  no identity).
- **UTF-16 code units.** The body stores `u16` code units, not Rust `str`
  bytes: JavaScript defines `length`, indexing and relational order on code
  units, and a string may hold lone surrogates. The lexer decodes a string
  literal straight to code units (clj-quickjs-141; the token and the
  syntax tree carry a `JsString`): raw source characters are UTF-16
  encoded, and each `\u` escape appends its code unit (or, above U+FFFF,
  its pair) as is, so a lone surrogate escape is a lone code unit and two
  surrogate escapes that spell a pair give the same code units as the
  astral character (QuickJS `js_parse_string` likewise). Every operation
  that compares strings reads only the code-unit sequence, never how it
  was produced: equality (`===`, `==`, SameValue in atom compare-and-set),
  persistent map keys (`content_hash` and equality in
  `src/collections/map.rs`), constant deduplication and relational order,
  so `"\u{dbff}" + "\u{dfff}" === "\u{10ffff}"`.
  `length` is `JsString::len` (stored count, O(1)); indexing reads one
  stored unit (`JsString::code_unit_at`, O(1) after the first flatten,
  clj-quickjs-142/-146); relational order (`<` etc.,
  clj-quickjs-68) is `JsString`'s `Ord`, lexicographic over code units
  (QuickJS `js_string_compare`), not Unicode scalar order. Reading a
  string never panics on a lone surrogate: `code_units()` is exact,
  `Debug` is exact (a quoted literal that writes a lone surrogate as
  `\u{d800}`; `src/main.rs` prints a string result with it), and text for
  diagnostics and host effects (`to_string_lossy`, `Display`) maps a lone
  surrogate to U+FFFD.
- **Flat buffers and ropes.** Every string uses the 16-bit form.
  Concatenation up to 32 units copies into a flat body; larger results link
  the two operands in O(1), with a cached length (clj-quickjs-146). Empty
  operands return the other's body. `OnceLock<JsString>` caches a flat leaf
  at the first contiguous read, comparison or hash. The cache changes no observable
  content and is shared safely across readers. A Latin-1 form remains a
  possible private representation. The length limit is QuickJS's
  `JS_STRING_LEN_MAX`, 2^30 - 1 code units; a longer concatenation fails
  the task with `OutOfMemory` ("string too long", QuickJS's InternalError).
- **Instructions.** String literals are constants (`ExecutableSpec::constants`,
  `Primitive::String`; deduplicated by content; the `TooLarge` limit of
  65,536 distinct constants is unchanged). `plus` (opcode `0x49`) is
  JavaScript's binary `+` on primitives: two numbers add; when either
  operand is a string the ToString of both concatenate (clj-quickjs-66);
  an object operand fails with `UnsupportedOperand` (ToPrimitive is
  clj-quickjs-70); other primitives add their ToNumber. ToString of a
  primitive (`to_js_string` in `src/vm/dispatch.rs`) is `"undefined"`,
  `"null"`, `"true"`/`"false"`, the string itself, or Number::toString in
  radix 10 (`value::string::number_to_string`): the shortest round-trip
  digit count from Rust's `{:e}` formatting, the digits themselves the
  closest such candidate with ties to even (Rust's correctly rounded
  `{:.*e}`, since `{:e}` rounds exact decimal ties up:
  `1000000000000000.25` is `"1000000000000000.2"`), laid out by the
  ECMA-262 rules
  (plain decimal for `1e-6 <= |x| < 1e21`, else `1e+21` / `1e-7`;
  `"0"` for `-0`). Radix 2..36 formatting is implemented by `number_to_string_radix` (clj-quickjs-149; see below). The compiler emits
  `add` when both operands are statically numbers and `plus` otherwise, so
  `+` never applies ToNumber to a string. `to_number` on a string is
  StringToNumber (`string_to_number`, clj-quickjs-67; QuickJS `js_atof`
  semantics): whitespace-trimmed, unsigned radix-prefixed integers rounded
  correctly at any length, decimals through Rust's correctly rounded
  `f64` parser after the text is checked against the StrDecimalLiteral
  grammar (so Rust's `inf`/`nan` spellings stay NaN), signed `Infinity`,
  NaN otherwise. `===` and `==` compare two strings by content; `==`
  between a string and a number or boolean compares numbers (the boolean
  as 0 or 1). The relational operators (clj-quickjs-68) lower to
  `compare_lt`/`compare_gt`/`compare_le`/`compare_ge` (opcodes
  `0x4A`–`0x4D`, IsLessThan on primitives, `is_less_than` in
  `src/vm/dispatch.rs`) when neither operand is statically a number: two
  strings compare by `JsString`'s code-unit `Ord`, any other pair by
  ToNumber; with a static number on either side two strings are
  impossible, so the compiler keeps `to_number`/`to_number_pair` and the
  number-only `lt`/`gt`/`le`/`ge`. Calling a string is a
  TypeError "string is not a function"; an uncaught thrown string reports
  its text.
- **Tests.** `tests/js_source.rs` `string_values`, `tests/publication.rs`
  `strings_publish_as_persistent_values`, `tests/gc.rs`
  `strings_in_frames_captures_and_collections_survive_collections`, and
  unit tests in `src/value/string.rs`, `src/value.rs`,
  `src/collections/map.rs`, `src/atom.rs` and `src/value/share.rs`.

## Ordinary objects (clj-quickjs-114)

Contract: `src/object.rs` (`OrdinaryObject`, `PropertyKey`,
`PropertyKeys`), `src/vm/property.rs` (`GetField`, `PutField`,
`DefineField`, and which values have properties), `src/value/share.rs`
(publication). Design: `design.md` §§4, 11, 14.

**Decision.** An ordinary JavaScript object is `HeapObject::Plain` (the
existing kind, given storage: kind registry and `ObjectCounts::plain`
unchanged) holding an `OrdinaryObject`:

- its **owner** (`Option<TaskId>`: the task whose `NewObject` made it, or
  `None` for an embedder's `Scope::new_object`);
- **own properties in creation order**: a `Vec` of (key, property) entries,
  plus a key → position hash index once there are more than 8; a
  redefinition keeps the first position; each property is a data property
  (value plus `writable`/`enumerable`/`configurable`);
- a **`[[Prototype]]`** slot (`Option<ObjectId>`, null for now) and an
  **`[[Extensible]]`** flag.

It is `Shareability::TaskOwned` mutable state, changed in place under the
heap's object-table lock by its owning task's run only, exactly like a
binding cell or a transient builder: another task's `GetField`/`PutField`
fails with `VmErrorKind::ForeignObject` (not catchable, like
`ForeignBinding`); an embedder-made object has no owner task, so every task
may read it and none may change it. Ownership follows the task, not a
thread, so an object migrates with its task between workers. No per-object
lock exists: the owner is the only reader and writer, and each access takes
the one short object-table lock every heap read takes.

**Property keys.** A `PropertyKey` is a private `u32` issued by the
runtime-wide `PropertyKeys` table owned by the heap (one per runtime). Names
are interned on first definition (`PutField`, `DefineField`); `GetField`
only looks up, so reading a never-defined name grows nothing. Keys compare
and hash by their private id (derived Rust equality), never by a user hook;
the table's `HashMap<JsString, PropertyKey>` uses the standard randomized
hasher. Its `RwLock` is a leaf lock, never held with another heap lock, and
consulted before the object lock is taken. A symbol key (clj-quickjs-127)
will be a fresh id without a name entry. They are QuickJS's `JSAtom`s but
are not called atoms (`crate::atom` is the concurrent reference).

**Tracing and publication.** `OrdinaryObject`'s `Trace` reports every
property value and the prototype; the root inventory lists the holder
(`object_property_retains` / `object_property_reclaims`). Publication
rejects any value reaching an ordinary object, and describes the path on
into its property values (`.property(name)`, "Publication rules" above).
A closure that captured a `const` bound to an object captures the object by
value and is rejected at `.capture(i)`.

**Which values have properties** (`src/vm/property.rs` module table): an
ordinary object reads its own property, then its prototype chain, then
gives `undefined`; a write updates or adds an own data property. The
runtime's other heap values (persistent vectors, maps, occurrences,
builders, atoms, binding cells, non-error natives) have no JavaScript
properties: a read gives `undefined`, a write is a catchable `TypeError`,
as for a non-extensible object without properties in strict mode. Function
objects expose code metadata and their property record (clj-quickjs-101/-125).
Caught engine errors use ordinary Error instance properties (clj-quickjs-230).
Numbers and strings read through their prototypes (clj-quickjs-73,
"Primitive bases"); booleans remain deferred (clj-quickjs-39). `undefined` and
`null` bases throw catchable `TypeError`s (`cannot read property 'x' of
undefined`, `cannot set property 'x' of null`) at the member expression;
for a write the right-hand side is evaluated first (PutValue).

**Alternatives considered.**

- *Persistent (immutable, shareable) objects*, updated by path copying like
  persistent maps: `o.x = v` would have to replace `o` in every binding
  and property that refers to it, which JavaScript identity semantics
  (`a = b; a.x = 1; b.x`) forbid. Rejected; persistent maps remain the
  shareable record type, and an object's contents can be copied into one
  explicitly.
- *Objects shared between tasks with a per-object lock* (or
  `Arc<Mutex<_>>` per object): the design (§4) excludes general shared
  mutable object graphs, the group constraints forbid universal
  `Arc<Mutex<_>>` objects, and every read would pay a lock. Rejected; the
  explicit concurrent reference is the atom.
- *A new heap kind beside an identity-only `Plain`*: two kinds for one
  JavaScript concept, with `NewObject` and the embedder producing different
  ones. Rejected; `Plain` gained storage.
- *String keys compared by content* (no intern table): no shared table and
  no table lock, but every lookup compares code units, and symbol keys,
  which have identity and no name, would need a second key form. Rejected
  for the private-id key space that both kinds share.
- *Shape (hidden-class) transitions with inline caches*, as QuickJS's
  `JSShape`: faster repeated access, but shapes are a runtime-wide shared
  structure that tasks on different workers would contend on, and nothing
  here needs them yet. Deferred: the entry vector can later be split into a
  shape plus a value array without changing the instruction contract.

**Consequences.**

- Property access costs one object-table lock per object visited plus a
  read lock on the key table per access (the name is an executable
  constant, but executable code is runtime-independent, so its keys are
  looked up at run time). A per-task or per-code key cache is a later
  optimization.
- Interned names are never removed: the table grows with the number of
  distinct names ever written in the runtime (a write interns its name
  before it looks at the object, so a write that then fails interns too).
  Computed keys (clj-quickjs-115) make that data-dependent
  (`o["k" + i] = i` in a loop interns one name per iteration). Decision
  (clj-quickjs-115): computed access ships without key reclamation, and
  the growth is documented as an explicit gap (`docs/grammar.md`,
  "Property names are never forgotten"). Reads (`GetField`, `GetElem`)
  only look names up and array indices on arrays are never interned, so
  only writes of new names grow the table, by one entry per distinct
  name (not per object). Reclaiming unused names needs the collector to
  mark the keys of every live object, the realm and any key an operation
  holds across a safepoint, and to reissue an id only after a full
  mark: that is clj-quickjs-259.
- The storage leaves room for the later pellets: the entry's value becomes
  a data/accessor enum for getters and setters (clj-quickjs-119/-120);
  `prototype` gets set by `Object.prototype` and `Object.create`
  (clj-quickjs-117/-121), at which point a write of an absent property must
  consult the chain (`SetOutcome::Inherited`; done, see "Realm intrinsics"
  and "Prototype reflection");
  `extensible` is cleared by `Object.preventExtensions`/`freeze`
  (clj-quickjs-122; done, see "Extensibility and freezing"); creation order is kept for enumeration, where integer
  keys must sort first (clj-quickjs-123); `delete` removes an entry and
  shifts the index (clj-quickjs-124; done, see "The `delete` operator"); function objects, which are
  persistent today, need a separate task-owned property store for their
  own properties (clj-quickjs-125).
- Since clj-quickjs-117 a literal's `[[Prototype]]` is `Object.prototype`
  ("Realm intrinsics" below) and a write of a property the object does not
  have walks the chain (`SetOutcome::Inherited` is decided there); objects
  made by `NewObject` and by the embedder keep a null prototype.

## Realm intrinsics (clj-quickjs-117)

Contract: `src/realm.rs` (the realm, its intrinsic objects, built-in global
names), `src/vm/builtin.rs` (the built-in function kind and the `Object`
functions), `src/vm/property.rs` (prototype-chain reads and writes),
`src/vm/instr.rs` (`GetBuiltinGlobal` 0x52, `CallMethod` 0x53,
`NewObjectLiteral` 0x54). Design: `design.md` §§4, 5, 11.

The question: intrinsics (`Object.prototype`, `Object`, later
`Function.prototype`, `Array.prototype`, ...) must be reachable from every
task, while ordinary objects are task-owned (above) and persistent
functions must stay publishable, with no lock on ordinary property reads,
no per-worker imports, and the intrinsics as collector roots.

**Decision: one shared, immutable realm per runtime.** The heap owns at
most one realm (`Heap::realm`, a `OnceLock`), built by the first
instruction that needs it (`NewObjectLiteral`, `GetBuiltinGlobal`) and kept
until the heap is dropped. Its objects are:

- `Object.prototype`: an ordinary object (`HeapObject::Plain`) marked
  *intrinsic*: no owner, null prototype, own `constructor`, `toString`,
  `valueOf`;
- built-in functions (`Object`, `Object.is`, `Object.prototype.toString`,
  `Object.prototype.valueOf`): a new heap kind, `HeapObject::Builtin`
  (`ObjectKind::Builtin`, appended), holding a `Builtin` id naming Rust code
  and an intrinsic `OrdinaryObject` as its property store (`length`,
  `name`; `Object` also `prototype` and `is`). clj-quickjs-119 appended
  `Object.defineProperty` and `Object.getOwnPropertyDescriptor` ("Property
  attributes" below), clj-quickjs-121 `Object.getPrototypeOf`,
  `Object.setPrototypeOf` and `Object.create` ("Prototype reflection"),
  clj-quickjs-122 `Object.preventExtensions`, `Object.isExtensible`,
  `Object.freeze` and `Object.isFrozen` ("Extensibility and freezing").

Every task reads them without any lock (an immutable copy of each
intrinsic's properties and prototype is published with the realm, and
property reads that reach an intrinsic use it instead of the object table;
`intrinsic_property_reads_do_not_lock_heap`) and gets the same identities; no task changes them: any write to an intrinsic, of any
property, is a catchable `TypeError` ("built-in objects are shared by all
tasks and immutable", `docs/grammar.md` "Explicit gaps"). The property
attributes are still the standard ones (methods writable, non-enumerable,
configurable; `length`/`name` read-only; `Object.prototype` fixed), so an
ordinary object that inherits `toString` can be given its own:
immutability comes from sharing, not from the descriptors, which avoids
the "override mistake" a frozen prototype would cause (`o.toString = f`
and `C.prototype.toString = f` would both throw). The realm is built
completely (allocations and property definitions while the building thread
is a registered mutator between safepoints, so no collection intervenes),
marked known-shareable, then installed; later reads are one atomic load.
Concurrent first uses serialize on a build lock that exists only for that
moment (a leaf lock, never taken by the collector).

- **Publication.** Intrinsics never change once installed, so they are
  `Shareability::Persistent` (`classify`: `Builtin`, and `Plain` when
  intrinsic) and known shareable from creation. A closure that reads
  `Object` captures nothing (the name compiles to `get_global`,
  resolved through the running task's global object, clj-quickjs-104); one that captured
  `Object` or `Object.prototype` by value still publishes. Objects that
  inherit from `Object.prototype` stay task-owned: an edge from a
  task-owned object to an intrinsic is allowed, the reverse never exists.
- **Migration.** Nothing is per worker or per task: a task that moves
  between workers keeps reading the same realm.
- **Roots.** The realm's objects are roots of the new category
  `RootCategory::Intrinsic` while the heap lives (root inventory above);
  their property values are reached through them.
- **Names.** A free identifier that is a built-in global (`Global`:
  `Object` today) resolves after every lexical scope and before the
  `ReferenceError` (`frontend::lower`, `Access::Global`), so `let Object`
  shadows it. `typeof Object` is initially `"function"`. These are writable global-object
  properties (clj-quickjs-104); replacing a binding leaves the shared
  intrinsic itself unchanged.
- **Calls.** `Call` and `CallMethod` accept a built-in function: it runs to
  completion inside the instruction (a poll point; it may allocate, as
  `Object()` does, and its result replaces the operands at once), and so
  does a built-in passed as a native callback (`__rt.reduce`). A later
  built-in that must run JavaScript (a getter, a user `toString`, a
  callback) is started as a resumable native (`crate::vm::native`), so that
  JavaScript runs in ordinary VM frames across which the task can yield,
  wait or migrate; the `Builtin` dispatch is where that choice is made.
- **Method calls.** A call whose callee is `o.m` evaluates `o` once and
  emits `CallMethod`, which passes `o` as the receiver: built-ins see it
  as `this`, and a JavaScript function callee gets it as its frame's
  `this` (clj-quickjs-116, "Calls and frames"). A plain call passes
  `undefined` (strict mode), so `let f = o.toString; f()` is
  `"[object Undefined]"`.
- **Literals.** Object literals compile to `NewObjectLiteral`, which
  allocates an owned object whose prototype is `Object.prototype`, like
  `Object()`; `NewObject` keeps a null prototype (OrdinaryObjectCreate(null),
  the prototype `Object.create(null)` gives, clj-quickjs-121) and never builds the realm, so
  hand-assembled programs that only allocate objects do not pay for it.

**Alternatives considered.**

- *Per-task realms*: each task lazily materializes task-owned intrinsics,
  and objects and functions refer to intrinsic prototypes by a
  realm-relative reference resolved through the running task. Built-ins
  would be fully mutable per task (monkey-patching works), but every task
  that touches an object would allocate and initialize its own realm
  (thousands of short tasks), `Object` would be a different object in
  every task (a published closure that captured it would carry another
  task's object, which publication must then reject), every prototype edge
  of a persistent value (a function's `Function.prototype`) would need the
  indirection, and a change one task made (for example load-time code
  extending a built-in prototype) would be invisible to the others.
  Rejected.
- *Shared mutable intrinsics* behind a lock or with a writer protocol:
  every ordinary property read that reaches a prototype would pay for the
  synchronization, and `design.md` §4 excludes general shared mutable
  object graphs. Rejected.
- *Frozen intrinsics* (non-writable descriptors): the same immutability,
  but the override mistake above. Rejected in favour of immutability by
  sharing.
- *Eager creation in `Runtime::new`*: simpler to reason about, but every
  runtime and every collection pays for objects most early VM programs
  never use, and heap counts in every test would include them. The lazy,
  build-once realm gives the same identities.

**Consequences.**

- Programs cannot add or change properties of built-in objects. Code that
  patches built-in prototypes (ClojureScript's `extend-type` on a native
  type writes `Date.prototype.…`) needs the compiler to lower such
  extension differently (`design.md` §5 allows private lowering), or a
  later per-task overlay; this is listed under "Explicit gaps".
- Later intrinsics follow the same pattern: `Function.prototype`
  (clj-quickjs-102), `Array.prototype` (added by clj-quickjs-118, with an
  `Array` built-in as its `constructor`) and the `Array` global and methods
  (clj-quickjs-39, -153),
  `Symbol.prototype` (clj-quickjs-129) and the error prototypes and
  constructors (clj-quickjs-230) are added to `realm::build` as intrinsic
  ordinary objects and `Builtin` functions, their constructors to `Global`
  (append-only, the operand values are part of the instruction format), and
  their methods as `Builtin` variants. `Function.prototype`
  (clj-quickjs-102) is itself a built-in function (callable, returning
  `undefined`) inheriting from `Object.prototype`; built-in functions get
  it as their `[[Prototype]]` and function objects, which are persistent,
  as an implicit prototype read through the realm (their property records
  inherit from it). Its standard names that do not exist yet (`bind`,
  `toString`, `arguments`, `caller`) fail with `UnsupportedOperand` when a
  lookup reaches it without finding them, instead of reading
  `Object.prototype`'s or `undefined`. `Function` is a stub constructor
  (failing when called, clj-quickjs-111) and `Reflect` an intrinsic
  ordinary object with `apply`.
- The realm's objects (twenty-seven since clj-quickjs-73, thirty-three since clj-quickjs-102) and its interned names live as long as the
  runtime: the first object literal adds them to the heap counts.
- Embedder-made objects keep a null prototype (`Scope::new_object`); an
  object that should inherit from `Object.prototype` is made by a task (a
  literal or `Object()`).

## Array objects (clj-quickjs-118)

Contract: `src/object/array.rs` (`ArrayObject`, `array_index`),
`src/vm/property.rs` (`GetElem`, `PutElem`, `NewArray`, `AppendElement`,
`AppendHole`; property keys and ToPropertyKey), `src/value/share.rs`
(publication). Design: `design.md` §§4, 11. QuickJS reference:
`js_create_array`, `JS_NewArray`, `OP_array_from`, `OP_get_array_el`,
`OP_put_array_el` and its fast arrays (`research/quickjs/quickjs.c`,
revision 7d1c13c5d8f8); no code is ported.

**Decision.** A JavaScript array is a new heap kind,
`HeapObject::Array` (`ObjectKind::Array`, `ObjectCounts::array`),
holding a boxed `ArrayObject` (boxed so that its storage does not enlarge
the slot of every heap object):

- an embedded **`OrdinaryObject`** for everything an array shares with
  an ordinary object: the owner task, the `[[Prototype]]`, the
  `[[Extensible]]` flag and the own properties whose key is not an array
  index (`a.name = 1`, `a["01"]`, `a[-1]`), so ownership, foreign-task
  failures and named-property storage are exactly "Ordinary objects";
- **elements** in task-owned storage: a dense `Vec<Option<Value>>` for
  indices below its length (`None` is a hole, an absent property), and a
  sparse `BTreeMap<u32, Value>` fallback for indices at or beyond it. A
  write lands in the dense part when it is inside it, or when the sparse
  part is empty and the index is at most 1024 past the dense end (the gap
  fills with holes); otherwise in the sparse part. The dense part thus
  only grows while the sparse part is empty, so every sparse index lies
  beyond every dense one and elements enumerate in index order without
  sorting. Literals and sequential appends stay dense; `a[4294967294] = 1`
  costs one map entry;
- **`length`**, a `u32` that is the own `length` property (non-enumerable,
  writable, non-configurable, never stored as a named property): one more
  than the greatest index present, or more (trailing holes). Writing an
  index at or beyond it sets it to index + 1.

An array index is the canonical decimal string of an integer
`0 ..= 2^32 - 2` (`"0"`, `"7"`, not `"07"`, `"-0"` or `"4294967295"`).
Property access carries a key as `Key::Index(u32)` or `Key::Name`: a
number that is such an integer (`-0` included) converts to an index
without building a string, a string is parsed once, and an ordinary
object stores an index key under its name. `__rt.vector(...)` values are
untouched: a persistent vector is not a JavaScript array (it has no
JavaScript properties: `v[0]` and `v.length` read `undefined`), and an
array is not a persistent vector (the collection intrinsics reject it as
they reject an ordinary object).

Arrays are `Shareability::TaskOwned`, like ordinary objects. The
collector traces every element, then the embedded object (root inventory:
`array_element_retains` / `array_element_reclaims`). Publication rejects
a value that reaches an array at the array, and the description walks on
into its elements, in index order and before its named properties, with
a `PathStep::Element(i)` step (`value[1][3]`), to the innermost
task-owned object along the first such chain.

Array literals compile to `NewArray` (`0x57`), then per element its
value and `AppendElement` (`0x58`), or `AppendHole` (`0x59`) for an
elision; computed member access to `GetElem` (`0x5A`) and `PutElem`
(`0x5B`), which run ToPropertyKey on the key operand; `o[k](...)` is a
method call (`Dup`, `GetElem`, `CallMethod`). `o[k]++` (clj-quickjs-76) adds
`ToPropertyKey` (`0x5D`: o k → o key, the base check and the one key
conversion) and `Dup2` (`0x5C`), so the reference is read with `GetElem`
and written with `PutElem` without evaluating or converting the key again;
the converted key (an index as a number, any other name as a string)
converts to itself, so the later conversions are unobservable. Only `NewArray` allocates a
heap object (a poll point; it also builds the realm on first use);
element storage grows outside the object table, as property storage
does.

**Prototype.** Following the realm decision of clj-quickjs-117 ("Realm
intrinsics"), `Array.prototype` is a shared immutable intrinsic: an
empty array represented by ordinary property storage inheriting from
`Object.prototype`, with an own `length`
of 0, a `push` method (clj-quickjs-155), `join` and `toString`
(clj-quickjs-156), and a
`constructor` that is the callable and constructible global `Array`
(clj-quickjs-153/-155). Its realm identity carries the array brand for
`Array.isArray` and `Object.prototype.toString`; an object merely inheriting
from it is not an array. Every array literal
inherits from it; a hole or a missing name reads along the chain, and a
write of an absent index or name consults the chain (OrdinarySet) before
the element or property is added.

**Alternatives considered.**

- *Back arrays by the persistent vector of `src/collections`* (a
  task-owned array object holding a `PVector` in a mutable slot,
  replaced on each write). Snapshotting an array into a publishable value
  would be O(1), but every element write would path-copy and allocate trie
  nodes, a persistent vector has no hole (absent element) distinct from
  `undefined`, and an index near `2^32 - 2` would need a 4-billion-slot
  trie or a second representation anyway. Rejected: the vector stays the
  shareable sequence type (`__rt.vector`), and converting an array into
  one is an explicit copy (a later intrinsic if fixtures need it).
- *Elements as ordinary named properties* (QuickJS's slow arrays only):
  no new storage, but every index would be interned in the runtime-wide
  key table (which never shrinks), each element would cost a key lookup
  under the table lock, and index order would need a sort. Rejected.
- *A dense vector only*: holes are cheap, but `a[4294967294] = 1` would
  allocate gigabytes. Rejected for the dense-plus-sparse split, which
  costs one extra branch on a miss.
- *An array flag inside `OrdinaryObject` (no new heap kind)*: fewer
  matches to extend, but every ordinary object would carry or check the
  element storage, heap statistics could not count arrays, and
  publication errors could not name the kind. Rejected; the embedded
  `OrdinaryObject` already gives the shared behaviour.

**Consequences.**

- `typeof []` is `"object"`, `[] !== []`, and an array behaves as an
  ordinary object for every named key, so later object pellets (getters,
  `Object.keys`, `delete`, `defineProperty`) apply to the embedded object
  and must add the elements: enumeration lists indices first in ascending
  order (clj-quickjs-123), `delete a[i]` makes a hole (clj-quickjs-124, done),
  and a non-default element attribute (`defineProperty`, freezing) needs
  per-element flags or a move to a named property, which the sparse part
  can take (clj-quickjs-119 added a sorted map of the exceptions;
  clj-quickjs-122 a `frozen` flag that changes the default, so freezing
  adds no entries: "Extensibility and freezing").
- `Object.getPrototypeOf([])` names `Array.prototype` (clj-quickjs-121);
  the `Array` global also names it (clj-quickjs-155). Its methods are
  added to the realm (`realm::build`, `Builtin`, append-only).
- Assigning or defining `length` uses ArraySetLength (clj-quickjs-154,
  "Array length changes" below). The `Array` constructor and `Array.isArray`
  are supported (clj-quickjs-153); remaining methods belong to their own built-in pellets.
- Computed keys make the property-key table data-dependent: `o[k] = v`
  with arbitrary string keys interns each new name (array indices on
  arrays never do). Key reclamation by the collector is clj-quickjs-259
  ("Ordinary objects", Consequences).
- These instructions are all of computed member access
  (clj-quickjs-115 added its tests and documentation, no new
  representation): `a[k]` and `a[k] = v` with primitive keys on every
  base. Object keys (ToPrimitive, clj-quickjs-70) fail with
  `UnsupportedOperand`, and `o[k](...)` is a method call passing `o`
  (clj-quickjs-117's `CallMethod`).

## Property attributes (clj-quickjs-119)

Contract: `src/object.rs` (`PropertyFlags`, `PropertyDescriptor`,
`OrdinaryObject::define_own_property`), `src/vm/descriptor.rs`
(ToPropertyDescriptor, FromPropertyDescriptor and the two built-ins),
`src/vm/property.rs` (`define_own_property`, `get_own_property`,
`find_field`, and `put_field`'s strict/sloppy rejection). Semantics:
`docs/grammar.md`, "Execution".

**Decision.** The attributes live where clj-quickjs-114 put them: each
entry of an object's creation-ordered property `Vec` carries its value and
`writable`/`enumerable`/`configurable` flags, so no new storage, heap kind
or index was needed. `OrdinaryObject::define_own_property` is ECMAScript's
OrdinaryDefineOwnProperty with ValidateAndApplyPropertyDescriptor for data
properties: creation defaults absent fields to `undefined`/false, an update
keeps absent fields and the entry's position, and a non-configurable
property rejects every change except `writable` true to false and
SameValue rewrites. Rejections report `Rejected(reason)` and change nothing.
clj-quickjs-120 adds accessor descriptors and resumable field reads; see
"Accessor properties" below. The two built-ins are appended to the realm's
`Object`.

- **Descriptor reads are `[[Get]]`s.** ToPropertyDescriptor reads the six
  fields through `property::read`, the prototype-chain read that
  tells an absent field from one holding `undefined`, so inherited fields
  count. Accessor fields run through a resumable native cursor
  (clj-quickjs-120), with getters in ordinary VM frames that may yield,
  wait or migrate. The decision point is the built-in dispatch.
- **Writes and modes.** `PutField` passes the running code's
  `Code::strict` (clj-quickjs-95) to `put_field`: a write that `[[Set]]`
  rejects (an own or inherited read-only property, a non-extensible
  receiver) is a catchable `TypeError` in strict code and does nothing in
  sloppy code. Writes to realm intrinsics stay a `TypeError` in both
  modes: that is this runtime's sharing rule, not an ECMAScript `[[Set]]`
  result, and silently dropping it would hide the restriction.
- **Intrinsics.** `getOwnPropertyDescriptor` reports the standard
  attributes of built-in objects' properties (`Object.length` is
  read-only and configurable); `defineProperty` on a built-in object is
  the same `TypeError` as a write, so a configurable intrinsic property
  still cannot be reconfigured. The realm's immutable views stay exact.
- **Descriptor objects** are fresh task-owned ordinary objects inheriting
  from `Object.prototype`, built with all four properties before they are
  allocated, so no property value is held only by a Rust local across an
  allocation.

**Alternatives considered.**

- *A separate attribute table (or shape) per object*: would let objects
  with identical attributes share them, but nothing needs that before
  shapes exist ("Ordinary objects", alternatives), and it would split a
  property across two structures. Rejected.
- *Validation in the built-in instead of the object*: `delete`
  (clj-quickjs-124), `Object.freeze` (clj-quickjs-122) and accessor
  definitions (clj-quickjs-120) need the same rules, so they live on
  `OrdinaryObject` and the built-in only converts arguments.
- *Leaving sloppy writes as `TypeError`s until a later pellet*: no sloppy
  test could then assign a read-only property at all; the mode is already
  in the code, so the branch is one argument.

**Consequences.**

- Arrays (clj-quickjs-118) keep attributes beside their element storage:
  `ArrayObject` holds a sorted map of the elements whose attributes are
  not all true (empty for every literal, so plain arrays pay nothing) and
  a `length_writable` flag. `define_element` runs the same
  `validate_and_apply` as ordinary properties (a new element at or beyond
  a read-only `length` is rejected); `define_length` implements
  ArraySetLength (clj-quickjs-154). Writes consult both: a read-only element is a
  read-only property for `[[Set]]` (own or inherited), and a read-only
  `length` rejects new elements at or beyond it and `a.length = v`.
- clj-quickjs-120 adds getter/setter slots and real accessor definitions;
  `getOwnPropertyDescriptor` reports `get`/`set` instead of
  `value`/`writable`.
- `enumerable` is stored and reported; `Object.keys` enumerates with it
  (clj-quickjs-123), and `for`-`in` (clj-quickjs-85) will.

## Function own properties (clj-quickjs-125)

Contract: `src/vm/function.rs` ("Own properties": `FunctionObject::owner`,
`FunctionObject::properties`, `read`, `write_target`, `define_target`,
`own_store`), `src/vm/property.rs`
(function bases), `src/value/share.rs` ("Functions with own properties"),
`src/vm/executable.rs` (`ExecutableSpec::constructor`). Design: `design.md`
§4.

The question: JavaScript functions are objects (`f.x = 1`, `F.prototype`
for `new`, class statics), but function objects are `Persistent` (code plus
captured values, never changed) so that an untouched closure can be
published; ordinary objects are `TaskOwned` ("Ordinary objects"), and the
realm's intrinsics are shared and immutable ("Realm intrinsics"). A heap
object's category is fixed for its lifetime, so a function cannot turn
task-owned when it is first written.

**Decision: a lazily attached, task-owned property record.**

- A `FunctionObject` records its creating task (`owner`) and an optional
  **property record**: an ordinary object (`HeapObject::Plain`) owned by
  that task, never itself a JavaScript value, whose `[[Prototype]]` is
  `Function.prototype` (`Object.prototype` until clj-quickjs-102). It is
  allocated and attached by the first write of a property or the first
  observation of the lazy `prototype`, and never replaced. Every property
  operation on the function is the ordinary-object operation on the record
  (reads continue at `Function.prototype` when there is none), so attributes,
  extensibility, the owner check (`ForeignObject` for another task) and the
  prototype-chain write check come from `crate::object` unchanged, and the
  later define, delete and enumeration pellets inherit them.
- **`prototype`.** Code compiled from a `function` declaration or
  expression has `ExecutableSpec::constructor` set (not arrows, scripts or
  hand-assembled code). Its function objects have an own `prototype`
  (writable, not enumerable, not configurable) created on first
  observation (a read, or the first creation of a property record), as QuickJS's
  `js_instantiate_prototype` does: a new ordinary object owned by the same
  task, inheriting from `Object.prototype`, with a non-enumerable
  `constructor` pointing back. It is stored in the record, so repeated
  reads give one object. `Object.defineProperty` and
  `Object.getOwnPropertyDescriptor` (clj-quickjs-119) on a function work on
  its record (`define_target`, `own_store`), materializing `prototype`
  first, so a partial descriptor is validated against it (the upstream
  `defineProperty(g, "prototype", {writable: false})` row) and its
  descriptor is reported. Metadata descriptors need no record (clj-quickjs-101).
- **Publication.** The function stays `Persistent` and its category never
  changes; the record is one more edge, which the publication walk follows
  like a capture, so a function with a record is rejected there:
  `value.properties` (a task-owned `object` owned by the creator), or
  `value.properties.property(prototype)` once `prototype` was read. An
  untouched function publishes as before. The edge is the one exception to
  "persistent edges never change", and the known-shareable bit stays sound
  because the two refuse each other under the object-table lock: a record
  is attached only while the function's bit is clear
  (`Heap::with_object_mut_known`), and a check sets bits only if no
  function it expanded has gained a record since it read it
  (`Heap::mark_shareable_unless`; otherwise it walks again). Hence a
  function is never known shareable at allocation any more: it becomes
  known when a check proves it (`Runtime::publish`, an atom install, a
  dependency result), and from then on it is **shared and immutable**, like
  a realm intrinsic: writing a property is a catchable `TypeError`, and
  creating its `prototype` fails with `UnsupportedOperand`.
- **Fit with the neighbours.** `length` and `name` (clj-quickjs-101) are
  derived from immutable code metadata and answered without creating a record.
  The first record creation copies both properties and materializes a
  constructor's `prototype`, in that order, before adding user properties.
  The record then contains all own properties and is authoritative, so
  deletion never reveals a code default again. `Function.prototype` (clj-quickjs-102) is a realm
  intrinsic: it is the record's prototype and where a record-less
  function's reads continue (`f.call`, `f.apply`, `f.constructor`). The
  names it does not have yet (`toString`, `arguments`, `caller`)
  fail with `UnsupportedOperand` unless the function
  has its own (`function::FUNCTION_PROTOTYPE_NAMES`), and every other name
  continues at `Function.prototype` and then `Object.prototype`. Class statics (`static m() {}`) and `F.prototype.m =
  ...` are record writes, so a class or constructor that has been set up is
  task-owned, like the objects it creates. Bound functions (clj-quickjs-103)
  use the same record. Built-in functions keep their immutable
  intrinsic property store.

**Metadata (clj-quickjs-101).** `ExecutableSpec::function_length` and
`function_name` retain immutable JavaScript metadata separately from the
argument-slot count and diagnostic name. Source lowering counts parameters
before the first default, and infers anonymous function/arrow names for
direct binding initializers, identifier assignments, parameter defaults,
and object literal property values. Inference changes no lexical self
binding. UTF-16 property names, including lone surrogates, remain exact.
Missing metadata in hand-assembled specifications defaults to their
parameter count and diagnostic name; `to_spec` preserves resolved metadata.

`function::own_metadata`, `read` and key enumeration answer from code
without a record or heap allocation (result descriptors/arrays are ordinary
task-owned objects). Rejected metadata writes likewise create no record.
Once the record exists, ordinary reads, descriptors, redefinitions,
deletion, freezing, and key order apply. Deleting either configurable
metadata property creates the record first, then removes that entry;
`Function.prototype` supplies the inherited 0/empty-string fallback.
`Object.getOwnPropertyNames` lists a constructor's lazy `prototype` without
materializing its value. Published functions stay record-free and immutable;
their metadata descriptors report non-configurable properties, matching
`isFrozen`. Reading metadata before or after publication preserves the
existing transitive capture checks. `bind` snapshots current own numeric
length minus bound argument count (clamped at zero) and `bound ` plus the
current string name, including deleted/redefined metadata and default arity.

Semantic references: `research/quickjs/quickjs.c`,
`js_function_set_properties`, `js_get_function_name`, and
`research/quickjs/tests/test_language.js` `test_function_length`,
`tests/test_builtin.js` `test_function`, revision `7d1c13c5d8f8`.
The new tests are synthetic fixtures; no upstream source or test text was copied.

**Alternatives considered.**

- *Functions task-owned from creation* (or a function object holding its
  properties inline): simple, but no closure could ever be published,
  which `design.md` §4 (and every published callback) relies on.
  Rejected.
- *Category decided at creation* (a function that "will be" mutated is
  task-owned): the compiler cannot know, since any reference may write a
  property. Rejected.
- *Per-task side tables* (function identity → properties, kept by each
  task): functions stay persistent and every task may annotate a shared
  function, but each task sees different properties of one object, and
  publishing a function silently drops the properties its creator gave it.
  Rejected: identity semantics would differ from JavaScript without an
  error.
- *Never caching functions as known shareable* (re-walk them on every
  publication) instead of freezing them once proven: keeps published
  functions writable by their creator, but every publication of any value
  containing a function would walk it again, and a function written after
  it was handed over would be seen by the receiver as another task's
  object (`ForeignObject`) rather than rejected at publication. Rejected in
  favour of "shared means immutable", which matches the realm.
- *Eager `prototype`*: every ordinary function would get a task-owned
  record at creation and none could be published. Rejected; the lazy
  object is also what QuickJS does.

**Consequences.**

- A function's first property write (or `prototype` read) costs two or
  three small allocations and makes it unpublishable; reads of absent
  properties allocate nothing. Property accesses on a function take one
  extra short object-table lock (to read its state) before the ordinary
  access on its record.
- Publishing a function freezes it: its creator can no longer give it
  properties, and `new` on it (clj-quickjs-71) fails if its `prototype` was
  never created. Code that sets up constructors (`F.prototype.m = ...`,
  ClojureScript's `deftype` and protocol extension) must do so before any
  value reaching `F` is published, or the compiler must lower that set-up
  to shareable data (`design.md` §5).
- Function objects are no longer known shareable at allocation, so the
  first publication of a value containing a closure walks it once; later
  ones stop at the cached bit as before.
- The record is a `PlainObject` in heap counts (`heap_usage().objects`) and
  a new root-inventory holder (`function_properties_retain` /
  `function_properties_reclaim`).

## Prototype reflection (clj-quickjs-121)

Contract: `src/vm/prototype.rs` (`[[GetPrototypeOf]]` and
`[[SetPrototypeOf]]` by kind, the three built-ins, the `__proto__`
accessor, `SetProto`), `src/vm/property.rs` (where the walk meets the
accessor), `src/object.rs` (`OrdinaryObject::set_prototype`),
`src/vm/instr.rs` (`set_proto`, `0x5F`). Semantics: `docs/grammar.md`,
"Execution".

**Decision.** The `[[Prototype]]` slot clj-quickjs-114 gave every ordinary
object and array becomes settable by its owner task: `Object.create`,
`Object.setPrototypeOf`, `o.__proto__ = p` and a literal's `__proto__: p`
(lowered to the appended `set_proto` opcode after the value, as QuickJS's
`OP_set_proto`) all end in `prototype::set_prototype_of`, which is
ECMAScript's OrdinarySetPrototypeOf: setting the current prototype is a
no-op for every value; otherwise the target must be an ordinary object or
array the running task owns and that is extensible, and the new chain must
not lead back to it. `getPrototypeOf`, `setPrototypeOf` and `create` are
`Builtin` variants appended to the realm's `Object`; none runs JavaScript,
so they run inside their call instruction.

- **Ownership of prototype links.** A link is an edge of the task-owned
  object holding it, traced with its properties, and changed only by the
  owner, so a chain the running task walks cannot change under it. It may
  point at the task's own objects, at realm intrinsics (shared and
  immutable), at embedder-made objects (readable by every task, changed by
  none) and at persistent values; never at another task's object, because
  every operation that installs a link first reads the new chain with the
  usual owner check and fails with `ForeignObject`. Since task-owned
  objects are never published, a link never crosses tasks except into
  shared, immutable or ownerless objects, and the publication check needs
  no change. Intrinsics keep their prototypes: changing one is the same
  `TypeError` as writing one of their properties, and their lock-free
  views stay exact.
- **The cycle check** walks the new chain through ordinary objects and
  arrays only, one short object-table lock per object, and stops at an
  intrinsic or any other kind: their prototypes are fixed and can never
  lead back to a task's ordinary object (an intrinsic's chain holds only
  intrinsics).
- **Other kinds.** Values without JavaScript properties (vectors, maps,
  builders, ...) report a `null` prototype and are not extensible, so
  setting anything else is a `TypeError`. Functions and built-in
  functions report `Function.prototype` (clj-quickjs-102); a built-in
  function's cannot change (an intrinsic: `TypeError`) and neither can a
  function's yet (`UnsupportedOperand`: its property record would have to
  take the new prototype, and the cycle walk would have to follow
  functions). Engine errors follow ordinary object prototype rules,
  initially inheriting from the matching Error prototype (clj-quickjs-230). A function used as a prototype
  (`Object.create(function () {})`) lends its own properties, read and
  write-checked through its record (`function::read`, `own_store`),
  with their attributes; the names `Function.prototype` lacks fail
  explicitly through the chain as on the function itself, and
  `Function.prototype`'s own (`call`, `apply`) are inherited. The `__proto__`
  setter always receives the original base, not the record a function's
  write is redirected to.
- **`Object.prototype.__proto__` is a stored accessor (clj-quickjs-261).**
  The realm stores built-in `get __proto__` and `set __proto__` functions
  through the accessor representation of clj-quickjs-120. The generic
  property walk selects them, preserving the original receiver, and
  descriptor reflection and own-name enumeration expose the property.
  Setter lookup on an intrinsic or function precedes data-write rules:
  an intrinsic receiver may make the no-op `Object.prototype.__proto__ = null`,
  and an ignored function assignment creates no property record. Neither
  built-in runs JavaScript. See "Stored prototype accessor" below.

**Alternatives considered.**

- *A native-accessor property kind now*: the property entry would become a
  data/accessor enum with a Rust getter/setter variant ahead of
  clj-quickjs-120, which designs that enum for JavaScript accessors and
  their resumable calls. Two designs of one entry would conflict; the walk
  hook was temporary; clj-quickjs-261 replaces it with the generic accessor
  path and built-in getter and setter functions.
- *Lowering `__proto__: p` to a call of `Object.setPrototypeOf`*: needs
  the realm's function on the stack and has the wrong semantics (a
  non-object value must be ignored, not a `TypeError`). A one-byte opcode
  matches QuickJS and allocates nothing, so it is not a poll point.
- *Allowing only ordinary objects as prototypes*: ECMAScript accepts any
  object; the runtime's other values already behave as objects without
  properties, so they end a chain like `null` does, and functions and
  error objects fail explicitly until their properties exist.

**Consequences.**

- clj-quickjs-120 adds the accessor storage; clj-quickjs-261 stores
  `__proto__` as an accessor with built-in getter and setter and removes
  `is_proto_accessor`, `reaches_proto_accessor` and `Chain::ProtoSetter`.
- clj-quickjs-122 (`preventExtensions`) only has to clear the flag:
  `set_prototype_of` already rejects a non-extensible target.
- `Object.create`'s second argument (`Object.defineProperties`) fails with
  `UnsupportedOperand`; it needs own enumerable keys (clj-quickjs-123).
- `Function.prototype` (clj-quickjs-102) gives functions a prototype to
  report; changing it is left: the cycle walk must then follow functions
  (and their records) whose prototype can change.

## Extensibility and freezing (clj-quickjs-122)

Contract: `src/vm/integrity.rs` (the four built-ins and the table of what
each kind reports), `src/object.rs` (`OrdinaryObject::prevent_extensions`,
`freeze`, `is_frozen`), `src/object/array.rs` (`ArrayObject::freeze`,
`is_frozen`, the `frozen` flag), `src/vm/function.rs`
(`integrity_store`, `integrity_target`). Semantics: `docs/grammar.md`,
"Execution".

**Decision.** The `[[Extensible]]` flag every ordinary object has carried
since clj-quickjs-114 is cleared by `Object.preventExtensions` and
`Object.freeze`, and never set again. Everything that adds a property
already consulted it (`OrdinaryObject::set`/`add`/`define_own_property`,
`ArrayObject::add`/`define_element`, `prototype::set_prototype_of`), so the
strict `TypeError` / sloppy no-op of `[[Set]]` (clj-quickjs-119), the
`defineProperty` `TypeError` and the `setPrototypeOf` `TypeError`
(clj-quickjs-121) needed no new code. `Object.freeze` is ECMAScript's
SetIntegrityLevel(frozen) applied to the stored attributes: every entry
of an ordinary object becomes non-writable and non-configurable;
`isFrozen` is TestIntegrityLevel over the same entries. The four are
`Builtin` variants appended to the realm's `Object`; none runs JavaScript,
so they run inside their call instruction.

- **Frozen arrays need no per-element entries.** An array freezes its
  ordinary part, makes `length` read-only and sets a `frozen` flag that
  changes the default element attributes from all-true to
  non-writable, enumerable, non-configurable; the sparse map of element
  attributes keeps only the elements that differ from that default (those
  `defineProperty` made non-enumerable). Freezing an array of a million
  elements allocates nothing. The flag is never cleared: a frozen array
  takes no new elements and its elements cannot be reconfigured. Every
  write path (index, hole, past the end, `length`, named) already
  rejects read-only and non-extensible cases.
- **Functions keep the flag in their property record** (clj-quickjs-125).
  A function without a record is extensible and not frozen. Making one
  non-extensible or frozen creates the record and, for a constructor,
  its lazy `prototype` first, so the own property exists before the
  record stops taking new ones (otherwise its first read would fail).
  The record is task-owned, so the function can no longer be published,
  like after any property write. A published function is shared and
  immutable, so it reports non-extensible and frozen, and freezing it
  again succeeds without change; publication racing the change resolves
  under the object-table lock as for writes (`Heap::mark_shareable_unless`),
  and a change that lost the race reports the now-frozen function.
- **Intrinsics report their attributes.** A realm intrinsic is immutable
  because it is shared, not because of its attributes ("Realm
  intrinsics"): `isExtensible(Object.prototype)` is `true` and
  `isFrozen` `false`, as `getOwnPropertyDescriptor` reports writable,
  configurable methods, while `preventExtensions` and `freeze` on one are
  the `TypeError` of a `[[PreventExtensions]]` that returns false. The
  realm's lock-free views stay exact.
- **Values without JavaScript properties** (vectors, maps, occurrences,
  builders, atoms, cells, non-error natives) already behave as objects
  with no properties and no extensibility (`crate::vm::property`), so they
  report non-extensible and frozen, and the two changes return them.
  Error instances follow ordinary object integrity rules (clj-quickjs-230).
- **Freezing does not change ownership.** A frozen object or array is
  still `Shareability::TaskOwned`: other tasks get `ForeignObject` and the
  publication check rejects it, however deeply it is frozen. The pellet
  does not ask for frozen objects to become shareable, and doing so would
  need a publication rule for objects (identity, prototype links to
  task-owned objects, the owner's later reads), which is a separate design.

**Alternatives considered.**

- *Per-element attribute entries for frozen arrays*: correct but
  proportional to the array; the flag costs one byte and keeps
  `defineProperty`'s exceptions map small.
- *A non-extensible bit in the function object itself* (no record): would
  keep an arrow function publishable after `Object.freeze`, but makes the
  function object mutable after allocation, which the publication cache
  relies on not happening, and a constructor needs its `prototype`
  materialized (a task-owned object) anyway. Rejected; the record is the
  single place a function's mutable property state lives.
- *Reporting intrinsics as frozen*: would contradict the attributes
  `getOwnPropertyDescriptor` reports for them (a frozen object cannot have
  a writable property). Rejected.

**Consequences.**

- clj-quickjs-120: freezing an accessor property only makes it
  non-configurable (accessors have no `writable`), and `isFrozen` must
  skip `writable` for them.
- clj-quickjs-124 (`delete`) must reject deleting the non-configurable
  properties a frozen object has; nothing else is needed for
  non-extensible objects, whose properties stay deletable (done).
- clj-quickjs-101 includes `length`/`name` in a frozen function's record;
  published functions report virtual metadata as non-configurable.
- `Object.seal`/`isSealed` (not requested) would be the same two loops
  with `writable` left alone.
## Own-property keys (clj-quickjs-123)

Contract: `src/vm/keys.rs` (`own_property_keys`, `KeyFilter`, `OwnKey`,
the `Object.keys` / `Object.getOwnPropertyNames` built-ins),
`src/object/array.rs` (`ArrayObject::element_keys`), `src/vm/function.rs`
(`property_record`). Semantics: `docs/grammar.md`, "Execution".

**Decision.** Own-key order is computed when keys are enumerated, not kept
by the storage. `own_property_keys` reads the target under one
object-table lock, collecting array elements (already ascending: dense
part, then sparse part), an array's `length`, and the interned keys of the
ordinary store in creation order, each with its `enumerable` attribute.
After the lock is released it names the interned keys in the property-key
table, moves every name that is an array index (`array_index`) into the
index list, sorts that list, and emits indices first, then the other
names: ECMAScript OrdinaryOwnPropertyKeys (QuickJS
`JS_GetOwnPropertyNamesInternal` and `num_keys_cmp` count and sort the
same way). Symbols follow the string keys in creation order
(clj-quickjs-127); both built-ins filter them out. `for`-`in`
(clj-quickjs-85, -86) and object spread/rest copies are expected to call
`own_property_keys` with `KeyFilter::Enumerable` rather than walk storage
themselves.

`Object.keys` and `Object.getOwnPropertyNames` are `Builtin` variants
appended to the realm's `Object` (nineteen intrinsics). Neither runs
JavaScript, so both run inside their call instruction and allocate their
result, a fresh array owned by the running task, last.

**Alternatives considered.**

- *Keep ordinary objects' entries sorted (indices first) on insertion*:
  enumeration becomes a plain scan, but every insert of an index name
  shifts entries and rebuilds the key index, and the creation order of the
  remaining names must still be kept. Rejected: enumeration is rarer than
  insertion, and an object with many index-named properties is usually an
  array, whose elements are already in order.
- *Cache each interned key's array-index value in the key table*: saves
  parsing names on every enumeration but grows every key entry for a rare
  case. Rejected for now; `array_index` is a bounded scan of at most ten
  code units.

**Consequences.**

- `Object.getOwnPropertyNames` of a function includes metadata and its
  constructor's lazy `prototype` (clj-quickjs-101); of `Object.prototype`,
  its stored `__proto__` accessor (clj-quickjs-261). `Object.keys` of either
  lists no non-enumerable properties. Error instances use ordinary own
  property storage (clj-quickjs-230).
- The `delete` operator (clj-quickjs-124) only has to remove the entry
  (and fix up the key index): a re-added key is pushed at the end, so it
  then enumerates last (done).
- `Object.create`'s second argument and `Object.defineProperties`, `Object.assign`,
  `Object.entries`/`values` can use the same helper.

## `instanceof` (clj-quickjs-75)

Contract: `src/vm/prototype.rs` (`instance_of`), `src/vm/instr.rs`
(`instanceof`, `0x60`). Semantics: `docs/grammar.md`, "Execution".

**Decision.** `instanceof` is a binary operator at relational precedence
(`BinaryOp::Instanceof`), lowered by the existing out-of-line
`binary_expr` helper to one appended opcode, as QuickJS's
`OP_instanceof`. The opcode is InstanceofOperator without the
`@@hasInstance` lookup (protocol dispatch deferred), then OrdinaryHasInstance. Bound
functions delegate to their targets (clj-quickjs-103); otherwise the right operand must be a callable
object, a primitive left operand is `false`, `F.prototype` is read with
the ordinary property walk (a built-in method such as `Object.is`
inherits no `prototype` from `Function.prototype` or `Object.prototype`,
so it is `undefined`) and must be an object, and the chain is walked
with `[[GetPrototypeOf]]` (`prototype::get_prototype_of`), so its owner
rules apply, a function's chain runs through `Function.prototype`
(clj-quickjs-102: `f instanceof Function`, `f instanceof Object`), and an
Error instance follows its ordinary prototype chain (clj-quickjs-230). The walk only reads; chains are acyclic,
so it ends. Reading `prototype` may create a function's lazy prototype
object (and build the realm), so the opcode is a poll point and keeps
both operands on the operand stack, traced, until it has the answer. It
calls no JavaScript (no accessors yet), so nothing needs to be resumable
inside it; once `@@hasInstance` or a `prototype` getter can run
JavaScript, the instruction must become a call into a frame like
`construct`.

## The `delete` operator (clj-quickjs-124)

Contract: `src/vm/property.rs` (`delete`, `delete_own`), `src/object.rs`
(`OrdinaryObject::delete`, `DeleteOutcome`), `src/object/array.rs`
(`ArrayObject::delete_element`), `src/vm/function.rs` (`delete_target`),
`src/vm/instr.rs` (`delete`, `0x61`). Semantics: `docs/grammar.md`,
"Execution".

**Decision.** `delete` is a prefix operator with the precedence of `!`
(`ExprKind::Delete`, which keeps the `delete` token so a parenthesized
expression still reports there) whose operand the parser checks: an
identifier, including the reserved constants `undefined`/`NaN`/`Infinity`
that parse as literals (recognized by the token the parser last turned
into one, not by spelling), is an early `Invalid` in strict code and
deletes global properties in sloppy code (clj-quickjs-105). The
lowering pushes the base and the key (for `o.p` the name as a string
constant) and emits one appended opcode, as QuickJS's `OP_delete`; the
opcode does the `undefined`/`null` check, ToPropertyKey and `[[Delete]]`
in that order, so it needs neither `to_property_key` (whose message names
a read) nor a second, named form. `[[Delete]]` follows the module table
of `crate::vm::property` by kind:

- an ordinary object removes the entry from its creation-order vector
  (`Vec::remove`) and shifts the key index's later positions down, so the
  other properties keep their order and a re-added key comes last
  (clj-quickjs-123 enumerates in that order). Extensibility does not
  matter; a non-configurable property (frozen objects included) stays;
- an array element becomes a hole (`None` in the dense part, no sparse
  entry), dropping its attribute entry, so the dense/sparse invariant of
  "Array objects" holds and `length` is unchanged; `length` is not
  configurable;
- a function's own properties are in its property record
  ("Function own properties"): deleting metadata creates the record first; a
  constructor's `prototype` is non-configurable whether or not it exists
  yet, so a function stays publishable after deleting an absent property;
- a realm intrinsic or built-in function is shared and immutable, so a
  property it has is a `TypeError` in both modes (the same sharing rule as
  writes); a property it lacks is simply absent;
- a primitive base is treated as its wrapper object without allocating
  one: a string's indices below its length and its `length` are
  non-configurable, nothing else is own.

The opcode runs no JavaScript. Deleting initial function metadata can
allocate its record and prototype (clj-quickjs-101); deleting an absent
property allocates nothing. `Delete` polls for collection before execution,
while its operands are still rooted in the task's VM stack.

**Alternatives.** *`delete_field` and `delete_elem` opcodes* mirroring
`get_field`/`get_elem`: two registry entries for a rare operator whose
named form is one constant push. *Reusing `Lowerer::property_reference`*
(clj-quickjs-76): it converts the key with `to_property_key`, whose
`undefined`/`null` message is a read's. *A tombstone instead of
`Vec::remove`*: constant-time removal, but every read, enumeration and
`freeze` would skip tombstones, and objects are small; a deletion-heavy
dictionary use would want shapes or a compacting map later.

**Consequences.** Deleting is linear in the object's own-property count.
Sloppy `delete` of a non-configurable property is `false` (the running
code's `Code::strict`, as for writes). `delete super.x` (clj-quickjs-138)
and `delete a?.b` (clj-quickjs-134) wait for those forms; `delete` of an
identifier is clj-quickjs-105; `delete f.length`/`f.name` use the ordinary
record after copying initial metadata (clj-quickjs-101).

## `in` (clj-quickjs-74)

Contract: `src/vm/property.rs` (`in_operator`, `has_property`),
`src/vm/instr.rs` (`in`, `0x62`). Semantics: `docs/grammar.md`,
"Execution".

**Decision.** `in` is a binary operator at relational precedence
(`BinaryOp::In`), lowered by `binary_expr` to one appended opcode, as
QuickJS's `OP_in`. The opcode checks that the right operand is an object
(a catchable TypeError otherwise, before the key is converted, as
`js_operator_in`), converts the key with the existing
`property::to_property_key` and asks HasProperty. HasProperty is the read
walk of `property::find` with one difference: reaching
`Object.prototype.__proto__` answers true without running the accessor's
getter (whose `[[GetPrototypeOf]]` of the receiver would otherwise fail
on a function). Sharing the walk keeps one set of owner and kind rules for
reads, so the kinds without JavaScript properties answer false and error
objects and the names `Function.prototype` lacks so far stay explicit
`UnsupportedOperand` (its `call` and `apply` are found, clj-quickjs-102).
A constructor's lazy `prototype` is reported present without being
created (`function::probe`; QuickJS `JS_HasProperty` leaves autoinit
properties alone), so asking attaches no task-owned state and works on a
published function. Continuing from a function to `Object.prototype` may
build the realm, so the opcode is a poll point and keeps both operands on
the operand stack until it has the answer. It calls no JavaScript (no getters, no proxies, no ToPrimitive of
an object key yet); once a `has` trap or ToPrimitive can run, the
instruction must become a call into a frame like `construct`.

The parser gained ECMAScript's `[~In]` parameter as one `no_in` flag: set
while the init of a `for` head is parsed, cleared by every bracketing
construct that the grammar parses with `[+In]` (parentheses, brackets,
braces, template substitutions, arguments, the middle of `? :`, function
parameters and bodies), and consulted only by the binary-operator loop.
An `in` after any init then starts a for-in head (clj-quickjs-85, "`for`-`in`"
below).

## `for`-`in` (clj-quickjs-85)

Contract: `src/vm/keys.rs` (`ForInIterator`, `for_in_start`,
`for_in_next`; module docs, "`for`-`in`"), `src/vm/instr.rs`
(`for_in_start`, `for_in_next`, `0x63`–`0x64`), `src/frontend/lower.rs`
(`Lowerer::for_in_statement`), `src/frontend/parser.rs`
(`parse_for_head`, `Parser::no_in`). Semantics: `docs/grammar.md`,
"Execution".

**Decision.** The enumerator is a `HeapObject::Native` payload, not a new
heap kind: a snapshot of the own enumerable string keys (as
`Object.keys` lists them, `own_property_keys`), a position, and the
object, whose only heap edge its `Trace` reports. It is task-owned
(`Shareability::TaskOwned`) and held only in a scratch local of the loop's
frame, never on the operand stack across the body, so the frame's tracing
covers it, yields, dependency waits and migration carry it with the frame,
no Rust state survives between iterations, and `break`, `continue`,
`return` and exceptions leave the loop without popping anything (the
validator's operand-stack invariant for jumps holds unchanged). JavaScript
can never reach it, so it is never published. `for_in_next` re-checks
each key with `[[GetOwnProperty]]` before returning it and skips one that
is gone or no longer enumerable (ECMAScript's informative
EnumerateObjectProperties; QuickJS `js_for_in_next` checks existence only),
taking the key under the object lock and looking it up after releasing
it. A `let`/`const` head's binding gets a fresh binding cell each
iteration when it is kept in a cell (captured and assigned, or captured in
the object expression while in its temporal dead zone), otherwise its
local is re-initialized; the object expression sees it uninitialized, so
the static temporal-dead-zone decision still holds. The parser tells the
heads apart with the `[~In]` flag of clj-quickjs-74 (`no_in`), set for an
expression head and for each initializer of a declaration head; a
`let`/`const`/`var` head is a for-in head when `in` follows its first
name. A `var` head (clj-quickjs-89) names a `var` binding of the function
(collected by `var_names_in`, so it is hoisted) and is assigned each key
as an identifier head is; it adds nothing to the loop's own scope.
The prototype-chain walk (clj-quickjs-86) keeps the same payload: the
snapshot now holds every own string key with its `enumerable` attribute,
and when it is used up the enumerator moves to the prototype (lazily, as
QuickJS `js_for_in_next` does), adding the left object's names to a
visited set (two `HashSet`s, of indices and of names, compared by
content) that hides inherited keys; its only edge becomes the object it is
on. A walk that reaches a realm intrinsic ends there when no intrinsic on
the chain has an enumerable property (always, today), decided from the
realm's immutable views (clj-quickjs-117) without the object-table or
key-table lock, so enumerating `{...}` never contends on
`Object.prototype`. QuickJS's eager fast-path check of the whole chain
(`js_for_in_prepare_prototype_chain_enum`) is not copied: it would read
prototypes before they are reached. Details: `src/vm/keys.rs`, "Prototype
chain".
Deferred: property targets (clj-quickjs-87), the non-strict
`for (var k = 1 in o)` (clj-quickjs-90), Proxy traps (clj-quickjs-91).
## Forwarding calls: `call`, `apply`, `Reflect.apply` (clj-quickjs-102)

Contract: `src/vm/builtin.rs` ("Forwarding built-ins", `forward`) and
`src/vm/dispatch.rs` (`resolve`).

**Decision: a tail call, not a native frame.** `Function.prototype.call`,
`Function.prototype.apply` and `Reflect.apply` return their target's
result unchanged, so nothing is left to do after the target returns. The
`Call`/`CallMethod` instruction (or a resumable native's callback step)
computes the forwarded call (target, receiver, argument list; building
the list reads properties but runs no JavaScript), replaces the
forwarding call's operands with it and enters the target's task-owned
frame exactly as a direct call, or runs a non-forwarding built-in to
completion. Chains of forwarding built-ins are followed in the same
instruction, bounded by the call-depth limit (`StackOverflow`).

- Considered: a resumable native frame per forwarding call
  (`crate::vm::native`, the `__rt.reduce` pattern). It would work, but it
  adds a task-owned frame, a step and an operand area for no state, and
  a failure would unwind through it for nothing. The tail call keeps the
  invariant that matters (no Rust frame, the callee may yield, wait,
  be collected around and migrate) with less state.
- Consequences: the forwarding call does not appear in a failure's call
  chain (the call site is the `.call(` instruction); its `this` is the
  receiver passed, so a sloppy callee's substitution (global object,
  wrappers) stays the explicit gap of `this`. A later forwarding built-in
  (`bind`'s bound function, clj-quickjs-103) follows the same path.

## Primitive bases (clj-quickjs-73)

Contract: `src/vm/property.rs` (module docs, "Primitive bases";
`primitive_prototype`, `put_primitive`), `src/realm.rs`
(`Intrinsics::primitive_prototype`, `Number.prototype`,
`String.prototype`), `src/vm/builtin.rs` (`this_number_value`,
`this_string_value`). Semantics: `docs/grammar.md`, "Execution".

**Decision.** A property read on a number or string starts the ordinary
prototype walk at `Number.prototype` or `String.prototype` instead of
allocating the wrapper object ECMAScript's ToObject describes, as QuickJS's
`JS_GetPrototypePrimitive` does. A wrapper is never observable through a
read: a Number wrapper has no own properties, and a String wrapper's own
properties (`length`, indices) are answered before the walk (see "String
own properties" below). A
method call needs no new mechanism: `call_method` already passes the base
value as the receiver, so `(1).toString()` hands the number itself to the
built-in as `this`, which is the strict-mode receiver; there is no
sloppy-mode function that could observe a wrapped `this`, since the
prototypes are immutable realm intrinsics. Writes need no prototype at
all: the receiver is a primitive, so OrdinarySet returns false for every
data property (a strict-mode TypeError), and only `Object.prototype`'s
`__proto__` setter, which ignores a primitive `this`, succeeds; booleans
share that rule now, while boolean reads wait for `Boolean.prototype`.

Consequences: no allocation and no task-owned state per access (tested by
an unchanged live-object count), the same functions on every task
(`(1).toString === Number.prototype.toString` everywhere), and the realm is
built by the first primitive read, so `GetField`/`GetElem` stay poll points.
The prototypes are ordinary intrinsics inheriting from `Object.prototype`
with `[[NumberData]]` / `[[StringData]]` holding `+0` / `""`
(`String.prototype` has its own `length` 0), added by clj-quickjs-72.
`Number` and `String` are built-in globals (operands 1 and 2 of
`get_builtin_global`); their constructors and current conversion limitations
are described in "Number and String wrapper data" below.

## String own properties (clj-quickjs-142)

Contract: `src/vm/property.rs` (`string_own_property`, module docs,
"Primitive bases"), `src/value/string.rs` (`JsString::code_unit_at`),
`src/vm/descriptor.rs`, `src/vm/keys.rs` (`string_own_keys`). Semantics:
`docs/grammar.md`, "Execution" (`"abc".length`, `"abc"[i]`).

**Decision.** A string primitive answers its String wrapper's own
properties itself, with no wrapper and no per-string property store:
`string_own_property` computes the property from the `JsString` and the
key (ECMAScript StringGetOwnProperty; QuickJS
`js_string_get_own_property`). `length` is the stored code-unit count and
an index key, already an integer (`Key::Index`, so `s[1]` builds no key
string), reads the one stored unit and wraps it in a new one-unit
`JsString`; both are O(1) and nothing rescans the string. The attributes
are fixed (indices read-only, enumerable, non-configurable; `length`
read-only, non-enumerable, non-configurable), so the same function answers
`[[Get]]`, `Object.getOwnPropertyDescriptor`, `[[Set]]` (a read-only own
property: strict-mode TypeError), `[[Delete]]` (non-configurable) and,
through `string_own_keys`, `Object.keys` / `Object.getOwnPropertyNames`
(indices ascending, then `length`, the OrdinaryOwnPropertyKeys order of
clj-quickjs-123). A one-unit result is a fresh `Arc` string rather than an
entry of a cached single-character table; it is not a heap object, so the
reads allocate nothing the collector sees (tested by an unchanged
live-object count).

String wrappers (`new String(v)`, clj-quickjs-72) keep virtual properties
over their immutable `[[StringData]]` in the ordinary store. Their reads,
descriptors, assignments, inherited writes and deletes reuse
`string_own_property`. Compatible definitions validate against the fixed
property through `validate_and_apply` without adding stored entries, even
on non-extensible wrappers. `string_own_keys` supplies virtual indices
and length to the existing key sorter alongside ordinary entries: all
indices ascending, then `length`, then other names by creation order.
String.prototype already stores its empty-string length and is excluded
from virtual key synthesis. Ownership, tracing and integrity checks remain
those of ordinary objects; virtual properties introduce no heap edges.

## QuickJS references and provenance

Consulted (Bellard upstream `https://github.com/bellard/quickjs.git`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `VERSION` 2026-06-04), as design
references only:

- `research/quickjs/quickjs.c`: `JSAsyncFunctionState` and the
  `JS_CallInternal` generator-resume branch (saved `cur_pc`, `cur_sp`, argument
  and variable buffers) for what a saved continuation must contain;
  `js_poll_interrupts` (a decrementing per-context counter) as the analogue of
  the instruction budget. QuickJS keeps frames on the C stack and resumes
  generators through promises; neither is used here.
- `research/quickjs/doc/quickjs.texi`, "Function call": QuickJS frames live on
  the system stack, which is the property this design replaces with
  task-owned frames.
- `research/quickjs/quickjs.c` `JS_MarkValue` / `mark_children` and
  `research/quickjs/quickjs.h` `JSClassGCMark`: edge inventory for the
  collector (object edges, bytecode constants, class mark hooks). QuickJS's
  reference counting, cycle removal and running-stack exemption are not used;
  `research/quickjs/doc/quickjs.texi`, "Garbage collection", for the contrast.
- `research/quickjs/quickjs.h` `JSClassDef.gc_mark` versus
  `JSClassExoticMethods`: native edge tracing versus user-extensible property
  hooks. Persistent collections report their edges like a `gc_mark` hook but
  route no private bookkeeping (lookup, hashing, equality) through
  user-extensible hooks. QuickJS has no persistent collections; none of its
  code is used for them.

- `research/quickjs/quickjs.c` (clj-quickjs-22): `compute_stack_size` for the
  validator's stack-depth walk (already written fresh in clj-quickjs-1), the
  `throw_error` opcode emitted for constant assignment, and the
  `eval_ret_idx` completion-value local. No code was copied.
- `research/quickjs/quickjs.c` (clj-quickjs-4): `JSStackFrame` and
  `JS_CallInternal` for call/return semantics and argument binding (missing
  arguments `undefined`, extra ones ignored), `JS_ThrowStackOverflow` for the
  depth failure, and `special_object` with `OP_SPECIAL_OBJECT_THIS_FUNC` for
  `current_function`. Frames here are task-owned indices, not C-stack
  pointers. No code was copied.
- `research/quickjs/quickjs.c` `js_strict_eq` and `js_same_value`
  (clj-quickjs-10): JavaScript `===` versus `Object.is`; atom CAS uses the
  SameValue rule on raw values, not `===`. `JS_DupValue`/`JS_FreeValue`
  (`quickjs.h`) are reference-count lifetimes, replaced here by rooting and
  collector grace periods. `quickjs-atom.h` atoms are interned names,
  unrelated to these atomic references. No code was copied.
- `research/quickjs/quickjs.c` `js_call_c_function` (clj-quickjs-12): the
  synchronous C-stack call of a native, which reenters `JS_CallInternal` for
  callbacks; resumable natives replace that recursion with task-owned native
  frames. `JSAsyncFunctionState` / `async_func_resume` as the inventory of
  state a pending native keeps; `quickjs.h` `JSCFunction` and `JSClassGCMark`
  for the call and edge boundaries. No promise semantics, ABI or code was
  used.

No QuickJS source or encodings were copied. Adapted material keeps the notice
from `research/quickjs/LICENSE` (MIT, Copyright (c) 2017-2021 Fabrice Bellard
and Charlie Gordon) and is listed here with its source path and revision:

- `tests/fixtures/js/quickjs_op1_while.js` (clj-quickjs-22): the supported
  cases of `test_op1` in `research/quickjs/tests/test_language.js` and
  `test_while` in `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8.
  Modified: rewritten for the subset (`let` for `var`, `x = x + 1` for `++`,
  failure flags instead of `assert()` calls); the bitwise, `**` and string
  cases are omitted. The file carries the full notice.
- `tests/calls.rs` (clj-quickjs-4): `fib1` from `test_closure3` and
  `myfunc1`/`myfunc2` from `test_closure2` in
  `research/quickjs/tests/test_closure.js`, revision 7d1c13c5d8f8, as
  hand-assembled bytecode (early VM tests). Modified: `fib1`'s self-reference
  is `current_function`; `myfunc2`'s capture of `myfunc1` is passed as an
  argument (closures are clj-quickjs-20); `assert()` calls are Rust
  assertions. The file carries the full notice.
- `tests/closures.rs` (clj-quickjs-20): `f` ('closure1') and `test_closure1`
  ('closure2') in `research/quickjs/tests/test_closure.js`, revision
  7d1c13c5d8f8, as hand-assembled bytecode (early VM tests). Modified: `log`
  string concatenation is a host effect per logged variable; `var x` is
  `const x` (captured by value); the `{ set, get }` object is a vector;
  `assert()` calls are Rust assertions. The file carries the full notice.
- `tests/fixtures/js/quickjs_closure.js` (clj-quickjs-21): the top-level
  `f`/`g1` case ('closure1'), `test_closure1`, `test_closure2` and
  `test_closure3` of `research/quickjs/tests/test_closure.js`, revision
  7d1c13c5d8f8, as JavaScript source compiled by this frontend. Modified:
  upstream's `var`s are kept since clj-quickjs-94 (`let`/`const` before),
  script-level ones are now global object properties (clj-quickjs-104);
  the added `failed` flag is a `let`; `log` appends two decimal digits per value to a
  number instead of concatenating strings; the `{ set, get }` object is a
  vector made with `__rt.vector` and read with `__rt.nth`; failure flags
  instead of `assert()` calls. `test_arrow_function`, `test_with`,
  `test_eval_closure` and `test_eval_const` are omitted (outside the
  subset). The file carries the full notice.
- `research/quickjs/quickjs.c` (clj-quickjs-21), consulted only:
  `js_parse_function_decl` (scoping of a function's name and its
  self-binding), `get_closure_var`/`add_closure_var` (resolving a name
  through enclosing functions and threading captures down) and
  `instantiate_hoisted_definitions` (function declarations created on scope
  entry). No code was copied.
- `tests/fixtures/js/quickjs_try_catch.js` (clj-quickjs-11, re-adapted
  with upstream strings in clj-quickjs-227): the `assert` function and
  `test_try_catch1`–`test_try_catch8` of
  `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8, called once
  each in upstream order, as JavaScript source compiled by this frontend;
  the script completes with `"ok"`. The string literals, thrown strings,
  `for(;;)` and `assert(actual, expected, message)` calls are upstream's.
  Modified: `let` for `var` (until clj-quickjs-94); `assert` tests
  `expected === undefined` instead of `arguments.length == 1` (until
  clj-quickjs-97), throws its message string instead of `Error(...)` (until
  clj-quickjs-228) and drops
  the unreached object/`toString()` comparison (until clj-quickjs-69 and
  clj-quickjs-70); `test_try_catch8` iterates `"x"`, `"y"` with a counted
  `for` instead of `for (var i in {x:1, y:2})` (until clj-quickjs-85,
  clj-quickjs-89 and clj-quickjs-114). The file carries the full notice.
- `tests/exceptions.rs` `break_and_continue_run_finally_blocks`
  (clj-quickjs-79): the body of `test_try_catch5` in
  `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8. Modified:
  `let` for `var`, `while (true)` for `for(;;)`, and logged letters are
  digits appended to a number. The test carries the notice. Consulted
  only: `emit_break` in `research/quickjs/quickjs.c` (its `OP_gosub` to
  each enclosing finally block and the dropped return addresses of finally
  blocks a jump leaves). No code was copied.
- `research/quickjs/quickjs.c` (clj-quickjs-11), consulted only:
  `js_parse_statement`'s `TOK_TRY` branch (catch binding scope, optional
  catch binding), the `OP_throw`, `OP_catch`, `OP_gosub` and `OP_ret`
  handlers and the `exception:` unwinding label of `JS_CallInternal`, and
  `current_exception_is_uncatchable` (uncatchable errors skip catch and
  finally). Handlers here are typed entries of a task-owned handler stack
  and a finally block's reason is an explicit completion record, not a
  catch offset or `gosub` return address on the C value stack. No code was
  copied.
- `src/frontend/tests.rs` `hashbang_comment_only_at_start` (clj-quickjs-49):
  the hashbang line `#! (shebang test)` of
  `research/quickjs/tests/test_std.js` (line 1), revision 7d1c13c5d8f8, as
  JavaScript source parsed by this frontend. Modified: followed by `x` to
  check the next line's position. The test carries the full notice.
  `research/quickjs/quickjs.c` `skip_shebang` was consulted only (where a
  hashbang may start and which characters end it); no code was copied.
- `src/frontend/tests.rs` `arrow_functions_and_asi` and
  `tests/js_functions.rs` `arrow_function_values` (clj-quickjs-53): the
  scripts `() => {}\n() => {}`, `() => {}\n+1` and `x => {}\n() => {}` of
  `test_parse_arrow_function` in `research/quickjs/tests/test_language.js`,
  revision 7d1c13c5d8f8. Modified: checked as parse trees and as script
  completion values instead of through `eval` and `typeof`; the `async`
  arrow cases are rejected as unsupported. Each test carries the notice.
  Consulted only, in `research/quickjs/quickjs.c`:
  `js_parse_skip_parens_token`, `js_parse_assign_expr2` and
  `js_parse_function_decl2` (arrow detection and bodies). No code was
  copied.
- `tests/js_source.rs` `identifiers_are_not_normalized` (clj-quickjs-50):
  `test_unicode_ident` of `research/quickjs/tests/test_language.js` (line
  634), revision 7d1c13c5d8f8, as JavaScript source compiled by this
  frontend. Modified: `let` for `var`; `typeof õ` evaluates to `"undefined"`
  (since clj-quickjs-69), and a plain reference to `õ` must fail with a
  ReferenceError; the declared name is also read back. The test carries the full notice; the name `Ãµ` also
  appears in `src/frontend/tests.rs` `unicode_identifiers`.
  `research/quickjs/quickjs.c` `next_token` (`lre_js_is_ident_first` /
  `lre_js_is_ident_next`) and `ident_realloc` were consulted only; the
  identifier tables now come from the pinned QuickJS tables, verified against
  Unicode 17.0.0 (clj-quickjs-191), replacing the initial Perl Unicode 16 data.
- `tests/js_source.rs` `bitwise_operators_follow_to_int32` (clj-quickjs-62):
  the bitwise and shift cases of `test_op1` (line 39) and the numeric cases
  of `test_cvt` (line 108) of `research/quickjs/tests/test_language.js`,
  revision 7d1c13c5d8f8, as JavaScript source compiled by this frontend.
  Modified: each `assert(r, v)` is a Rust assertion on the expression's
  value; the string operand cases of `test_cvt` are in
  `strings_convert_with_string_to_number` (clj-quickjs-67, below). The
  test carries the full notice.
  `research/quickjs/quickjs.c` `js_parse_expr_binary`,
  `js_binary_logic_slow`, `JS_ToInt32Free`, the `OP_shr` / `OP_sar` cases
  of `JS_CallInternal` and `js_unary_arith_slow` were consulted only. No
  code was copied.
- `src/frontend/tests.rs` `break_and_continue` and `tests/js_source.rs`
  `while_break_and_continue` (clj-quickjs-78): the first statement of
  `test_labels2` in `research/quickjs/tests/test_language.js` (`while (1)
  label: break` followed by a line break) and the body of `test_while_break`
  in `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8. Modified:
  the label is dropped (the labelled form is in `labelled_statements`) and
  the next line is `x`, checked as a parse tree; `let` for `var`, `x = x + 1`
  for `x++`, and the asserted condition is the script's completion value.
  Each test carries the notice. Consulted only, in
  `research/quickjs/quickjs.c`: the `TOK_BREAK`/`TOK_CONTINUE` case of
  `js_parse_statement_or_decl`, `emit_break`, `push_break_entry` and
  `close_scopes` (which enclosing loops and handlers a jump leaves, and the
  early errors). Here the compiler pops catch handlers of the task-owned
  handler stack with `pop_catch` before the jump. No code was copied.
- `tests/js_source.rs` `exponent_operator` (clj-quickjs-63): the `2 ** 8`
  case of `test_op1` (line 39) of `research/quickjs/tests/test_language.js`,
  revision 7d1c13c5d8f8, as JavaScript source compiled by this frontend.
  Modified: `assert(r, v)` is a Rust assertion on the expression's value.
  The test carries the full notice. `research/quickjs/quickjs.c`
  `js_parse_unary` (`PF_POW_FORBIDDEN`), `js_pow` and the `OP_pow` case of
  `JS_CallInternal` were consulted only. No code was copied.
- `tests/js_source.rs` `update_expressions_on_bindings` (clj-quickjs-64):
  the four identifier groups of `test_inc_dec` (line 141) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: `var a, r` is
  `let a, r`, and each `assert(cond, true)` is the script's value checked
  by a Rust assertion. The test carries the full notice.
  `research/quickjs/quickjs.c` `js_parse_unary` (`TOK_INC`/`TOK_DEC`),
  `js_parse_postfix_expr`, `OP_inc` / `OP_post_inc` and `js_post_inc_slow`
  were consulted only. No code was copied.
- `tests/js_source.rs` `update_expressions_on_properties` (clj-quickjs-76):
  the seven member groups of `test_inc_dec` (line 141) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: `a = {x:true}` is
  `let a = {x: true}` (and `var a, r` is `let a, r`), and each
  `assert(v, expected)` is the script's value checked by a Rust assertion.
  The test carries the full notice. `research/quickjs/quickjs.c`
  `js_parse_postfix_expr`, `js_parse_unary` (lvalue handling, `to_propkey2`,
  `dup2`, `insert3`), `OP_post_inc` and `js_post_inc_slow` were consulted
  only. No code was copied.
- `src/frontend/tests.rs` `number_literal_member_access` (clj-quickjs-48):
  the six cases of `test_number_literals` (line 667) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
  (`0.1.a`, `0x1.a`, `0b1.a`, `01.a`, `0o1.a` and `eval('0.a')`).
  Modified: each case is checked as a parse tree or a lexer rejection
  instead of evaluated (`undefined`, or a SyntaxError for `0.a`), and
  `01.a` is checked as sloppy code, where it is the legacy octal integer 1
  then a member access, and as strict code, where it is rejected
  (clj-quickjs-51). The test carries the full notice. No code was copied.
  `research/quickjs/quickjs.c` `next_token` (number branch) and `js_atof`
  (`ATOD_ACCEPT_LEGACY_OCTAL`) were consulted for clj-quickjs-51's legacy
  octal lexing only.
- `tests/js_source.rs` `loose_equality_with_a_boolean_left_operand`
  (clj-quickjs-60): the `null == undefined`, `undefined == null`,
  `true == 1` and `0 == false` cases of `test_eq` (line 126) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: each
  `assert(cond)` is a Rust assertion on the expression's value; the string
  and wrapper-object cases are omitted (they need string values and
  ToPrimitive). The test carries the full notice.
  `research/quickjs/quickjs.c` `js_eq_slow` was consulted only. No code
  was copied.
- `tests/js_source.rs` `do_while_loops` (clj-quickjs-80): the body of
  `test_do_while` (line 49) of `research/quickjs/tests/test_loop.js`,
  revision 7d1c13c5d8f8, as JavaScript source compiled by this frontend.
  Modified: `let` for `var`, `x = x + 1` for `x++`, and the asserted
  condition is the script's completion value. The test carries the full
  notice. The `TOK_DO` case of `js_parse_statement_or_decl` in
  `research/quickjs/quickjs.c` (`continue` targets the test; the optional
  `;` after `while (...)`) was consulted only. No code was copied.
- `tests/js_source.rs` `for_loops` (clj-quickjs-81): the first group of
  `test_for` (line 61) of `research/quickjs/tests/test_loop.js`, revision
  7d1c13c5d8f8, as JavaScript source compiled by this frontend. Modified:
  `let` for `var`, `c = c + 1` for `c++` in the body, and the asserted
  condition is the script's completion value; the second group (a `var`
  head) is in `var_heads_in_for_loops`. The test carries the full
  notice. The `TOK_FOR` case of `js_parse_statement_or_decl` in
  `research/quickjs/quickjs.c` (the head's scope, `continue` targeting the
  update, the loop variables closed before the first iteration and before
  each update) was consulted only. No code was copied.
- `tests/js_source.rs` `for_in_over_own_properties` (clj-quickjs-85): the
  first group of `test_for_in` (line 77) and `test_for_in2` (line 150) of
  `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: `let` for `var`,
  the keys collected with string `+` instead of `tab.push` and
  `tab.toString()`, and the asserted condition is the script's completion
  value; the other groups of `test_for_in` are left to clj-quickjs-86 to
  -90. The test carries the full notice. `js_parse_for_in_of`,
  `build_for_in_iterator`, `js_for_in_start` and `js_for_in_next` in
  `research/quickjs/quickjs.c` were consulted only. No code was copied.
- `tests/js_source.rs` `for_in_walks_the_prototype_chain` (clj-quickjs-86):
  the second and third groups of `test_for_in` (line 77) of
  `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: `let` for `var`,
  the keys collected with string `+` instead of `tab.push` and
  `tab.toString()`, and the asserted condition is the script's completion
  value. The test carries the full notice. `build_for_in_iterator`,
  `js_for_in_prepare_prototype_chain_enum` and `js_for_in_next` in
  `research/quickjs/quickjs.c` were consulted only. No code was copied.
- `tests/js_source.rs` `for_in_over_arrays` (clj-quickjs-88): the fourth
  group ("array optimization") of `test_for_in` (line 77) of
  `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: `let` for `var`,
  `a[a.length] = i` for `a.push(i)`, the keys collected with string `+`
  instead of `tab.push` and `tab.toString()`, and the asserted condition is
  the script's completion value, with a string-key check added. The test
  carries the full notice.
  `build_for_in_iterator` (its fast array path) and `js_for_in_next` in
  `research/quickjs/quickjs.c` were consulted only. No code was copied.
- `tests/js_source.rs` `var_heads_in_for_loops` (clj-quickjs-89): the
  second group of `test_for` (line 61) and the seventh group of
  `test_for_in` (line 77) of `research/quickjs/tests/test_loop.js`,
  revision 7d1c13c5d8f8, as JavaScript source compiled by this frontend.
  Modified: the for-in keys are collected with string `+` instead of
  `tab.push` and `tab.toString()`, and the asserted condition is the
  script's completion value. The test carries the full notice. The
  `TOK_FOR` case of `js_parse_statement_or_decl` and `js_parse_for_in_of`
  in `research/quickjs/quickjs.c` (a `var` head parsed without the `in`
  flag; the initializer early error of a for-in head) were consulted only.
  No code was copied.
- String values (clj-quickjs-65): `research/quickjs/quickjs.c`, revision
  7d1c13c5d8f8: `JSString` (8- and 16-bit code-unit storage),
  `JS_STRING_LEN_MAX`, `js_strict_eq2` (string branch), `js_add_slow`
  (string concatenation) and `js_string_compare` (code-unit order) were
  consulted only, for semantics. No code was copied.
- `src/frontend/tests.rs` `labelled_statements` and `tests/js_source.rs`
  `labelled_break_and_continue` (clj-quickjs-82): `test_for_break` (line
  193) and `test_cyclic_labels` (line 374) of
  `research/quickjs/tests/test_loop.js`, and `test_labels` (line 399) and
  `test_labels2` (line 410) of `research/quickjs/tests/test_language.js`,
  revision 7d1c13c5d8f8, as JavaScript source compiled by this frontend.
  Modified: `let` for `var`, `i = i + 1` for `i++` in `test_for_break`,
  the `with` statement of `test_labels` omitted, the next line after
  `while (1) label: break` is `y` in the parse-tree case, and the asserted
  conditions are script completion values; `test_cyclic_labels` is
  compiled as a function and its loop is run under an instruction budget
  and cancelled. Each test carries the notice. Consulted only, in
  `research/quickjs/quickjs.c`: the label handling of
  `js_parse_statement_or_decl`, `is_label`, `emit_break` and
  `push_break_entry` (`is_regular_stmt`). Unlike QuickJS, a label set
  (`L1: L2: while ...`) lets `continue` target any of its labels. No code
  was copied.
- `tests/js_source.rs` `addition_concatenates_strings` (clj-quickjs-66):
  the message expression of the `assert` helper (line 1) and the number
  `19686109595169230000` of `test_cvt` (line 108) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: the helper's parameters are `let` bindings, its optional
  message part is omitted, and the number is converted by `"" +` instead
  of `.toString()`. The test carries the full notice.
  `research/quickjs/quickjs.c` `js_add_slow`, `JS_ToStringInternal` and
  `js_dtoa2` were consulted only, for semantics; the Number::toString
  layout in `src/value/string.rs` follows ECMA-262 and takes its digits
  from Rust's shortest and correctly rounded formatting. No code was copied.
- `tests/js_source.rs` `switch_statements` (clj-quickjs-83):
  `test_switch1` (line 208) and `test_switch2` (line 230) of
  `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8, as
  JavaScript source compiled by this frontend. Modified: `let` for `var`
  and the asserted condition is the script's completion value (`s += a` is
  upstream's since clj-quickjs-84; it was `s = s + a` before); a numeric version maps `"a"`, `"b"`, `"c"` to 1, 2, 3
  and appends with `s = s * 10 + a` (and sets `a = i` for `a = "" + i`).
  The test carries the full notice. The
  `TOK_SWITCH` case of `js_parse_statement_or_decl` in
  `research/quickjs/quickjs.c` (discriminant evaluated once, cases tested
  in order with `strict_eq`, then a jump to `default`; one scope for the
  case block; "duplicate default") was consulted only. Unlike QuickJS,
  the discriminant lives in a scratch local rather than on the operand
  stack, and all tests precede the clause bodies. No code was copied.
- `tests/js_source.rs` `strings_convert_with_string_to_number` and
  `strings_compare_loosely_with_numbers` (clj-quickjs-67): the four string
  groups of `test_cvt` (line 108) and the string rows of `test_eq` (line
  126: `"" == 0`, `"123" == 123`, `"122" != 123`) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: each `assert(cond)` is a Rust assertion on the expression's
  value. The first test carries the full notice.
  `research/quickjs/quickjs.c` `js_atof`, `JS_ToNumberHintFree` and
  `js_eq_slow`, and `research/quickjs/libunicode.h` `lre_is_space`, were
  consulted only, for semantics; `string_to_number` in
  `src/value/string.rs` follows ECMA-262 StringToNumber and parses decimals
  with Rust's correctly rounded `f64` parser. No code was copied.
- `tests/js_source.rs` `strings_compare_by_code_units` (clj-quickjs-68):
  the `('b' > 'a')` case (line 103) of `test_op1` of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: `assert(a, b, msg)` is a Rust assertion on the expression's
  value. The notice is on `strings_convert_with_string_to_number`.
  `research/quickjs/quickjs.c` `js_relational_slow` and
  `js_string_compare` were consulted only, for semantics; `is_less_than` in
  `src/vm/dispatch.rs` follows ECMA-262 IsLessThan on primitives. No code
  was copied.
- Compound assignment (clj-quickjs-84): `js_parse_assign_expr2` in
  `research/quickjs/quickjs.c`, revision 7d1c13c5d8f8 (`TOK_MUL_ASSIGN` ..
  `TOK_POW_ASSIGN`: `get_lvalue` keeping the reference, the right operand,
  the operator, `put_lvalue`), was consulted only, for the evaluation
  order. `tests/js_source.rs` `compound_assignment_on_bindings` and
  `src/frontend/tests.rs` `compound_assignment` are written for this
  project. No code was copied.
- Compound assignment on properties (clj-quickjs-260): the same function
  and `get_lvalue` (`OP_get_field2`, `OP_get_array_el3`: the reference
  kept on the stack under the value read) in `research/quickjs/quickjs.c`,
  revision 7d1c13c5d8f8, were consulted only, for the evaluation order.
  `tests/js_source.rs` `compound_assignment_on_properties`,
  `tests/need.rs` `property_compound_assignment_resumes_after_wait` and
  `tests/gc_roots.rs`
  `property_compound_assignment_base_survives_wait_collection_and_migration`
  are written for this project. No code was copied.
- `tests/js_source.rs` `typeof_operator` (clj-quickjs-69): the `typeof 1`,
  `typeof null` and `typeof unknown_var` rows of `test_op2` (line 195) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: `assert(actual, expected, msg)` is a Rust assertion on the
  expression's value; `typeof Object` now reads its global binding. The
  notice is on `identifiers_are_not_normalized`, which also runs the
  `typeof õ` case of `test_unicode_ident`. `research/quickjs/quickjs.c`
  `js_operator_typeof`, `OP_typeof`, `js_parse_unary` and
  `OP_get_var_undef` were consulted only, for semantics: here an
  unresolvable operand uses a run-time lookup (clj-quickjs-104)
  and `type_of` in `src/vm/dispatch.rs` classifies this runtime's value
  kinds. No code was copied.
- `tests/js_source.rs` `strings_are_code_unit_sequences` (clj-quickjs-141):
  the literal rows of the unicode group of `test_string` (line 235) and its
  `"\0"` row (line 339) of `research/quickjs/tests/test_builtin.js`,
  revision 7d1c13c5d8f8. Modified: each `assert(a, b)` is a Rust assertion
  on `a === b`; `String.fromCharCode(0x20ac)` is the literal `"€"` and
  `eval('"\0"')` the literal itself; the `length`, `charCodeAt`,
  `codePointAt` and `String.fromCodePoint` rows are covered by later string
  property/method tests; `test_rope` runs in `upstream_string_rope`
  (clj-quickjs-146). The notice is on
  `strings_convert_with_string_to_number`. `research/quickjs/quickjs.c`
  `js_parse_string`, `JS_ConcatString` and `js_string_compare` were
  consulted only, for semantics. No code was copied.
- `tests/js_source.rs` `runtime_error_positions_follow_operator_tokens`
  (clj-quickjs-52): the operator, call and reference cases of
  `test_line_column_numbers` (line 1118) of
  `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8, where
  `@` marks the expected position. Modified: a function value stands in
  for `poisoned_number` (no objects or ToPrimitive yet, so the operator
  fails with `UnsupportedOperand`), `null` is called through a binding,
  and the caller-frame case calls a script function instead of `Error`;
  the `null . abcd` member-access case runs since clj-quickjs-114 (its
  getter and setter cases wait for accessors). The test
  carries the full notice. `research/quickjs/quickjs.c` `emit_source_loc`
  in `js_parse_postfix_expr`, `js_parse_unary` and `js_parse_assign_expr2`,
  `build_backtrace` and `get_line_col` were consulted only. No code was
  copied.
- `tests/js_source.rs` `object_literals_and_named_properties` and
  `src/frontend/tests.rs` `object_literals` (clj-quickjs-114): the
  `{x: 1, if: 2, async: 3}` row of `test_op2` (line 195), the nested
  `{a: {b: 2}}` shape of `test_optional_chaining` (line 610), and the
  contextual-keyword keys and `{x, get}` shorthands of `test_object_literal`
  (line 378) in `research/quickjs/tests/test_language.js`, revision
  7d1c13c5d8f8. Modified: each `assert` is a Rust assertion on the
  expression's value or parse tree; the getter/setter, method, computed-key
  and `__proto__` parts wait for their pellets. The notice is on the `js_source` test and on
  `number_literal_member_access` in the frontend tests.
  `research/quickjs/quickjs.c` `js_parse_object_literal`,
  `js_parse_property_name`, `JS_GetPropertyInternal`,
  `JS_SetPropertyInternal`, `add_property` and the `OP_object`,
  `OP_get_field`, `OP_put_field` and `OP_define_field` cases were consulted
  only, for semantics. No code was copied.
- `tests/js_functions.rs` `default_parameters` (clj-quickjs-100): the arrow
  `(a, b = 1, c) => {}` of `test_function_length` (line 445) and the
  strict `f = function f(a = f) { return a; }; f() === f` case of
  `test_argument_scope` (line 453) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: the arrow is called (`a + c`) instead of asking its `length`
  (clj-quickjs-101); the `arguments`, `this` and `eval` cases wait for
  clj-quickjs-97, -71 and -109. The test carries the full notice.
  `research/quickjs/quickjs.c` `js_parse_function_decl2`
  (`has_parameter_expressions`: an argument scope of `let`-like
  parameters, each default under `OP_get_arg`/`OP_strict_eq undefined`)
  was consulted only. No code was copied.
- `tests/js_source.rs` `object_intrinsics` (clj-quickjs-117): the
  `a = {}; a.toString() === "[object Object]"` case of `test` (line 124) in
  `research/quickjs/tests/test_builtin.js` and the `Object.is(actual,
  expected)` comparison of the `assert` helper (line 1) in
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: each `assert` is a Rust assertion on the expression's value;
  `new Object()` waits for built-in constructors (clj-quickjs-71 constructs
  with ordinary functions only); the `Object.getPrototypeOf` row is in
  `prototype_reflection` (clj-quickjs-121, below). The notice on `object_literals_and_named_properties`
  covers them. `research/quickjs/quickjs.c` `js_object_constructor`,
  `js_object_toString`, `js_object_valueOf`, `js_object_is`, `JS_ToObject`,
  `JS_GetPropertyInternal` (prototype walk), `JS_SetPropertyInternal`
  (inherited read-only check) and `CASE(OP_call_method)` were consulted
  only, for semantics. No code was copied.
- `tests/js_source.rs` `template_literal_values` (clj-quickjs-57):
  `test_template` (line 356) and `test_template_skip` (line 371) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: the `String.raw` case is dropped (clj-quickjs-58), each
  `assert` is a value the completion checks, and the destructuring
  default of `test_template_skip` is a plain `var` initializer. The test
  carries the full notice; `frontend::tests::template_literals` parses the
  same forms. `research/quickjs/quickjs.c` `js_parse_template_part`,
  `js_parse_template`, `js_parse_string` and the template case of
  `js_parse_skip_parens_token` were consulted only, for semantics. No
  code was copied.
- `tests/js_source.rs` `define_property_data_descriptors`
  (clj-quickjs-119): the `Object.defineProperty(a, "y", { value: 3,
  writable: true, configurable: true, enumerable: true }); a.y === 3` case
  of `test` (line 124) in `research/quickjs/tests/test_builtin.js`,
  revision 7d1c13c5d8f8. Modified: `new Object()` is `Object()`
  (built-in constructors are not constructors yet) and the `assert` is a Rust assertion. The notice on
  `object_literals_and_named_properties` covers it.
  `research/quickjs/quickjs.c` `js_object_defineProperty`,
  `JS_DefineProperty`, `js_update_property_flags`, `js_obj_to_desc` and
  `js_object_getOwnPropertyDescriptor` were consulted only, for semantics.
  No code was copied.
- `tests/js_source.rs` `function_own_properties` (clj-quickjs-125): the
  `f.prototype.constructor === f` case and the `g` rows
  (`Object.defineProperty(g, "prototype", { writable: false })`, then
  `g.prototype.constructor === g`) of `test_prototype` (line 260) in
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: each `assert` is a Rust assertion on the expression's value;
  the `g` rows are asserted to fail explicitly until clj-quickjs-119. The
  notice on `loose_equality_with_a_boolean_left_operand` covers them.
  `research/quickjs/quickjs.c` `js_instantiate_prototype`, `js_closure2`
  and `js_closure` were consulted only, for semantics. No code was copied.
- `tests/js_source.rs` `new_operator_constructs_objects` (clj-quickjs-71):
  the constructor `F` (`function F(x) { this.x = x; }`, line 190) and the
  `b = new F(2); b.x === 2` case of `test_op2` (line 195) in
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: the `assert` is a Rust assertion; the `a = new Object` case is
  asserted to be a catchable `TypeError` (built-in constructors are not
  constructors yet). The notice on
  `loose_equality_with_a_boolean_left_operand` covers it.
  `research/quickjs/quickjs.c` `js_parse_postfix_expr` (`TOK_NEW`),
  `OP_call_constructor`, `JS_CallConstructorInternal`,
  `JS_CreateFromConstructor` and `js_get_prototype_from_ctor` were consulted
  only, for semantics. No code was copied.
- `tests/js_source.rs` `prototype_reflection` (clj-quickjs-121): the
  `Object.getPrototypeOf(a)`, `Object.create(a)` and
  `Object.setPrototypeOf(a, c)` rows of `test` (line 124) in
  `research/quickjs/tests/test_builtin.js` and the `o2.__proto__ = o1;
  o2.f() === o2` row of `test_arrow_function` (line 128) in
  `research/quickjs/tests/test_closure.js`, revision 7d1c13c5d8f8.
  Modified: `new Object()` is `Object()`, the accessor literal `a` is a
  data literal (clj-quickjs-120), the methods are function-valued
  properties and the `eval("super.f()")` arrow is left out, and each
  `assert` is a Rust assertion. The test carries the full notice.
  `research/quickjs/quickjs.c` `js_object_getPrototypeOf`,
  `js_object_setPrototypeOf`, `js_object_create`,
  `JS_SetPrototypeInternal`, `js_object_get___proto__`,
  `js_object_set___proto__`, the `__proto__` case of
  `js_parse_object_literal` and `CASE(OP_set_proto)` were consulted only,
  for semantics. No code was copied.
- `tests/js_source.rs` `object_extensibility` (clj-quickjs-122): the
  `Object.isExtensible(a)`, `Object.preventExtensions(a)`, `a.y = 2`
  throws and `typeof a.y` rows of `test` (line 124) in
  `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8.
  Modified: `var` is `let` and the asserts are joined into one condition
  checked by a Rust assertion. The test carries the full notice.
  `research/quickjs/quickjs.c` `js_object_isExtensible`,
  `js_object_preventExtensions`, `JS_PreventExtensions`, `js_object_seal`
  and `js_object_isSealed` were consulted only, for semantics. No code
  was copied.
- `tests/js_source.rs` `own_property_key_order` (clj-quickjs-123): the
  object literal and expected key list of `test_enum` (line 174) in
  `research/quickjs/tests/test_builtin.js`, and the
  `Object.getOwnPropertyNames(x).toString() === "0,length"` assertion of
  `test_spread` (line 435) in `research/quickjs/tests/test_language.js`,
  revision 7d1c13c5d8f8. Modified: the arrays are compared element by
  element and by `length` instead of with `assert(tab, [...])` and
  `toString` (clj-quickjs-39), and the spread `[ ...[ , ] ]` is written
  `[ , 1]` (spread is its own pellet). The test carries the full notice.
  `research/quickjs/quickjs.c` `JS_GetOwnPropertyNamesInternal`,
  `num_keys_cmp`, `JS_GetOwnPropertyNames2`, `js_object_keys` and
  `js_object_getOwnPropertyNames` were consulted only, for semantics. No
  code was copied.
- `tests/js_source.rs` `instanceof_operator` (clj-quickjs-75): the
  `a = {}; (a instanceof Object)` case of `test_op2` (line 195) in
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: `let` declares `a`, the `assert` is a Rust assertion, and the
  `a instanceof String` case waits for the `String` constructor
  (clj-quickjs-72). The notice on
  `loose_equality_with_a_boolean_left_operand` covers it.
  `research/quickjs/quickjs.c` `js_operator_instanceof`,
  `JS_IsInstanceOf`, `JS_OrdinaryIsInstanceOf` and `OP_instanceof` were
  consulted only, for semantics (the two TypeError messages are QuickJS's).
  No code was copied.
- `tests/js_source.rs` `delete_property_references` (clj-quickjs-124):
  the `a = {x: 1, y: 1}; delete a.x`, `delete "abc"[100]` and
  `delete null.a` rows of `test_delete` (line 222) in
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: `let` declares `a`, the asserts are Rust assertions,
  `("x" in a) === false` is checked with `in` (clj-quickjs-74), and
  the caught error is checked as a `TypeError`
  failure rather than with `instanceof TypeError`. The notice on
  `object_literals_and_named_properties` covers it.
  `research/quickjs/quickjs.c` `js_parse_delete`, `CASE(OP_delete)`,
  `JS_DeleteProperty` and `delete_property` were consulted only, for
  semantics. No code was copied.
- `tests/js_source.rs` `in_operator` (clj-quickjs-74): the
  `a = {x : 2}; assert(("x" in a), true); assert(("y" in a), false)` cases
  of `test_op2` (line 195) in `research/quickjs/tests/test_language.js`,
  revision 7d1c13c5d8f8. Modified: `let` declares `a`, the asserts are one
  `===` conjunction checked by a Rust assertion. The notice on
  `loose_equality_with_a_boolean_left_operand` covers it.
  `research/quickjs/quickjs.c` `js_operator_in`, `OP_in` and
  `JS_HasProperty` were consulted only, for semantics. No code was copied.
- `tests/js_source.rs` `methods_on_primitive_bases` (clj-quickjs-73): the
  `assert((19686109595169230000).toString() === "19686109595169230000")`
  case of `test_cvt` (line 108) in
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8.
  Modified: the `assert` is a Rust assertion on the string value. The
  notice on `addition_concatenates_strings` covers it.
  `research/quickjs/quickjs.c` `JS_GetPrototypePrimitive`,
  `js_thisNumberValue`, `js_number_valueOf`, `js_number_toString` and
  `js_thisStringValue` were consulted only, for semantics. No code was
  copied.
- `tests/js_functions.rs` `call_apply_and_reflect_apply` (clj-quickjs-102):
  the `my_func.call(null, 1, 2)`, `my_func.apply(null, [1, 2])`,
  `(function () { return 1; }).apply(null, undefined)` and
  `assert_throws(TypeError, ... Reflect.apply(..., null, undefined))` rows
  of `test_function` (line 83) with its helper `my_func` (line 78) in
  `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8.
  Modified: the asserts are Rust assertions and `assert_throws` is a
  `try`/`catch`; the `new Function` rows wait for clj-quickjs-111.
  Bound functions are covered separately by clj-quickjs-103. The test carries the full notice.
  `research/quickjs/quickjs.c` `js_function_proto`, `js_function_call`,
  `js_function_apply`, `build_arg_list` and `js_reflect_apply` were
  consulted only, for semantics (call and apply are tail calls here, not
  C-stack calls). No code was copied.

- `tests/js_source.rs` `string_length_and_index_properties`
  (clj-quickjs-142): the `a = String("abc"); a.length === 3; a[1] === "b"`
  (lines 238-240) and `a = "\u{10ffff}"; a.length === 2` (lines 257-258)
  cases of `test_string` (line 235) in
  `research/quickjs/tests/test_builtin.js`, revision 7d1c13c5d8f8.
  Modified: the assertions are Rust assertions, with additional primitive
  and wrapper descriptor, mutation and enumeration cases. The notice on
  `object_extensibility` covers it. `research/quickjs/quickjs.c`
  `js_string_get_own_property`, `js_string_define_own_property`,
  `js_string_obj_get_length` and `js_string_constructor` were consulted
  only, for semantics. No code was copied.

## Array length changes (clj-quickjs-154)

`vm::property::length_descriptor` validates primitive length values as
ToUint32/ToNumber before mutation, normalizing negative zero. It is shared
by assignment and `Object.defineProperty`; object values explicitly fail
through the existing unsupported ToPrimitive path, so conversion runs no
JavaScript under the heap's object-table lock. A future object conversion
must run through resumable VM frames before entering that lock.

`ArrayObject::define_length` validates the non-configurable length
property's descriptor, grows without allocating elements, or calls
`truncate`. Truncation deletes sparse entries from the greatest key down,
then dense elements from the end; a failed deletion leaves length at that
index plus one. Dense slots and attribute exceptions above the resulting
length are dropped, so the collector sees no removed edges and regrowth
cannot resurrect elements or attributes. The operation is atomic under the
object-table lock and costs at most the stored sparse entries and dense
slots visited, independent of holes in the numeric length. It creates no
per-index work for sparse gaps and retains no Rust stack across a wait.

The resulting writable flag is applied even on partial truncation failure.
`[[Set]]` maps rejection through the executing code's strict flag (strict
TypeError, sloppy no-op); `defineProperty` always throws. Invalid length
values use the new catchable `VmErrorKind::RangeError` through the existing
ordinary Error instance allocation/unwinding path (clj-quickjs-230). No new heap kind or
constructor is introduced. `tests/js_source.rs` covers both entry paths,
and `array_length_truncation_reclaims_elements` in `tests/gc_roots.rs`
verifies collection of dense and sparse element values while a suspended
frame keeps their array alive.

## Bound functions (clj-quickjs-103)

`HeapObject::BoundFunction` appends a callable kind with immutable target,
receiver and arguments. `dispatch::resolve` follows bound targets and the
existing forwarding built-ins iteratively, prepending arguments and preserving
bound receivers, before entering the target's task-owned frame. Construction
unwraps bound targets under the same argument and hop limits, then uses the
ordinary constructor path. `instanceof` unwraps to the target's prototype.
No JavaScript call retains a Rust frame across suspension.

Initial `length` and `name` live in an immutable internal property store,
traced through the bound object, so metadata reads and reflection do not
prevent publication. The first property change copies this store into the
same task-owned record used by ordinary functions. Publication traverses the
bound operands and rejects mutable records, including records attached during
a publication walk; installing a record and marking shareable exclude each
other under the object-table lock. Ordinary-function metadata now follows
the same descriptor/publication rules (clj-quickjs-101); `new.target`
syntax remains with clj-quickjs-99.

Rejected assignments to the initial read-only metadata follow strict/sloppy
`[[Set]]` rules without attaching a record. Publication implicitly freezes
the bound function; reflected metadata descriptors then become
non-configurable, matching its reported frozen integrity level.

Semantic references, independently implemented rather than copied:
`research/quickjs/quickjs.c`, `js_function_bind`, `js_call_bound_function`,
`JS_OrdinaryIsInstanceOf`, and `research/quickjs/tests/test_builtin.js`,
`test_function`, at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
The added source tests are synthetic fixtures, not adapted upstream text or
light-cljs output.

## Array push (clj-quickjs-155)

`Builtin::ArrayPrototypePush` is appended to the built-in registry and
installed on the realm's immutable `Array.prototype`, with length 1 and
name `push`. The `Array` constructor is exposed through an
appended built-in-global operand so `Array.prototype.push.call(o, item)`
works without implementing construction. No opcode or heap kind is added.

The leaf builtin reads `length`, applies primitive ToLength, checks the
`2^53 - 1` limit, and uses `property::put` for each index followed by
`put_field` for `length`, always passing Throw=true. That preserves owner
checks, inherited read-only properties, extensibility, ArraySetLength and
partial writes on failure. Numeric array indices reuse the dense storage
path (`ArrayObject::set`/`add`/`store`): a Vec append costs amortized O(1),
with no scan of prior elements. The fixed realm prototype chain adds
constant work; a custom chain is walked as required by OrdinarySet. Sparse
arrays keep their existing sparse fallback. Indices from `2^32 - 1` onward
are ordinary named properties; a failing final array length Set leaves
them in place.

The native loop is bounded by the task's operand-stack argument limit,
independent of the receiver's numeric length. Arguments and receiver stay
rooted on the operand stack for the instruction; array elements use the
existing precise tracing implementation. `tests/js_source.rs::array_push`
runs 10,000 appends of numbers and heap objects under instruction budgets,
collecting between yields. No JavaScript callback runs inside the leaf:
object-valued length conversion and primitive wrapper receivers fail
explicitly. Future ToPrimitive/getter support must enter resumable native
frames before invoking JavaScript, as for the other built-ins.

## Resumable object conversion (clj-quickjs-70)

`native::start_conversion` intercepts object operands of ToNumber,
ToNumberPair, Plus, Compare*, LooseEq, ToString and computed property
operations (GetElem, PutElem, ToPropertyKey, In and Delete). Primitives
keep the existing leaf path. Object equality and nullish comparisons do
not convert; property operations reject an invalid base first.

A Convert native frame owns the original instruction, its operands, the
operand index, hint and next method index. A step looks up valueOf or
toString and returns a Call action with the original receiver and no
arguments. The existing trampoline resolves forwarding built-ins and
enters a task-owned callback frame. A primitive return replaces that
operand; an object return advances to the other method, looked up only
then so callback mutations are visible. Left conversion finishes before
right conversion starts. With no primitive result the native raises a
catchable TypeError. Absent Function.prototype methods continue along the chain for
conversion; explicit reads continue reporting their unsupported status.

After conversion a Resume action restores the converted operands, drops
the native frame and executes the original leaf instruction. This calls
one instruction handler, never the interpreter loop; no Rust stack survives
a yield or wait. Every callback and native step consumes budget and uses
the existing safepoints. NativeFrame::trace visits all operands, including
the unvisited right operand and unrelated property values. Exceptions and
cancellation use the same unwind/discard path as reduce and needAll.
Symbols are primitives (clj-quickjs-127); well-known coercion protocol
dispatch and resumable conversions inside other leaf built-ins remain
with their own pellets.

## Number and String wrapper data (clj-quickjs-72)

`OrdinaryObject` has an optional, typed `PrimitiveData` slot (`Number(f64)`
or `String(JsString)`), initialized before allocation and immutable thereafter.
`HeapObject::Plain` still handles property storage, ownership, prototype edges,
integrity, publication classification and tracing. This avoids a new heap kind
and duplicated dispatch for ordinary wrapper properties. It costs one optional
primitive-data field in each ordinary store. The alternatives were distinct
Number/String heap kinds (more property dispatch and inventory cases), or a
JavaScript property (incorrect: user-visible and forgeable). The prototypes
are intrinsic ordinary stores with the same slots, containing +0 / `""`, so
thisNumberValue/thisStringValue inspect internal data instead of comparing
against intrinsic identities. Inheriting a prototype never inherits its slot.

Only the owning task may access an allocated wrapper's primitive data;
intrinsic prototypes remain shared and immutable. No new root category or
object kind is needed. The collector traces each ordinary store's properties
and prototype as before. PrimitiveData has no heap edges: f64 is inline and
JsString uses the existing immutable Arc storage boundary. The source test
`primitive_wrappers_survive_wait_collection_and_migration` forces collection
while wrappers and nested properties are held across a wait, resumes on another
thread, verifies data, rejects publication and foreign reads, then checks
reclamation after the last root is dropped.

Calls and construction use the pure ToNumber/ToString helpers for primitives.
An object argument enters the same NativeState::Convert continuation as
coercing instructions, with number hint for Number and string hint for String.
The conversion target records whether to return a primitive or allocate a
wrapper after conversion. Forwarded calls and bound constructors resolve their
arguments first. Built-in callbacks can push a nested conversion at the same
call depth; native-frame count is bounded by the call-depth limit. Every
operand remains traced until its method returns a primitive, and callbacks
run through explicit VM frames, preserving waits, GC, budget yields, migration,
exceptions and cancellation. Implicit wrapper equality and addition use the
existing resumable operator paths, observing overridden conversion methods.
String exotic own properties and enumeration use the immutable string slot;
see "String own properties" (clj-quickjs-142).

Semantic references only (no upstream implementation code copied):
`research/quickjs/quickjs.c` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `js_number_constructor`,
`js_string_constructor`, `js_thisNumberValue`, `js_thisStringValue`.
The first three snippets in `tests/js_source.rs`
`primitive_wrappers_convert_with_value_of` adapt
the wrapper expressions of `research/quickjs/tests/test_language.js`, `test_eq`,
at that revision. The test references the full QuickJS copyright and MIT notice
on `loose_equality_with_a_boolean_left_operand` in the same file.

## Arrow receiver captures (clj-quickjs-98)

The parser accepts `this` in scripts, functions and arrows, including
parameter defaults (clj-quickjs-104). An ordinary function has its own
receiver; script-level arrows capture the global object.
`lower::Place::This` loads an ordinary frame's receiver with `get_this`.
`Lowerer::resolve_this` adds one value capture to each intervening arrow,
reusing it for repeated reads and nested closures. Both lowering passes
visit these captures identically; they need no mutable-binding facts or
cells because a frame's receiver is immutable. Closure tracing and
transitive publication therefore apply without new VM or heap state.

QuickJS semantic reference: `research/quickjs/quickjs.c`,
`js_parse_function_decl2` and `OP_special_object`, and
`research/quickjs/tests/test_closure.js`, `test_arrow_function`, revision
7d1c13c5d8f8c6b32893786ed35cdcebd88194d2. No source or fixture text was
copied; the tests added for this pellet are synthetic.

### Arguments object creation and lexical capture (clj-quickjs-97)

The lowerer's existing analysis pass records references to an implicit
`arguments` binding, including through arrows. Only referenced ordinary
functions emit `new_arguments` (`0x65`); scripts and arrows introduce no object.
With default parameters the binding lives in the parameter scope, before
initializers, and body `var` bindings copy it under the existing scope rules.
The normal value/cell rule handles assignment and capture; reaching the ordinary
object remains task-owned and fails publication.

Loaded code derives its argument-retention flag from the instruction stream,
so ExecutableSpec round trips keep the behavior. A frame retains original
actual values only until `new_arguments` builds an ordinary object with
`Object.prototype`; frame tracing visits those values before materialization,
and ordinary property tracing visits them afterward. Retained values count
against call storage limits. No Rust frame is retained across yields or waits.

QuickJS semantic references: `research/quickjs/quickjs.c` `js_build_arguments`,
`add_arguments_arg`, and `OP_special_object`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The Rust implementation and inline
synthetic tests are original; no upstream code or test text was copied. Mapped
sloppy arguments, the callee accessor, arguments branding, and Symbol.iterator
(clj-quickjs-127) remain deferred.

## String code-unit built-ins (clj-quickjs-143)

Five appended `Builtin` variants implement `charAt`, `charCodeAt`,
`codePointAt`, `String.fromCharCode` and `String.fromCodePoint`. The realm
build appends three own methods on `String.prototype` and two on `String`
before freezing its views; existing intrinsic tracing and publication cover
these functions. All use `PropertyFlags::METHOD` and function length 1.
There are no new heap kinds or mutable string representations.

Access methods use RequireObjectCoercible and ToString, then
ToIntegerOrInfinity, checking bounds before any Rust index cast. They read
the flat `JsString::code_units()` slice in O(1); `codePointAt` reads at most
two units. These generic methods accept number and boolean receivers as
well as strings, unlike the stricter `this_string_value` used by
`String.prototype.toString`/`valueOf`. The prototype itself represents the
empty string. Null/undefined receivers take the existing catchable
TypeError path.

Static constructors build arbitrary UTF-16 units, including lone
surrogates. `fromCharCode` truncates and reduces finite numbers modulo
65536; `fromCodePoint` validates finite integral numbers before encoding a
point, using the existing catchable `Fault::RangeError` path for invalid
points. The leaf loop costs O(argument count plus total primitive-string
conversion work), with at most two output units per argument and
`MAX_STRING_LEN` enforced. The configured VM value-stack limit (default
2^20 values, with `apply` checking stack room) bounds the argument count,
but repeated large primitive strings can make the aggregate conversion
work large within one call. Bounded resumable coercion is clj-quickjs-265;
until then this can delay yielding and collection.
Primitive-only calls run no JavaScript and retain no continuation across a
wait. Object operands select `native::start_builtin_conversion` after
call/apply/bind resolution, including native callbacks that are built-ins.
`NativeState::Convert` retains and traces all relevant operands. Access
converts the receiver with string hint, finishes primitive ToString, then
converts the position with number hint. Static arguments convert with
number hint and validate each code point before any later callback runs.
Completed operands are stored as strings or numbers and never replayed.
The final primitive-only helper then accesses or constructs the string.
Nested built-in conversions share the existing native-depth bound, unwind
and cancellation protocol. The source coercion tests force one-instruction
yields, GC between slices and during waits, thread migration, callback
errors, forwarding and nested natives. Strings remain independently owned,
immutable leaf buffers outside the heap.

## Concise object-literal methods (clj-quickjs-126)

`FunctionKind::Method` distinguishes `{m() {}}` from `{m: function () {}}`.
The parser accepts identifier names (reserved words included), strings and
numbers as keys; `get()`, `set()`, `async()` and `static()` are method names.
Methods inherit the surrounding strictness or become strict through a body
`"use strict"` directive (clj-quickjs-266), own their receiver and unmapped
arguments, and support ordinary parameters, defaults and closures. Arrows nested inside
methods capture the method's receiver. Methods have no lexical self-name;
NamedEvaluation supplies the property's string key as function metadata.
Constructor capability is separate from owning arguments: methods have neither
a lazy own `prototype` nor constructor capability.

Lowering keeps `Lowerer::expr` a dispatcher and uses the object-literal and
closure helpers. The append-only `method` opcode (`0x66`, u16 child-code index)
allocates the closure with the literal below its captured values as its
`FunctionObject::home_object`, preserving the literal on the operand stack for
`define_field`. That field defines the usual writable, enumerable, configurable
data property. A `__proto__() {}` method is a data property, including beside
a `__proto__: value` prototype setter. Computed keys, accessors, async/generator
methods and `super` remain deferred.

The home-object reference is immutable from allocation, separate from captures
and the optional property record chosen by clj-quickjs-125. Function objects
remain `Persistent` in `src/value/share.rs::classify`: publication follows the
new edge with `.home_object` in its diagnostic path, so every concise method
currently fails publication at its task-owned literal, even if detached or
unused by its body. Reading metadata or the absent `prototype` creates no
property record. `Trace` follows the home object, and the precise collector
retains and reclaims literal/method cycles; the root inventory names the
retention and reclamation tests. No new heap kind or root category is needed.

Semantic references inspected, without porting implementation code:
`research/quickjs/quickjs.c`, `js_parse_object_literal`,
`js_method_set_home_object`, and `OP_define_method`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The receiver row in
`tests/js_source.rs::object_literal_methods` adapts `test_optional_chaining`
in `research/quickjs/tests/test_language.js` at that revision, under the
copyright/license notice on `object_literals_and_named_properties` in the same
file. All other new fixtures are synthetic, not light-cljs output.

## String substring search (clj-quickjs-144)

Two appended built-ins add `indexOf` and `lastIndexOf` as non-enumerable
`String.prototype` methods with length 1. They require a non-null receiver
and use generic ToString, including observable String-wrapper conversions.
The existing `NativeState::Convert` visits receiver and search with string
hint, then position with number hint, in order. It retains all unconverted
operands as traced values while callbacks yield, wait, collect or migrate.
Absent search becomes `"undefined"`. Position truncates toward zero and
clamps to the UTF-16 length; NaN means zero for `indexOf` and positive
infinity for `lastIndexOf`.

Every call, including primitive-only calls and forwarded native callbacks,
then uses `NativeState::StringSearch`. Its `StringSearch` owns immutable
`JsString` leaves and a Rust-owned KMP prefix table, with no heap edges.
Prefix construction and scanning perform at most 256 code-unit comparisons
per native step; each step charges the instruction budget and allows a
safepoint. Total search work is O(receiver length + search length), with
O(search length) auxiliary indices. The index vector grows through normal
Rust allocations (amortized linear copying); allocation itself is not
interruptible. Reverse search scans forward through the last eligible match
end and retains the latest start, preserving overlapping matches. Empty
search returns the clamped position. Unwind and cancellation drop the search
with the existing native-frame protocol. Primitive coercion still has the
bulk-conversion limitation tracked by clj-quickjs-265.

## String extraction and combination (clj-quickjs-145)

Five appended realm built-ins implement `substring`, `slice`, `concat`,
`padStart` and `padEnd`. Every call enters a native conversion frame so
primitive-only calls also budget result construction. Receivers undergo
RequireObjectCoercible and generic ToString, rather than the restrictive
thisStringValue used by `String.prototype.toString`/`valueOf`. The existing
ToPrimitive continuation retains unconverted values as precise roots and
calls observable conversion methods as ordinary VM frames. Hints follow
operand roles: string for receivers, concat arguments and fillers; number
for extraction positions and target lengths. Padding stops before filler
conversion when no padding is needed. Concat checks its accumulated length
before visiting the next operand. Omitted/undefined ends and fillers are
represented by absent conversion operands, so objects returning undefined
still undergo the required numeric or string conversion.

`NativeState::StringBuild` owns flat-copy or whole-operand rope plans (immutable source strings,
a starting unit, output length) and an output `Vec<u16>`, with no heap edges.
A repeated plan supplies padding. Each native step copies at most 256 units
or advances part boundaries, charging the instruction budget and allowing a
safepoint. The result length is checked against `MAX_STRING_LEN` (`2^30 - 1`)
before reserving output storage; overlong results throw `RangeError` with
"string too long". Empty padding returns the original text even for an
infinite target. A plan representing one whole source reuses that string.

Flat `JsString` bodies own a `Vec<u16>` instead of a boxed slice;
they remain immutable behind independently owned `Arc` storage. This permits
the native builder to reserve once before bounded copying and transfer the
completed allocation without a final linear shrink/copy. Allocation failure
returns a catchable error; allocation itself is not interruptible. Bulk
operand conversion and plan creation retain the limitation tracked by
clj-quickjs-265. Unwind/cancellation drop the builder with its native frame.
No heap kind, tracing edge or publication category changes. Concat uses
the rope assembly path added in clj-quickjs-146, retaining whole operands
and processing at most 256 parts per native step. Other plans still copy
code units. Tests cover callback waits, budget yields,
collection and thread migration during coercion and long result construction.

## Number predicates and constants (clj-quickjs-147)

The shared realm appends `isNaN` and `isFinite` to `Global` (operands 6 and
7). Their task-owned global bindings resolve with `get_global`, like other
built-in globals (clj-quickjs-104). It also installs four Number predicate
functions and eight primitive Number constants before copying the intrinsic views and publishing
the realm. Constants use `PropertyFlags::FIXED`; `MIN_VALUE` is binary64
bit pattern 1, the smallest subnormal (Rust's `MIN_POSITIVE` is normal).
All function objects use the existing intrinsic roots and shareability rules.

The Number predicates are bounded leaf computations that inspect only
`Value::Number`; objects and wrappers return false without callbacks. Global
predicates apply ToNumber to the first argument (undefined if absent).
Object arguments enter `native::Convert` with number hint and a
`NumberPredicate` completion target. The traced operand vector roots the
receiver across ordinary VM callback frames, waits, yields, GC and worker
migration; completion applies the same leaf predicate to the resulting
primitive. No Rust frame survives suspension. Forwarded, bound and native
callback calls use this same conversion entry point.

Synthetic source tests cover constants, descriptors, numeric boundaries,
non-coercing object inputs, conversion order and throws. The existing
one-instruction conversion matrix additionally exercises these globals
through waits, collection at every yield and thread migration.

## Number prefix parsing (clj-quickjs-148)

The append-only realm global table adds `parseInt` and `parseFloat`;
`Number.parseInt` and `Number.parseFloat` reference those exact intrinsic
function objects. Their usual method descriptors and function metadata are
included in the immutable realm views and precise intrinsic roots. Their
global bindings are task-owned properties (clj-quickjs-104).

Every call uses `ConvertTarget::NumberParse`: ToString of the first operand
finishes before ToInt32 of the radix begins. OrdinaryToPrimitive calls run
as resumable VM frames with string and number hints respectively. Pending
operands are traced by the existing conversion frame. Extra arguments are
ignored. The parser state owns only immutable `JsString` leaf storage and
Rust scalars/buffers, with no heap edges; unwind and cancellation drop it
through the ordinary native-frame protocol.

`NativeState::NumberParse` scans at most 256 input units per budgeted step,
including leading whitespace. For integers, a fixed 32-limb accumulator
retains the exact magnitude up to 1024 bits; larger magnitudes necessarily
round to Infinity. Each input digit performs at most 32 multiply-adds.
Finalization inspects at most 1024 bits and reuses StringToNumber's
round-to-nearest, ties-to-even top-bits/guard/sticky rounding helper. This
rounds once, avoiding accumulated floating-point multiply-add errors in all
radixes 2..36, including decimal.

For decimals, the StrDecimalLiteral prefix state machine also validates
StringToNumber's full decimal grammar. An incomplete exponent is ignored
only by parseFloat. Decimal accumulation retains at most 1100 significant
digits, an adjusted decimal exponent and a sticky flag for discarded
nonzero digits. Every binary64 rounding midpoint has fewer than 1100
significant decimal digits (it is a dyadic rational with denominator at most
2^1075); appending a sticky digit preserves the side of every midpoint.
Rust's correctly rounded parser therefore receives a bounded decimal of at
most 1101 digits plus its exponent. Exponent saturation exceeds twice
MAX_STRING_LEN so long significands/fractions cannot cancel away an
otherwise saturated exponent. Parser allocation and finalization are
bounded; bulk primitive coercion of a string radix retains the existing
StringToNumber limitation tracked by clj-quickjs-265.

## Accessor properties (clj-quickjs-120)

`Property` keeps the existing data value and flags and adds an optional
getter/setter pair. A present pair identifies an accessor: its unused data
value is undefined and writable flag false. This preserves existing data
storage and makes both callable edges explicit to the collector. Generic
and partial descriptors retain omitted fields; a configurable kind change
resets the kind-specific fields; non-configurable accessor identities use
SameValue. Freeze only removes accessor configurability. Arrays retain an
undefined presence slot in dense/sparse storage and keep accessor pairs in
a sorted side map, alongside their existing exceptional attributes. Every
pair is traced, and index deletion/truncation removes it.

The parser distinguishes contextual `get`/`set` from ordinary data keys,
shorthands and methods. Getter/Setter function kinds reuse the `Method`
closure instruction, strictness, home-object edge, own this/arguments and
non-constructor capability. The appended `DefineGetter`/`DefineSetter`
instructions (`0x67`/`0x68`, name operands, object/function → object) merge
the matching half of a configurable accessor. `DefineField` still replaces
the whole property. Names are inferred with `get ` / `set ` prefixes.

`property::read` returns a value or a getter request; `prepare_put` either
applies a data write or returns a setter request. Both release every heap
lock before returning. The dispatch loop installs a traced `PropertyCall`
native continuation, retaining the original receiver, callback and (for a
setter) assignment value. Its step returns `Action::Call`; the existing
trampoline uses `dispatch::resolve` and `builtin::forward` to enter ordinary
VM frames, including bound and built-in accessors. Getter returns complete
the property read; setter returns are discarded in favor of the assignment
value. Nothing recursively enters the interpreter or retains a Rust stack.
The native frame records the property operation's instruction position;
exceptions unwind through it to ordinary catch/finally handlers. Dependency
waits, budget yields and migration retain ordinary task state. Cancellation
uses the existing native discard path.

ToPropertyDescriptor is now a native cursor (`descriptor::DefineProperty`)
that retains target, attributes and accumulated fields. Each field is probed
and read only when reached, in enumerable/configurable/value/writable/get/set
order. Getter results are validated and stored before advancing; a wait
never replays earlier fields. `getOwnPropertyDescriptor`, keys, for-in,
`delete`, `in` and integrity checks inspect property metadata without calling
accessors. Function metadata reads continue through their property record
so an accessor there receives the original function as this.

OrdinaryToPrimitive distinguishes a getter for a conversion method from the
method call itself. `instanceof` retains its callable and left operand while
reading prototype; a primitive left operand still returns false before that
read. `bind` reads length/name in order, and `apply` and `push` retain their
array-like cursor across length/index callbacks. `apply` stops synchronous
forwarding before list expansion, then asks the trampoline for the final
call. All these continuations trace their receivers, arguments and partial
results and share the native stack/depth limits. `apply`/`push` ToLength uses
the same conversion continuation for object-valued lengths. Ordinary
constructors keep their non-configurable data prototype property; its read
cannot encounter a user accessor in the current object model.

No new heap kind, sharing category or dependency is introduced. Ordinary
objects/arrays remain task-owned, and accessor functions retain their home
object, preventing publication exactly as concise methods do. The built-in
`Object.prototype.__proto__` accessor also uses this storage (clj-quickjs-261).

Verification: frontend accessor AST/error cases; source descriptor validation,
inheritance, integrity, enumeration, function metadata and native-call cases;
`tests/need.rs::getter_waits_on_dependency_and_resumes` forces collection at
every one-instruction yield and while parked, then migrates the resumed run.
Cancellation and dependency failure exercise unwind; GC tests cover accessor
slots and home-object cycles.

### Number radix formatting (clj-quickjs-149)

`value::string::number_to_string_radix` delegates radix 10 to the existing
formatter. Other radixes extract the binary significand and exponent;
integer division uses fixed 34-limb storage, preserving every bit even for
integers above 2^53. Fractional remainders and half-ulp boundaries use the
common exact denominator 2^1075. Each step multiplies them by the radix,
emits one digit, and tests the truncated and rounded-up candidates against
the binary64 rounding interval. Powers of two have a narrower lower boundary
except at the subnormal transition. Midpoint inclusion follows the even
binary significand; if both candidates fit, choose the nearer one, ties to
an even radix coefficient. Carry propagation and trailing-zero removal finish
the output. This is an independent exact-interval implementation, using
`js_dtoa` in `research/quickjs/dtoa.c` at revision `7d1c13c5d8f8` as a
semantic reference for shortest-sufficient output.

This leaf computation is bounded by binary64: at most 1024 integer digits or
1075 fractional digits; every arithmetic pass touches at most 34 u32 limbs.
The buffers contain no collector edges and require no new heap kind.
Receiver validation uses `builtin::this_number_value` before any radix
callback. An object radix enters `native::ConvertTarget::NumberToString`,
retaining the validated f64 and tracing the radix through the existing
conversion operands. OrdinaryToPrimitive uses number hint and ordinary VM
frames, so callbacks can yield, wait, survive collection and migrate.
Converted undefined is explicitly passed through ToNumber to distinguish it
from an originally undefined radix. Invalid radixes use the catchable
`Fault::RangeError` path introduced for array lengths (clj-quickjs-154).

## Math numeric functions (clj-quickjs-152)

The shared realm appends `Math` to `Global` (operand 10) and allocates an
ordinary intrinsic object inheriting from `Object.prototype`, with six
method functions: `floor`, `ceil`, `abs`, `imul`, `fround`, `hypot`.
The existing immutable views, intrinsic roots and persistent publication
rules apply to the namespace and its functions. No new heap kind or opcode
is introduced. It is neither callable nor constructible.

Fixed-arity methods run as leaf built-ins for primitive operands. Objects
enter `native::Convert` with number hint and a Math completion target;
each operand is converted to a Number before moving to the next argument.
The existing traced operand vector, callback VM frames, depth limit and
unwinding protocol preserve state through waits, GC, yields and migration.
`imul` reuses the VM's ToInt32 bit conversion and wrapping multiplication
(the low 32 bits are identical for ToUint32). `fround` casts to binary32
and back to binary64; floor, ceil and abs use the IEEE Rust operations.
In particular floor(-0.5) is -1, following the user-approved acceptance
correction of 2026-10-24; ceil(-0.5) and floor(-0) preserve negative zero.

`hypot` always enters the conversion continuation, even for primitives,
and converts one argument per native step while finding the largest
absolute value. Only after all conversions succeed does it transition to
`MathHypot`: Infinity returns +Infinity even with NaN inputs; otherwise
each budgeted step normalizes one argument by the largest magnitude and
adds its square with Kahan compensation. The final result rescales the square
root. Zero terms are skipped, giving +0 for empty/all-zero inputs, while
NaN terms propagate. Scaling keeps finite squares/sums in range even for
extreme/subnormal input magnitudes. Initial argument-vector copying is
bounded by the VM value-stack limit, as for existing built-ins.

Source tests cover the upstream numeric examples, descriptors, binary32
ties, signed zeros, missing/unused arguments, throws and coercion order,
including conversion getters, fallback methods and throws that wait with
collection and migration at every yield.
GC inventory tests include the namespace and all six functions. Math
constants and other methods are outside this pellet.

## Math.pow and shared exponentiation (clj-quickjs-164)

The realm registers a seventh Math function, `pow` (length 2), using the
existing method flags and intrinsic rooting. `builtin::math` and the Pow
opcode both call `dispatch::exponentiate`, keeping Number::exponentiate
semantics in one place. Its existing `f64::powf` guard returns NaN for
absolute base 1 and a non-finite exponent, as in QuickJS's `js_pow`.
Object arguments enter the existing Math conversion continuation with two
operands and number hint, so conversion remains ordered and traced through
yields, dependency waits, collection and worker migration. Primitive calls
use the fixed-arity leaf path. No new opcode or heap kind is needed.

## Task-owned global objects (clj-quickjs-104)

Contract: `realm::new_global_object`, `realm::declare_global`,
`VmState::global_object`, and the global instructions in `vm::instr` and
`vm::dispatch`. This complements the shared immutable intrinsics above.

**Decision.** Each task lazily creates one ordinary mutable global object,
owned by its task ID and inheriting from the shared `Object.prototype`.
The task retains it across budgeted runs, calls, waits and worker migration;
it is released with VM state at completion or cancellation. It is never
worker-local or implicitly shared between tasks. The same immutable code
can run in multiple tasks with separate globals. Currently each task runs
one script (or entry function); admitting further scripts and eval into an
existing declarative environment is deferred.

`globalThis` is initially a writable, non-enumerable, configurable own
property referring to the object. Script top-level `this` reads the object
itself, including after replacing that property. Each built-in in
`realm::Global::ALL` is installed as a writable, non-enumerable, configurable
own property whose initial value is the shared intrinsic; replacing the
binding does not mutate the intrinsic. `undefined`, `NaN` and `Infinity`
are fixed own properties (source still reserves those identifiers).

Script `var` and function declarations are global object bindings.
Instantiation runs before body statements, creating non-configurable,
enumerable, writable own properties; a `var` reuses an existing own
property without changing its descriptor or value. A function declaration
checks restricted properties before any closure is created, then installs
its closure. The compiler rejects conflicts with that script's lexical
bindings. A function declaration against a non-configurable property that
is an accessor or is not writable and enumerable raises a catchable run-time
`SyntaxError`.
Script `let` and `const` retain the existing lexical frame/cell machinery
and never become global object properties.

Unbound identifiers and references to script object bindings use
`get_global`, `get_global_or_undefined` (`typeof`) and `put_global`.
Every access walks current properties; no global value is cached in code,
locals, captures or a function's self-reference. Descriptor replacement,
read-only strict writes and ignored sloppy writes use the existing
property implementation. Getter/setter calls receive the global object,
including for inherited accessors, through the traced `PropertyCall`
continuation and ordinary VM frames from clj-quickjs-120. `typeof` also
invokes getters. Setter return values are ignored; assignment returns its
right operand. No Rust frame survives a callback wait. Missing reads and
strict writes raise `ReferenceError`; sloppy writes create configurable
properties, and sloppy identifier deletion uses the same property path
(clj-quickjs-105).
`get_builtin_global` remains available to hand-assembled code, but source
identifiers no longer use it.

**Publication consequences.** A closure that reads a global captures no
mutable realm state, and can be published if its other captures permit it.
When called in another task it reads that receiving task's globals, never
the producer's. A lexical capture retains its existing value/cell rules:
a cell remains unpublishable. A captured global object (including a
script-level arrow's lexical `this`) is an ordinary task-owned object, so
transitive publication rejects it. This is intentional task-relative
execution rather than a shared JavaScript global environment.

**Alternatives.** An explicitly concurrent global reference could preserve
one mutable environment across tasks, but would require defined atomic
property/descriptor operations and a callback protocol that never runs
JavaScript under its synchronization. Nothing currently requires that
sharing, so it would add synchronization and semantics without a client.
Capturing the producer global object in every closure would preserve its
environment but prevent publishing any global-reading function; silently
sharing it would violate `design.md`. Duplicating all intrinsics per task
would lose their shared identities and add allocations; the mutable binding
overlay preserves both task isolation and shared immutable intrinsics.

**Collection and verification.** `Trace for VmState` reports the global
object even when no operand or local holds it. Ordinary-object tracing
covers property values, keys, the prototype and the `globalThis` cycle;
there is no new heap kind, root category or publication exemption.
`tests/gc_roots.rs` checks retention and reclamation after dropping or
finishing a task. `tests/js_functions.rs` checks receiving-task lookup,
publication rejection for a captured global, and one-instruction runs with
collection and migration between every slice. `tests/js_source.rs`
`global_object_bindings` checks source semantics and data/accessor descriptors.
`global_object_descriptors` checks every initial global binding's value and
attributes, and all three attributes of script var/function properties.

`tests/need.rs::global_accessors_wait_collect_and_migrate_without_replay`
checks getters, setters, inherited accessors, `typeof`, updates and throws
with collection at every one-instruction yield and while parked, then
resumption on another thread with exactly-once effects. Sloppy non-object
`this` substitution remains the existing clj-quickjs-116 gap.

QuickJS references and provenance: `research/quickjs/quickjs.c`,
`JS_CheckDefineGlobalVar`, `OP_get_var`, `OP_put_var`, and
`research/quickjs/tests/test_language.js`, `test_global_var_opt`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The descriptor sequence in
`tests/js_source.rs::global_object_bindings` adapts the upstream sequence:
`Object.defineProperty` creates the initial configurable property instead
of indirect eval; Rust assertions check script completion values. The
identifier-delete group is covered separately by clj-quickjs-105. The test carries the QuickJS MIT notice.
No upstream implementation code is copied.

## Stored prototype accessor (clj-quickjs-261)

The shared immutable realm stores `Object.prototype.__proto__` in the ordinary
accessor representation, with two appended `Builtin` variants for `get __proto__`
and `set __proto__`. Both functions are intrinsic GC roots, inherit from
`Function.prototype`, and appear in the immutable realm views. Getter/setter
slots in those views are checked to contain only intrinsic references.

The generic accessor call path handles inherited access and extracted calls.
There is no name-based hook in property reads, setter lookup, descriptor
reflection, deletion or enumeration. Before creating a function property record
or rejecting an intrinsic data write, setter lookup inspects the receiver and
its chain; this preserves ignored assignments without mutation or lost
publishability. An ordinary object's own data property still shadows the
accessor, and a chain ending before `Object.prototype` has no inherited accessor.

The getter uses `[[GetPrototypeOf]]`; number/string primitives return the fixed
prototype of their wrapper without allocating it. Boolean getter receivers
remain unsupported until `Boolean.prototype` exists. For primitive writes,
Boolean's supported inherited setters can be found from `Object.prototype`,
since Boolean's standard own prototype properties are data properties. The
setter requires a coercible receiver, ignores a non-object/non-null value or
primitive receiver, and otherwise calls the existing ownership/cycle/immutability
checked `set_prototype_of`. Neither built-in invokes JavaScript. Literal
`__proto__: value` continues to use `SetProto` separately.

The descriptor's configurable flag does not grant permission to mutate shared
intrinsics. Redefinition and deletion still throw TypeError. Tests cover stored
reflection, aliases, extracted calls, primitive receivers, unchanged prototype
reflection, cross-task link rejection, and publication/collection/migration of
the built-in functions. QuickJS `js_object_get___proto__`,
`js_object_set___proto__` and `js_object_proto_funcs` in
`research/quickjs/quickjs.c`, revision `7d1c13c5d8f8`, were consulted for semantics;
no source or tests were copied.

## Array string conversion (clj-quickjs-156)

`Array.prototype.join` and `toString` are realm built-in functions executed
through task-owned native continuations. Join retains its receiver, separator,
snapshotted ToLength, index, stage and accumulated immutable string parts.
Length/index getters and OrdinaryToPrimitive run as ordinary VM frames;
ToNumber/ToString subcontinuations return into the saved stage without replay.
The receiver and separator are traced roots; string parts own immutable Arc
bodies without heap edges. Number/String primitive receivers are boxed using
the existing wrapper representation. Boolean wrappers remain unsupported.

Each join step processes at most one element. It checks the running UTF-16
length as each separator/element is appended and rejects overflow above
`2^30 - 1` with RangeError `"string too long"`. Completed parts move into the
existing flat `StringBuild`, with their already checked total length, avoiding
another linear scan. The builder reserves the final output once, then copies
at most 256 units/part boundaries per native step. Its consuming iterator
releases each part as it finishes, and transfers its Vec into the immutable
string without a final copy. Parts metadata grows amortized linearly; copying
string content is linear, with no repeated prefix concatenation. Allocation
and metadata-vector growth remain synchronous, as with other native builders.

Array `toString` reads the dynamic `join` property, including accessors, then
calls it with the receiver and no arguments; a non-callable value selects
intrinsic Object.prototype.toString (the existing Array tag). Both calls use
ordinary resumable dispatch. Cyclic arrays keep recursing through explicit
native frames, bounded by `VmLimits.max_call_depth`, producing the catchable
StackOverflow failure required by this pellet. There is no Rust recursion,
cycle suppression, or empty-string substitution for cycles.

## Sloppy implicit globals and identifier deletion (clj-quickjs-105)

`PutGlobal` rejects an absent name only in strict code. Sloppy code uses
the existing ordinary property Set path to create a writable, enumerable,
configurable property; inherited setters, read-only properties and a
non-extensible global object keep their ordinary sloppy-write rules.
Callbacks still run through `PropertyCall` and task-owned VM frames.

The parser accepts `delete IdentifierReference` only in sloppy code. It
restores reserved constant operands (`undefined`, `NaN`, `Infinity`) to
identifier nodes for this operation, preserving parentheses and comments.
The lowerer's out-of-line helper resolves the identifier: a declarative
binding gives constant `false` without a read or TDZ check; a global
binding emits `GetGlobalObject`, a name constant and `Delete`. No opcode
or heap kind is added. Global deletion therefore uses the existing owner
checks and configurable flags and never invokes a getter. The object is
read directly from task state, even if its `globalThis` property changed.

`tests/js_source.rs::sloppy_implicit_globals_and_delete` uses synthetic
fixtures for creation, descriptors, deletion/recreation, strict failures,
declarative bindings (including TDZ), inherited properties and getters.
Semantic references: `research/quickjs/quickjs.c`, `OP_put_var`,
`OP_delete_var`, and `JS_DeleteGlobalVar`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. No upstream code or tests
are copied into this pellet.

### Error instances (clj-quickjs-228)

The realm appends `Error`, `Error.prototype` and its `toString` method to
its immutable intrinsic table. Error instances use the ordinary object's
internal `error_data` marker, independent of their mutable properties and
prototype. No new heap kind or publication category is introduced: instance
properties and prototype edges are traced by the ordinary object tracer,
and instances remain task-owned, rejected as dependency failure values.
`src/vm/native/error.rs` retains the partially constructed instance, options,
receiver and converted name across ordinary VM callback frames. ToString
uses the existing conversion continuation with string hint; cause uses
HasProperty followed by a resumable property read. Uncaught diagnostics
inspect data properties without invoking getters or object conversions.
Engine errors use the same ordinary storage with an additional internal
diagnostic slot (clj-quickjs-230, below).

QuickJS references and provenance: `research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `js_error_constructor`,
`js_error_toString`, `js_error_proto_funcs` and `JS_NewCConstructor` were
consulted for semantics. Implementation and synthetic source tests were
written fresh; no QuickJS source or test text was copied.

## String splitting (clj-quickjs-157)

`String.prototype.split` is appended to the realm as a non-enumerable method
with length 2. `ConvertTarget::StringSplit` validates RequireObjectCoercible
before entering resumable OrdinaryToPrimitive: receiver with string hint,
limit with number hint and ToUint32, then separator with string hint. Flags
preserve the distinction between an originally undefined operand and an
object returning undefined. Even a zero limit observes separator coercion.
Objects use ToString; RegExp and `Symbol.split` dispatch remain deferred.

`NativeState::StringSplit` traces its fresh task-owned array while
`value::string::StringSplit` retains only immutable string leaves, KMP search
indices/prefix storage and an incremental string builder. Each step scans
at most 256 comparisons, copies at most 256 units/boundaries, or appends one
field with CreateDataProperty semantics through the existing array-literal
append helper. The search prefix table is reused between non-overlapping
matches. Empty separators emit one UTF-16 unit per step; undefined separators
reuse the receiver's immutable body. Limits stop work immediately after the
requested number of fields. Results have Array.prototype and ordinary
writable/enumerable/configurable elements; no heap kinds or publication
categories change. Allocation itself and bulk primitive coercion retain
the existing limitations documented for string built-ins (clj-quickjs-265).

Tests cover all 20 upstream rows with length, every element and join, limit
wrapping, empty inputs, surrogate pairs, conversion order and throws,
forwarding, callback dependency waits, collection and thread migration
during coercion, scanning, copying and result-array construction.

## Error stack capture (clj-quickjs-59)

`VmState::error_stack` snapshots at most 64 explicit JS frames before an
Error constructor starts its resumable conversions, or before a catchable
engine failure unwinds. It uses the same instruction/source-location table
and saved caller PCs as diagnostics. Native callbacks keep the invoking JS
operation as their caller position. Compiled debug metadata retains the
script filename separately from each function's name, so nested closures
and separately compiled scripts keep the right origin. Names use immutable
compiled metadata; diagnostic capture never runs JavaScript.

The snapshot is an ordinary immutable `JsString` leaf buffer with no heap
edges, using the existing coarse immutable Arc boundary for strings. It
retains no frame, function object, source text or task; creation cost is
bounded by 64 frames and the names written. Constructed Errors store it as
an ordinary non-enumerable property, as do engine errors (clj-quickjs-230).
Engine error slots retain and trace their original cause even if the own
`cause` property changes or is deleted. No worker-local state or system-stack walk is involved, so
waiting, migration and collection need no additional roots or protocol.

## Array construction and brand (clj-quickjs-153)

The Array built-in uses one leaf implementation for calls and construction,
including forwarding and bound constructors. A single Number is checked by
exact equality with its uint32 conversion; invalid lengths return the same
catchable RangeError fault used by ArraySetLength. Array arguments never
require ToPrimitive, so no JavaScript callback or continuation is needed.
Element copying is bounded by the VM's argument/value-stack limit; numeric
length construction takes constant time and storage, including `2^32 - 1`.
`ArrayObject::with_length` records holes as a logical length with empty
dense and sparse stores, and subsequent large-index writes use sparse storage.

The result is allocated as the same task-owned, precisely traced Array
heap kind as a literal. Arguments remain rooted until the completed array
replaces the call operands; no safepoint occurs inside the leaf. Ownership,
publication rejection, element tracing and mutation rules are unchanged.
The immutable empty-array intrinsic `Array.prototype` retains ordinary
property storage; its realm identity supplies the array brand for isArray
and Object.prototype.toString. All mutable arrays use the Array heap kind.
This preserves existing realm views and publication categories without
adding an intrinsic array variant or changing append-only registries.

## NativeError instances (clj-quickjs-229)

The realm appends seven constructor/prototype pairs, rooted in its existing
`all` inventory and immutable views. `Builtin::NativeError(kind)` selects
the prototype for Error's existing native constructor continuation; the
allocated instance uses the same ordinary-object error marker, tracing and
task ownership. Each constructor inherits from Error and each prototype
from Error.prototype, sharing its resumable toString implementation.
No new heap kind, publication category or callback mechanism is introduced.
Engine-raised errors use these prototypes too (clj-quickjs-230).

## Engine Error instances (clj-quickjs-230)

`raise_failure` allocates an ordinary object only when a handler could see
a catchable error. Its prototype is the realm's ReferenceError, TypeError,
RangeError or SyntaxError prototype, or InternalError for stack overflow;
dependency failures and cancellations inherit from Error.prototype.
Own message and stack properties use writable, configurable, non-enumerable
attributes. Stack overflow exposes `"stack overflow"` to JavaScript while
the embedder retains its detailed VM limit diagnostic. Failed dependencies
also get an own cause holding the published value by identity.

An optional boxed engine slot in `OrdinaryObject` holds the original
`VmError` and cause. The collector traces that cause independently of the
ordinary properties, so replacing or deleting cause cannot change
`Rooted::error_cause`. Rethrow reads this slot and preserves kind, position
and call chain without reading properties or invoking JavaScript. Explicitly
constructed Error instances have no such slot (`Rooted::error` returns None).
All instances are task-owned: publication rejects them directly, as failure
values or through closure captures, even after freezing. This intentionally
replaces the previous persistent native engine-error category; existing
dependency and end-to-end tests return host roots without publishing them.

Allocation and property-key exhaustion produce terminal OutOfMemory. The
new value reaches the catch operand or pending completion before any poll;
native-frame and cancellation cleanup unwinding use the existing path.
Getters, conversions and inherited toString use ordinary resumable frames.

QuickJS references and provenance: `research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `JS_ThrowError2` and
`JS_ThrowStackOverflow` were read for semantics. Implementation and synthetic
tests were written fresh; no upstream source or test text was copied.

QuickJS references and provenance: `research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `js_error_constructor` and
`js_native_error_proto_funcs` were consulted for semantics. Rust code and
synthetic fixtures in `tests/js_source.rs` were written fresh; no upstream
source or test text was copied.

## Math.sqrt and Math.log2 (clj-quickjs-205)

The realm adds two rooted built-in functions to its existing Math namespace.
`Builtin::MathSqrt` and `Builtin::MathLog2` are appended to the built-in enum;
there is no new heap kind, opcode, or global slot. Both have name/length
metadata and the existing method descriptor flags. Primitive operands use
`builtin::math`; object operands enter the existing traced `native::Convert`
continuation with a number hint, so getters and coercion callbacks may wait,
yield, collect and migrate without retaining a Rust frame.

Square root uses `f64::sqrt`, which is correctly rounded and preserves -0.
Log2 detects positive finite powers of two directly from binary64 exponent
and significand bits. Normal powers return exponent minus 1023; subnormal
powers return the lone significand bit's position minus 1074. Other inputs
use `f64::log2`, including its IEEE special values. Non-power-of-two accuracy
is platform/Rust-version dependent, not a claim of correct rounding; source
tests pin log2(10) to 3.321928094887362 and ceil(2000 * log2(10)) to 6644.
Source tests check every representable power of two, edge cases, descriptors,
conversion failures, and coercion across waits/GC/worker migration. The realm
and GC inventory expectations increase by exactly two intrinsic functions.

### Budgeted JSON parsing (clj-quickjs-159)

The append-only realm global registry includes the shared immutable `JSON`
ordinary object and its `parse` built-in. A task can shadow or replace its
own global JSON binding. The parser introduces no heap kind: produced
objects/arrays use the existing task-owned stores, prototypes, creation-order
keys and publication policy. CreateDataProperty APIs bypass inherited
setters, including Object.prototype's `__proto__` accessor.

`src/vm/json.rs` is an iterative grammar machine held by a native frame.
Its explicit stack holds at most 512 open containers; the native tracer
reports every container and the completed root while trailing whitespace is
scanned. ToString runs through an ordinary conversion continuation, so
getters and conversion methods may wait, yield, collect and migrate. Parsing
then calls no JavaScript. Cancellation and errors discard the continuation
under the usual native-frame protocol.

One parser step performs at most 256 grammar/scan/decode operations (each
escape reads at most six code units) and at most 256 decimal-accumulator
units. Strings are validated first, then decoded in bounded chunks into a
buffer reserved once from the token's raw length. The final Vec transfers
into immutable JsString storage without shrinking or copying. Numbers use
clj-quickjs-148's correctly rounded NumberParse over a delimited range of the
original immutable input, with no token copy and at most 1100 retained
significant digits plus sticky rounding. Allocations and ordinary property
insertion remain leaf operations. Member names are limited to 4096 decoded
UTF-16 units, checked incrementally while scanning, so the shared key
interner hashes/compares a bounded key. Exceeding that resource limit throws
SyntaxError; string values are unaffected. Object/array storage and the key
table may grow, as in source literals.
SyntaxError messages retain JSON input line/UTF-16 column/offset independently
of the VM call diagnostic. The engine's existing catchable error path is
used; NativeError constructors and engine-error prototypes are separate work.

## Tagged template objects and String.raw (clj-quickjs-58)

The parser retains an optional tag and optional cooked values on template
AST nodes, rejecting invalid escapes only for untagged templates. Out-of-line
parser/lowerer helpers keep recursive stack use bounded. Lowering evaluates
the tag reference with the ordinary member-call receiver rule, pushes
interleaved cooked/raw primitive constants, emits TemplateObject (0x6F, u16
part count), then substitutions unchanged and Call/CallMethod.

A realm caches template objects by retained Arc<Code> identity and instruction
index; retaining code prevents address reuse. Calls through different closures
and published closures in other tasks reuse the same site's object. Entries
are Intrinsic roots until realm teardown, even after all tasks and executable
handles are dropped. Shared code remains immutable primitive-only Arc storage,
usable in other runtimes with independent realm caches; no heap edges or
mutable caches are attached to code.

A short metadata mutex serializes lookup and first allocation/installation.
No JavaScript, native callbacks or safepoints run under it. The collector traces
the cache and releases its lock before taking the object-table lock, matching
the mutator's cache-to-object lock order. This serializes only template lookup
and creation, not interpreter execution. The cache retains code and its template
arrays for the realm lifetime; it does not retain closures or task state.

TemplateObject validates primitive part types even for hand-loaded code,
constructs arrays only on the first evaluation, freezes both before exposing
them, and records the cooked array before the next safepoint. ArrayObject's
internal template constructor uses the ownerless intrinsic ordinary store;
only these internal frozen arrays classify as Persistent, while user-frozen
arrays remain TaskOwned. Existing tracing follows elements, raw and intrinsic
prototype edges. Normal frozen descriptors govern writes, deletes and defines;
literal append explicitly rejects these arrays because it bypasses descriptors.
No new heap kind or publication category is introduced.

String.raw is appended to the built-in registry and installed on the realm's
String constructor. A native frame traces raw and substitutions, saves each
property-read/coercion stage before calling JavaScript, and passes completed
parts to the existing bounded StringBuild copier. Tests cover identity,
descriptors, publication of arrays/closures, and one-instruction yields with
collection and thread migration through five dependency waits.

Semantic references: research/quickjs/quickjs.c, js_parse_template and
js_string_raw, revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2. All new
implementation and synthetic fixtures were written fresh; no upstream source
or test text was adapted.

## Symbol primitives and property keys (clj-quickjs-127)

`Value::Symbol(JsSymbol)` is a primitive over an immutable `Arc<SymbolData>`
leaf, outside the traced heap, using the same coarse immutable storage boundary
as strings. The body holds a process-wide, never reused u64 identity and an
optional `JsString` description, with no heap edges. A relaxed atomic counter
only issues identities; publication of the immutable body uses the existing
value handoff. Exhausting the identity space fails instead of wrapping.
Cloning preserves identity. Equality, SameValue, SameValueZero and persistent
map hashing use that identity, including after publication through an atom.
A description is diagnostic text, never the symbol's identity.

The alternatives were a traced heap kind (requiring rooting for a primitive
with no heap edges), and a runtime-only identity table (preventing independent
host ownership). An immutable leaf keeps `Primitive::Symbol` root-free,
shareable by construction, and safely transferable between executors. The last
value releases an ordinary symbol's body and description, unless it has been
used to define a property: the runtime's property-key table retains those
symbols until heap drop, just as it retains interned string names. The shared
u32 key space bounds the table to 2^32 entries; reads of absent symbols do not
intern. This follows the existing key-table lifetime policy rather than
introducing a second collector. The three well-known bodies (`iterator`,
`toPrimitive`, `hasInstance`) live in process-wide OnceLocks and are shared by
all realms and tasks until process exit.

`share::classify` still exhaustively categorizes heap kinds; symbols introduce
no heap kind or trace edge. `Trace for Value` skips them as primitives. There
is no new root category or holder in the GC inventory. The `Symbol` function
is an ordinary Persistent built-in under the existing Intrinsic root category;
realm retention/reclamation tests include it. Its three well-known properties
are non-writable, non-enumerable and non-configurable.

`PropertyKey` remains a private u32. `PropertyKeys` maps string contents and
symbol identity through separate indexes into one key space; reverse entries
hold either a string or a symbol. `property::Key::Symbol` survives ToPropertyKey
unchanged, including object conversions that return a symbol, and uses the
ordinary object's existing descriptor store. Arrays and function property
records use the same path. Internal OwnPropertyKeys order is array indices,
other string keys, then symbols, with creation order in each latter group.
Object.keys, Object.getOwnPropertyNames and for-in exclude symbols.

`Symbol()` creates an absent description; a non-undefined argument undergoes
ToString. Object descriptions use the existing native conversion continuation
with string hint, including resumable getter/method calls and a distinct
completion target so an object returning undefined becomes the description
"undefined". New Symbol throws TypeError. ToBoolean is true; typeof is "symbol";
implicit ToString and ToNumber throw catchable TypeError at the source operator.
String(symbol) alone uses the descriptive `Symbol(description)` string without
losing UTF-16 code units; String(object) still uses ordinary ToString, even
when that object's conversion returns a symbol.

Symbol.prototype, wrappers, further well-known symbols
and well-known protocol dispatch are deferred to their own pellets. This change
exposes the three identities without changing iteration, instanceof or
OrdinaryToPrimitive protocol lookup.

Numeric binary conversions complete ToNumber of the left primitive before
converting the right object. This matters when the left operand is a symbol,
including an object that returns one: TypeError prevents any right-hand
conversion callback or wait. The native ToNumberPair continuation stores each
converted Number before advancing. Plus and relational comparisons preserve
their distinct rule of first obtaining both primitive operands.

### Budgeted JSON serialization (clj-quickjs-160)

The realm's JSON namespace appends a `stringify` method without changing
existing global or built-in identities. `src/vm/json.rs::Stringifier` is a
native continuation with an explicit stack of container values, array
length/index cursors or enumerable own-key snapshots, the current value and
member name, and a private ObjectId hash set for the active path. It adds no
heap kind and never recurses on the Rust stack. The native tracer roots the
current receiver/result and every open container; the identity set contains
only objects already rooted by those containers. Paths are removed on close,
so shared subobjects serialize repeatedly while cycles throw TypeError.

Property reads (including `toJSON` accessors), method calls and wrapper
coercions use ordinary VM callbacks/conversion frames. The saved stage makes
these effects occur once across budget exhaustion, dependency waits, GC and
worker migration. Errors/cancellation discard state through the existing
native-frame protocol. Objects snapshot keys through `vm::keys`; arrays
snapshot length once and stream ordinary index reads. Task ownership is
checked for direct values and `toJSON` results, including empty arrays.

Each native transition does one property/structural operation, quotes at
most 256 input code units, or copies at most 256 final output units/part
boundaries. Quoted chunks hold at most 1536 units, so their allocation and
transfer have bounded size, even for long escaped strings or member names.
Output length is checked incrementally. Chunks and punctuation are retained
as immutable JsString parts, then the existing StringBuild reserves once and
fills the final body in bounded steps, transferring its Vec without a final
linear copy. Container/part vector growth and allocator calls remain leaf
operations, as in other native builders.

Until own-key enumeration itself is resumable, ordinary objects are limited
to 4096 stored properties: a bounded preflight scan rejects excess before
calling its leaf scan/sort. Member names are capped at 4096 UTF-16 units before
property lookup to bound shared key hashing. Traversal caps open containers
at 512. Number-wrapper ToPrimitive uses a limited-number conversion target:
after its observable callbacks complete, string results longer than 4096
UTF-16 units throw RangeError before the shared leaf StringToNumber scan.
This bounds whitespace scanning, grammar validation, radix/decimal parsing
and copying without changing other users of the shared ToNumber path.
Limits throw catchable RangeError and are independent of native stack size;
array lengths and quoted string values retain their underlying limits.
Unsupported replacer and space arguments fail explicitly. Boolean wrappers
remain an absent prerequisite. Symbol primitives follow the omission rules;
enumerated symbol keys are filtered before any value read. The bounded
property preflight includes stored symbol keys, keeping the shared leaf
enumeration bounded even when none of those keys contribute JSON members.

## Runtime symbol registry (clj-quickjs-128)

Each shared `Heap` owns one `SymbolRegistry`, shared by every task, executor
worker and world of its runtime. It contains a UTF-16-content-keyed map to
`JsSymbol` and a reverse map from private symbol identity to key. One Rust
Mutex linearizes lookup/insertion and synchronizes publication of immutable
symbol bodies (unlock/relock supplies release/acquire synchronization).
The identity allocator still uses Relaxed ordering solely for uniqueness.
Hashing and equality use immutable strings and private u64 IDs, never
user-defined comparison. This leaf lock is never held with other heap locks,
across a safepoint, or while executing JavaScript or host code. It serializes
registry operations only, not VM execution. Poisoning fails closed rather than
exposing partially inserted forward/reverse entries.

The registry strongly owns every registered symbol body and its key until
the heap drops, even after all tasks and returned handles release them. Keys
and symbols are immutable Arc leaves with no heap edges, so entries are not
traced GC roots and need no new root category; they survive every collection
through Rust ownership. Dropping the registry releases its ownership; an
independently held primitive can outlive it as before. Registries are local
to runtimes; a foreign registered symbol passed as a host primitive has no
entry in another runtime's reverse map. Well-known symbols remain process-wide.

The alternatives were a process-global mutable registry (unbounded retention
beyond runtime drop) and per-task tables (incorrect worker/task identity).
The existing heap ownership supplies the runtime boundary without adding
per-worker imports or a global interpreter lock. Entries are append-only and
currently unbounded, following property-key table lifetime policy.

`Symbol.for` and `Symbol.keyFor` append to the built-in registry and are realm
methods with the existing METHOD flags. The leaf `for` path completes primitive
ToString before locking. Object keys use a distinct native conversion target
with string hint; getters and conversion methods are ordinary resumable VM
frames, including through dependency waits, GC and migration. The completed
primitive undergoes ToString before registry access, so returning a symbol
throws TypeError and returning undefined names the string "undefined".
`keyFor` validates its argument without coercion before looking up its identity.
Symbol.prototype and well-known protocol integration remain separate work.

## JavaScript Map objects (clj-quickjs-169)

`HeapObject::JsMap` holds a boxed `object::map::MapObject`, distinct from
`HeapObject::Map` (the immutable persistent HAMT). It embeds an ordinary
object for ownership, prototype, own properties and extensibility, plus an
insertion-ordered vector of optional key/value entries and a private hash
index from hash to entry positions. Only the owning task reads or mutates
it, under the same short object-table lock as ordinary objects; publication
classifies it as `TaskOwned`, including after `Object.freeze`. Freezing
properties does not freeze the Map's internal entries.

The hash and SameValueZero helpers are shared with the persistent HAMT:
numbers compare by value, all NaNs compare equal, and signed zeros share a
hash; stored zero keys are normalized to positive zero. Strings compare by
UTF-16 contents, objects by stable heap identity (generation included).
Hash collisions always compare actual keys, with no user code, coercion or
property access. The heap is nonmoving and live identities are never reused.
Symbols compare by their stable primitive identity (clj-quickjs-127).
BigInt keys compare by mathematical value, independently of literal,
conversion or arithmetic construction (clj-quickjs-171). Their canonical
`JsBigInt` representation makes derived equality and hashing consistent:
inline integers hash their value; heap-backed `Arc<[u32]>` bodies hash sign
and limb contents, never allocation identity. BigInt and Number keys stay
distinct, and BigInt negative zero is already canonical zero. The shared
key helpers invoke no JavaScript and allocate no limb copies. Hashing and
worst-case equality are O(limbs), at most 32,768 base-2^32 limbs under the
existing 1 Mbit cap, within a leaf Map call; there is no per-limb budget
charge or yield poll. The persistent HAMT key contract is unchanged.
Keys of future primitive kinds must extend those helpers consistently.

Updating an existing key preserves its slot. Deletion empties the slot;
reinserting appends a new one. `clear` empties every slot without truncating
the vector. There is deliberately no compaction: positions remain stable
for later live iteration, including deletions, clearing and appends during
a callback. Tombstones hold no key/value edges, though their slot storage
lasts until the Map dies. `Trace` reports the ordinary object's edges and
both key and value of every live entry. The root inventory test
`map_entries_are_traced` covers result-only retention and reclamation.
`map_properties_and_prototype_are_traced` checks the independent ordinary
property and custom-prototype edges with only the Map result rooted.

The realm appends an immutable ordinary `Map.prototype`, constructor,
methods and `size` accessor. Leaf calls never invoke JavaScript. Constructing
with a non-nullish argument explicitly throws TypeError until resumable
iterable construction exists. Iteration methods belong to later pellets.
`map_bigint_keys` covers fresh equal keys, updates, membership and deletion
for 1000 inline and 1000 larger-than-128-bit values.
`map_bigint_for_each_deletes_all_keys` covers the upstream callback deletion
loop for both key ranges, checking insertion order, membership and deletion
until all 1000 callbacks have run and the Map is empty (clj-quickjs-171).
The prototype also owns the non-writable, non-enumerable,
configurable `Symbol.toStringTag` value "Map"; the shared well-known identity
is exposed on `Symbol`.

## Live resumable Map forEach (clj-quickjs-170)

`MapForEach` appends a built-in identity and a non-enumerable, writable,
configurable method (length 1) to `Map.prototype`. Its native uses the
existing `vm::native::iterator::State` and native trampoline, with a Map
entry stage and a stable entry index rather than a separate cursor object.
The receiver is validated before the callback's callability, even for an
empty Map. The state traces the Map, callback and `thisArg` through the
existing native-frame root path.

Each budgeted step reads exactly one current entry slot under a short heap
read, advances the index, and releases the read before returning a callback
action. Tombstones consume a step but invoke nothing. Current keys/values
move to the ordinary callback arguments; callback return values are ignored.
After a callback, the next native step re-reads the Map, observing changed
values, deleted entries and appends. `clear` leaves tombstones, so pending
entries disappear while later appends are visited. No entry compaction ever
occurs, including while a cursor is suspended; no cursor remapping or active
iteration counter is needed. The Map retains tombstone slot storage until it
dies. Exceptions and cancellation use the existing native unwind/drop path.

`map_for_each_live_iteration` covers live mutation and the adapted 1000-entry
deletion loop. `map_for_each_suspends_in_callback` waits on the second entry,
collects while parked, migrates to a new thread, and finishes under one-step
budgets without replaying effects, including an entry appended after resume.

## Rope strings (clj-quickjs-146)

`JsString` keeps the existing coarse immutable Arc storage boundary. Each
body stores length, either flat units or immutable string children, and a
`OnceLock<JsString>` cache holding only a flat leaf. There are no heap edges or new publication
categories. Above 32 units, concat allocates one node and retains its two
operands, with no prefix copying or balancing. Appending/prepending N single
units copies only 527 units during short concatenations (2 + ... + 32),
then N at the first flatten. The deterministic measurement test bounds this
by 2N for N = 100,000, checks every code unit in JavaScript, and verifies
that subsequent indexed reads add no copies. `JsString::copied_code_units`
is a thread-local diagnostic counter, including native string-builder copies;
input construction and one-unit indexing results are excluded.

`code_units()` visits cached subtrees or children with an explicit DFS stack
and caches only the requested string. Comparison, hashing (including map and
property keys), search, formatting and index access all use this entry point.
Length and out-of-bounds index checks do not flatten. Concurrent first readers
initialize one cache. Retained prefixes remain immutable and independently
usable; flattening never changes children. The requested body retains its
cache and children, but subsequent concatenation reuses the cached flat leaf
instead of retaining that body's obsolete graph. Thus reading every growing
prefix does not keep every previous prefix buffer alive. A regression test
asserts that both append/read and prepend/read loops retain at most 2N units.

Rope depth is intentionally unbounded. Traversal and destruction are iterative:
`Body::drop` removes children and drains uniquely owned bodies through
`Arc::into_inner`, stopping at shared subtrees. This also avoids recursive
last-release races between threads. A 64 KiB stack test builds, flattens and
drops 100,000-node chains in both directions, including retained prefixes and
unread chains. Shared DAGs can represent `2^30 - 1` units without allocating
that buffer; concatenations above the limit fail before flattening.

Flattening remains synchronous O(length), using O(depth) temporary pointers
and O(length) output storage. It does not resume against the instruction
budget; allocation, first hash/comparison/index access and eventual release
can delay yields. Subsequent contiguous access is O(1). Existing incremental
search/copy natives retain their bounded scan/copy steps after that first
access. String.prototype.concat assembles at most 256 whole parts per native
step, preserving observable conversions in the existing continuation.

The unchanged upstream `rope_concat`/`test_rope` functions run in
`tests/fixtures/js/quickjs_rope.js`, invoked by `upstream_string_rope`.

## Regular expression pattern IR (clj-quickjs-186)

The engine is in-tree: its parser, IR and eventual backtracking matcher belong
in `src/regexp/`. No regex crate or C engine is used. ECMAScript backreferences,
lookaround, Annex B escapes, UTF-16 lone surrogates and capture-reset semantics
need an ECMAScript engine rather than the Rust `regex` crate's language.
`research/quickjs/libregexp.c` at revision `7d1c13c5d8f8` is a semantic reference
only; this Rust parser was written independently.

`regexp::parse::parse(&JsString, &JsString)` takes pattern text (without slash
delimiters) and flags. It returns an owned, validated `ir::Pattern` or a
`SyntaxError { message, offset }`. Offsets count UTF-16 code units within the
pattern; flag-validation and unsupported-v errors instead index the flags.
The IR is a flat arena with backward node edges. Explicit group frames parse
nested disjunctions without Rust recursion; destruction is flat as well.
One lexical pass counts captures, permitting forward decimal/named references
and Annex B's fallback from out-of-range decimal references to octal escapes.
Capture indices begin at one and retain opening-parenthesis order and names.
Lookaround records direction/polarity; repetition records minimum, maximum
and greediness; classes preserve ranges, built-in sets and negation for the
matcher. Modifiers record scoped i/m/s additions and removals. There are no
runtime values or GC edges in the IR.

Non-Unicode parsing implements Annex B identity, control and octal escapes,
literal malformed braces, class-escape range endpoints, and quantified
lookahead. Unicode parsing enforces strict escapes and syntax characters,
combines raw or fixed-width escaped surrogate pairs, and preserves lone
surrogates. Capture names reuse the frontend's Unicode 17.0.0 identifier
predicates, including escaped names. Duplicate names are allowed only in
disjoint alternatives; named references retain every candidate capture index
for the matcher. This follows ECMAScript's
[MightBothParticipate early error](https://tc39.es/ecma262/2025/multipage/text-processing.html#sec-static-semantics-mightbothparticipate).
Scoped modifiers permit an empty removal list when additions are present.
Quantifier counts have an explicit u64 implementation limit and overflow
returns a positioned error. Flags `dgimsuvy` are validated, but v patterns
fail explicitly pending clj-quickjs-194. Binary and General_Category property
escapes execute in u mode (clj-quickjs-192); Script properties remain deferred.
This parser introduces no JavaScript RegExp objects. Literal-site
early errors must reuse it in clj-quickjs-56. The matcher in clj-quickjs-187
is a leaf operation with a documented step limit and an error on exhaustion,
per the engine policy decision.

## BigInt primitives (clj-quickjs-196)

`JsBigInt` in `src/value/bigint.rs` is the single integer type for subsequent
BigInt operators. It uses in-tree little-endian base-2^32 sign/magnitude limbs,
with no crate dependency. Values fitting `i64` are inline; larger values use an
immutable `Arc<[u32]>` body. `from_sign_magnitude` is the checked normalization
boundary: it removes high zero limbs, canonicalizes negative zero to zero and
selects the inline form whenever possible. Its representation is private, so
value equality and hashing never depend on the construction path. Arithmetic,
shift/logic and future radix-conversion implementations extend this type
and pass every result through the same boundary. Addition, subtraction,
multiplication, negation and updates are described under clj-quickjs-198 below.

The magnitude cap is 1 Mbit (`MAX_BIGINT_BITS`), following the user-selected
QuickJS policy. Constructors return `BigIntError::TooLarge` with the text
`BigInt is too large`. Literal overflow is a positioned frontend `TooLarge`;
arithmetic overflow becomes catchable RangeError with that text.
Every operation costs one VM instruction regardless of limb count. The cap
bounds each leaf computation, including equality and hashing, but there is no
cancellation/yield poll within that computation. Power-of-two literals pack
bits in linear time; decimal input/output use bounded quadratic limb work.
Future arithmetic must preserve the cap and avoid unbounded intermediate sizes.

BigInts are immutable primitives stored directly in `Value` and `Primitive`.
Like strings and symbols, their shared bodies contain no heap edges and live
outside the precise traced heap: collection follows no BigInt edges, and the
last primitive/constant/collection/root clone releases its body. They require no
new heap kind or category, are publishable, and retain exact values in
persistent vectors/maps across collection and task publication. Persistent map
keys use canonical integer equality/hashing, distinct from Number keys.

Strict equality compares mathematical integer values; ToBoolean is false only
for zero; `typeof` and diagnostic type names are `bigint`. Mixed BigInt/Number
arithmetic throws TypeError. ToString, wrappers and property-key conversion fail
explicitly with UnsupportedOperand naming BigInt. JSON serialization rejects
BigInt with TypeError. `Rooted::bigint` and `Scope::bigint` return an immutable
root-free `JsBigInt`, inspectable as decimal (`Display`) or sign/magnitude.
The CLI prints a BigInt completion with an `n` suffix.

## ArrayBuffer storage (clj-quickjs-165)

`HeapObject::ArrayBuffer` boxes an ordinary task-owned property store, a
`Vec<u8>` backing store and a detached flag. The bytes contain no heap edges;
`Trace` visits only the ordinary properties and prototype. An explicit heap
kind gives typed-array views one stable `ObjectId` to retain and inspect
on every access: a view must observe the buffer's detachment by identity rather
than keeping a copy or a pointer into its old allocation. Detachment empties and
releases the byte vector, preserving the buffer identity and its properties.
Both attached and detached buffers classify as `TaskOwned` in `value/share.rs`;
atoms and dependency publication reject them. Ownership checks cover properties,
prototype reflection and branded getters/methods, including foreign-task reads.

The alternative of immutable persistent bytes would require replacing every
view on transfer; independent native payloads with copied bytes would lose the
shared detached state. Neither represents JavaScript buffer identity. Shared
backing memory and SharedArrayBuffer are outside this pellet.

A buffer is limited to **16 MiB (16,777,216 bytes)**. ToIndex truncates toward
zero, maps NaN/undefined to zero, and rejects negative integers, infinities and
lengths beyond that bound with `RangeError`. This bound also rejects every
length above the ECMAScript safe-integer bound. Byte allocations use
`Vec::try_reserve_exact` before zero filling, mapping reservation failure to
`RangeError`. Destination allocation precedes transfer's detachment, so failure
leaves the receiver attached. Copy/zero-fill work is bounded by the per-buffer
limit; total runtime memory remains proportional to the number of live buffers.
`HeapUsage.objects.array_buffer` counts live buffers, and the separate
`HeapUsage.array_buffer_bytes` sums their reserved byte capacity, including
capacity beyond logical length. Shallow object/table byte metrics retain their
existing definitions. Reclamation and detachment release the owned storage.

The realm adds a constructor/prototype cycle, two branded accessor functions
and transfer/slice methods to its intrinsic root list and immutable views.
Object argument coercion runs through traced native conversion frames, allowing
`valueOf`/`toString` to wait, yield, migrate or trigger collection. Transfer
checks detachment after coercion (which can itself transfer the receiver).
Slice rejects detachment before and after conversion; ordinary intrinsic slices
copy bytes without detaching. Custom slice constructors/species fail explicitly;
subclassing, resizable buffers and SharedArrayBuffer are deferred. Integer typed-array
views are supported below.
The semantic reference is `research/quickjs/quickjs.c`, revision `7d1c13c5d8f8`,
`js_array_buffer_constructor`, `js_array_buffer_get_byteLength`,
`js_array_buffer_get_detached`, `js_array_buffer_transfer` and
`js_array_buffer_slice`. Implementation and tests are original Rust/synthetic JS.

The root inventory adds holder `ArrayBufferProperty`, covered by
`array_buffer_properties_retained_and_reclaimed` for data/accessor/prototype
edges. `array_buffer_reclaimed_when_unreachable` checks retention by a host root,
reclamation after dropping it, stale `WeakHandle` rejection and backing-byte
accounting. `array_buffer_coercion_survives_waits_gc_and_migration` forces
collection and changes worker threads during constructor/transfer/slice coercion.

ArrayBuffer constructor object-valued options are rejected explicitly after
length coercion until resizable buffers are supported; primitive options are
ignored. The constructor continuation retains options as a traced operand.
The bounded JSON serializer applies its existing 4096-property snapshot limit
to buffer ordinary stores, including non-enumerable properties. A per-thread,
test-only reservation hook forces `try_reserve_exact` to return a capacity error
without exhausting memory, covering RangeError mapping and preservation of
source bytes, descriptors, length and attached state on failed transfers.

## Exact BigInt comparisons (clj-quickjs-197)

`JsBigInt::Ord` compares signs, magnitude lengths, then high limbs first,
borrowing large bodies and using two scratch limbs for inline values. It
preserves the canonical construction boundary and agrees with value equality.
`cmp_number` decodes the finite f64 significand and binary exponent, constructs
its exact truncated integer (at most 1024 bits), and compares as integers. A
fractional remainder breaks integer ties toward the Number's sign. It never
rounds a BigInt to f64; subnormals, negative zero, NaN and infinities retain
their mathematical comparison rules. Temporary storage is bounded by the
Number format; the existing one-instruction leaf cost and 1 Mbit cap remain.

The primitive relational and loose-equality paths use these comparisons,
including Boolean-to-Number conversion. Nullish loose equality and Symbol
inequality retain their existing rules; relational Symbol conversion throws
TypeError. Existing native conversion frames handle object operands before
the leaf comparison, with number/default hints and source-order conversion.
They retain BigInt primitives across yield, dependency wait, GC and migration;
no new heap edges are introduced. BigInt/String pairs use the resumable
StringToBigInt parser described under clj-quickjs-201. Tests cover all acceptance
rows, signed/limb boundaries,
every normal integer power of two, the largest f64, and resumable coercion.

## Intl locale data

The data source is the pinned QuickJS snapshot at
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04), imported
through the tested `scripts/quickjs-data/generate.sh` exporter and
`repack-intl.pl`. The snapshot does not identify a verified CLDR release;
we do not infer one. Input SHA-256 hashes, generator/repacker hashes, compiler
versions and verification provenance are checked in with the data. Generation
is offline, explicit and repeatable. Cargo builds do not use C, C2Rust, ICU,
network access, `build.rs` generation or the reference checkout. No dependency
was added. A different source requires a concrete missing capability and pinned
inputs extending this pipeline, rather than a new data-source selection.

The representation is immutable safe Rust in service-specific modules under
`src/intl/data/`. Numeric fields replace C bitfields; references are immutable
slices. C byte pools retain embedded separators and the final NUL. The checked
`intl::pool_string` and `table_string` readers interpret those separators,
validate UTF-8 and bounds, and return `None` for malformed/missing entries.
Shared pools keep their original indices (including unused entries); locale
records and compact-pattern arrays are subset by the repacker, not copied by
hand. See `scripts/quickjs-data/README.md` for regeneration and verification.

Initial advertised locales are only `en`, `en-US`, `en-GB`, `de`, `fr`, `es`,
`ja`, `zh`, `zh-Hant`, `zh-Hans-CN`, `ar`, `ru`, `pt-BR`, `tr`, plus `zh-TW`
resolution. `en-GB`, `zh-Hant` and synthetic `zh-TW` have exact snapshot rows;
`en-US` uses `en`, `pt-BR` uses internal parent `pt`, and `zh-Hans-CN` uses
internal parent `zh-Hans`. These fourteen physical records are not a claim of
full modern-locale coverage. `lookup` takes a canonical tag, tries the exact
row then progressively shorter parents (including extension-singleton removal),
and returns `None` for `xx` or any tag with no supported parent. It never
silently substitutes another language. `resolve` tries the requested list,
then the host default, then `en-US`/`en` as the final root. Tag validation,
canonicalization, service availability and ECMA-402 option/extension resolution
are layered above this data lookup API: Locale/getCanonicalLocales provide
language-tag handling, and NumberFormat provides service resolution,
including `supportedLocalesOf`. Other formatting services remain deferred.

| Service/input | Snapshot coverage and policy |
| --- | --- |
| Numbers, currency symbols, compact patterns; date/time names and patterns | The fourteen records above and shared pools provide data for every advertised locale through its recorded parent. Decimal/percent/currency JS NumberFormat and its supportedLocalesOf consume the number data; other formatting services, compact formatting and date/time formatting remain deferred. |
| Likely subtags | All 7,788 entries retained as supplemental lookup data, not an expanded advertised locale set; Locale maximize/minimize consumes them (clj-quickjs-211). |
| Calendar/hour preferences, week data, script directions | Snapshot-wide supplemental tables retained for later Locale/date consumers. |
| Display names | Exactly `en` and `fr` data; other requested languages must be reported unsupported by that service, then resolve through its available default/root (`en`). They must not claim translated names for the whole numbers/date locale set. |
| Plural rules | Not in `locale-data.c`: rules are code in `intl/pluralrules.c`; later PluralRules work must port/test the rules for the advertised locales. |
| List, relative time, duration units | Not exported locale tables: patterns/rules live in `intl/listformat.c`, `intl/relativetimeformat.c`, `intl/durationformat.c`. Later service pellets adapt/test those snapshot rules for their supported subset and explicitly expose missing coverage. No additional CLDR source is currently selected. |
| Collation and segmentation | Small snapshot rules in `intl/collator.c` and `intl/segmenter.c`; complete UCA/DUCET and UAX #29 break datasets are absent. Later pellets own rule coverage and must pin any additional required inputs. |
| Time zones | The same exporter preserves 59 transition records versus 598 names, not equivalent coverage. Timezone integration belongs to the host-zone/date pellets; a name alone must not imply supported transition behavior. |

The default locale is `Host::default_locale() -> String`, a synchronous leaf
host input with the host-effect guard: no VM reentry, heap access or suspension.
The default trait implementation and a run with no host use `en-US`.
`intl::default_locale` reads the input under that guard; `SimHost` exposes
`set_default_locale` and supplies itself to its VM activations. The setting can
change between runs/tasks/worlds; locale reads are unrecorded effects, not
recorded dependency edges. Embedders requiring replay reproducibility keep the
setting stable for the relevant computation. No OS `LANG`/`setlocale` is read.
NumberFormat captures its resolved locale in immutable construction slots;
later Intl services follow the same host boundary.

Expected output follows QuickJS strings: the upstream `tests/test_intl.js`
rows run unmodified, including the date-component layouts with TODO alternatives,
Spanish currency grouping and narrow-space time strings, Japanese field order,
and DisplayNames' rejection of lowercase currency `usd`. Numbering follows the
snapshot service behavior: `ar` numbers use Latin digits by default while `ar`
dates use Arabic-Indic digits. This is deliberate compatibility, not a claim
of current ICU/CLDR conformance; no existing expected rows were changed here.

Unicode consumers start with the pinned export's compressed Unicode tables and
share safe readers/offline expansion with clj-quickjs-191/-192. The coordinated
policy for -209/-191/-192 is Unicode 17.0.0: -191 verifies folding and identifier
data across the complete domain against hash-pinned UCD 17 inputs and upgrades
the previous Unicode 16 identifiers. New properties, normalization, collation
and segmentation consumers must verify their own data against that version;
the download script's target alone is not evidence. Importing compressed normalization and
property arrays does not establish complete collation or segmentation semantics.
No separately pinned UCA/UAX #29 inputs are used by this pellet.

`Intl` is an ordinary non-callable, non-constructible shared realm intrinsic,
rooted and published with the other intrinsics. Its prototype is
`Object.prototype`; its own `Symbol.toStringTag` is `Intl` with writable=false,
enumerable=false, configurable=true. `Object.prototype.toString.call(Intl)`
reads that fixed intrinsic identity without a callback. General user-defined
`@@toStringTag` consumption remains deferred. Intrinsic mutation retains the
realm-wide restriction even where descriptors are configurable. Global binding
attributes follow the existing task global-object mechanism. Formatting service
constructors are deferred to their sibling pellets.

CLDR-derived tables carry Unicode License v3, reproduced in
`docs/UNICODE-LICENSE`; the QuickJS MIT notice is retained in
`src/intl/data/QUICKJS-LICENSE`. See `docs/attribution.md`.

There is no binary-size budget for this stage, per the user's decision; later
services need not subset further to meet a cap. The release measurement below
is informational, not an optimization target.

Measured with `CARGO_BUILD_JOBS=3 nix develop --command cargo build --release`
(Rust 1.95.0, default release settings, same source tree/toolchain for both
builds, after rebase onto `2e9fb07`). A temporary measurement `src/main.rs`
passed references to all 66 public statics in the six generated service modules
to `std::hint::black_box`, forcing their transitive byte pools to stay linked.
The matching control main made the same 66 calls with `&()` instead. The
executables measured **1,466,280 bytes retained versus 536,928 bytes control**
with `stat -c '%s' target/release/clj-quickjs`: a delta of **929,352 bytes**,
including address-retention code. `nm -C` confirmed linked data symbols from
all six modules. This isolates the data from unrelated branch/CLI changes and
does not rely on dead-code-eliminated formatting services. The normal main was
restored afterwards; no measurement program is shipped.

To reproduce, back up `src/main.rs` inside `target/`, then enumerate each
`pub static NAME:` in `src/intl/data/{numbers,dates,locale,display_names,
likely_subtags,preferences}.rs`. Generate a main with
`use clj_quickjs::intl::data; fn main() { ... }` containing one
`std::hint::black_box(&data::MODULE::NAME);` per declaration. Build with the
command above and record the executable's `stat` size; replace the body with
the same number of `std::hint::black_box(&());` statements, rebuild and compare.
Restore the original main even if either build fails. The generated source is
about 1 MB. No binary-size cap or size optimization is imposed at this stage.

## Task-owned jobs (clj-quickjs-174)

Each task owns a FIFO `JobInbox` (`src/job.rs`) in addition to its VM state.
An entry is a callable and argument values. Engine code holding the task's
`RunGuard` can enqueue values from that task directly, or register a
`JobSender` for a callable obtained from its live VM values. There is no
JavaScript-visible enqueue API. A sender is an opaque capability bound to
that inbox: another task may deliver shareable arguments through it, but
cannot inspect the callable or execute its closure cells. Registration must
happen on the callable's owning task; immutable shareable callables can also
be registered there. Future promise reaction records retain these senders,
and async resumes must target the activation's owning task.

Cross-task delivery validates the arguments with the ordinary transitive
publication check and rejects foreign-runtime handles. It enters heap access,
then briefly locks inbox metadata to append; it runs no JavaScript or host
callback and takes no task body or dependency-cell lock. A sender contains a
raw callable value and weak heap/inbox references. Its owning reaction record
must trace it; an external Rust holder must explicitly root its callable
until delivery finishes. Thus it keeps neither the destination task nor an
unreachable reaction/callable cycle alive through hidden permanent roots.
Engine callers should release reaction senders when the reaction is consumed.
Delivery to a finished or dropped task returns `Closed`. Pending raw entries
are traced as `TaskFrame` roots by the task root source, including when its
executing body is skipped by the
collector; active jobs and the saved body result are traced by `VmState` at
safepoints. No queue operation or payload destruction executes user code in
the collector pause.

FIFO order is the order of insertion under the inbox mutex, including owner
and foreign producers; racing producers have no earlier implied order. The
runner removes one entry at a time. A job that appends more work places it
after the existing tail. The final empty check closes admission under that
same lock: an enqueue racing completion either enters the FIFO before the
check or receives `Closed`, never a successful but stranded delivery. A
running or ready task needs no new wake. A task waiting on `__rt.need` remains
parked even when jobs arrive: the wait blocks the whole task and only its
dependency wake (or cancellation) can resume it. Senders do not schedule work
inline, and a completed task cannot be resurrected. The reference executor
uses the existing task outcomes and needs no separate job scheduling path.

After the script body returns normally, its value is saved in traced task
state and jobs run as fresh VM activations with `this = undefined`. JavaScript
functions enter directly; other callable forms use a small ordinary
Call/Return trampoline, so built-ins, bound functions and native continuations
use their existing resumable protocols and resource checks. Task identity,
world, globals and open transient builders survive activation changes. A
single run's instruction budget continues across the body and every job;
yields, waits and migration keep all continuation state in the task. The
internal argument limit is 65,534 values. There is no Rust stack retained
across runs.

The boundary hook runs after the body and after each normally completed or
uncaught-failing job, before starting the next job or reporting completion.
It currently has no production effect; WeakRef's `ClearKeptObjects` belongs
there. `Complete(value)` reports the original body's value only once the
queue is empty. An uncaught job exception reports `Failed(error)` and drops
the tail. A failed body never starts jobs. Observing consumer cancellation
closes admission and drops pending jobs before running the active activation's
existing finally cleanup; cleanup can still yield. No queued job runs as
cancellation cleanup. Callable lifetime in an owning reaction record remains
independent of pending-job cleanup and follows ordinary precise tracing.

Tests in `src/task/job_tests.rs` use synthetic JavaScript fixtures and a
crate-internal enqueue helper; they cover FIFO order, appended jobs, boundary
counts, shared budgets, migration after waits, cross-task delivery, closure
ownership and GC retention/reclamation. QuickJS's `JS_EnqueueJob2`,
`JS_ExecutePendingJob`, `js_finrec_job` and `tests/test_builtin.js`
(`test_finalization_registry`, `test_weak_ref`) were consulted as semantic
references at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`; no source or
test text was adapted. Promise settlement/reactions, async resumes, WeakRef
kept-object clearing, FinalizationRegistry producers and timers are deferred.
Future unhandled-rejection checks must run only at quiescence after jobs and
timers drain, rather than after each job; this pellet adds no rejection check.



## Regular expression matcher (clj-quickjs-187)

The in-tree matcher compiles the arena with an explicit build stack into
immutable Rust bytecode. `regexp::matcher::compile` rejects lookbehind
and non-Unicode ignore-case execution explicitly. Backreferences
are supported, with an undefined/forward capture matching the empty string.
Scoped m/s modifiers are compiled into the consuming/assertion instructions.
The compiler checks arena edges/indices and computes each atom's capture span
bottom-up for quantified capture reset. Compilation and destruction never
recurse with pattern depth.

`exec(&Program, &[u16], start)` returns
`Result<Option<Captures>, MatchError>` rather than losing resource failures
inside `Option`. Captures are optional UTF-16 offset ranges; group zero and
the match index identify the whole match. The engine searches from start,
or only at start for sticky mode. g/d have no extra core effect; JavaScript
RegExp objects, lastIndex and error-value construction belong to the object
layer (clj-quickjs-188).

The machine uses an explicit backtrack stack, capture/repetition registers,
and an undo trail. Choices save offsets and a trail cursor rather than
copying every capture. Repetition instructions implement greedy/lazy bounds,
clear the atom's captures on entry, and fail an empty iteration once the
minimum was already satisfied, permitting backtracking inside the atom.
Lookahead boundary frames share the same machine and budget. Success drops
internal choices atomically and restores the input position; positive
captures persist, while negative assertions rewind their trail on both
success and failure. Nested assertions never invoke the matcher recursively.

Policy: a match is a bounded leaf native, with no JavaScript callbacks,
yield, retained native stack or collector edges. The named constant
`MAX_MATCH_STEPS = 1_000_000` bounds one entire search across candidate
positions. Instructions, stack operations, capture writes/restores,
class-member checks and individual backreference comparisons consume work.
Register initialization and result extraction also consume allowance.
Explicit stack/trail growth is bounded by this allowance. Exhaustion returns
`MatchError::LimitExceeded`; its display identifies RangeError, and the future
RegExp native must turn it into a catchable JavaScript RangeError. It must
never translate this failure to no match. Compilation has its own allowance
of the same size, bounding program expansion and validation work. These
limits are implementation policy, so even long linear matches can fail.

## BigInt arithmetic (clj-quickjs-198)

`JsBigInt::checked_add`, `checked_sub` and `checked_mul` compute immutable
base-2^32 sign/magnitude results and normalize through `from_sign_magnitude`.
Signed subtraction compares magnitudes before borrowing; multiplication uses
schoolbook limb products with u64 carry. The product bit-length lower bound
rejects definite overflow before quadratic work. Buffers and carry propagation
never store a magnitude beyond the 1 Mbit cap. `negated` shares large immutable
bodies when canonical representation allows it; zero and the i64 endpoints
retain canonical forms. No new dependency or traced heap kind is needed.

`Add`, `Sub`, `Mul` and `Plus` dispatch two BigInts to the checked operations;
`Neg` preserves their magnitude. Appended opcodes `ToNumeric` (0x70), `Inc`
(0x71) and `Dec` (0x72) let binding/property updates preserve BigInt and use a
unit of the same numeric type. Postfix lowering saves the converted old value;
failed arithmetic never reaches the store. `ToNumberPair` now preserves two
BigInts and rejects a mixed pair. The compiler no longer assumes an unknown
numeric expression necessarily returns Number, including compound assignments
and updates. Unary `+` retains ToNumber and rejects BigInt with TypeError.

Object conversion continues through task-owned native Convert frames, with
hint default for addition and number for other numeric operators; values
survive yields, waits, collection and worker migration. Arithmetic itself is a
leaf computation costing one instruction, without internal polls. Each result
and intermediate operation checks the cap and maps overflow to RangeError;
later cancellation cannot hide an oversized intermediate. BigInt/Number and
other non-string primitive mixtures throw TypeError. BigInt ToString and the
remaining numeric operators are supported by the later BigInt sections below.

The Number constructor explicitly converts BigInt to binary64 (clj-quickjs-203),
including after an object's resumable ToPrimitive conversion and during
construction. Unary plus and other implicit ToNumber paths reject BigInt.

## Locale identifiers (clj-quickjs-210)

`intl::tag::LanguageTag` parses the Unicode locale identifier subset of BCP 47
using owned Rust strings and ordered maps/sets. It validates language, optional
script/region, unique variants/singletons, Unicode attributes/keywords,
transformed-language fields, other extensions and private use. Serialization
normalizes case, applies the selected snapshot aliases, sorts variants,
attributes, keywords and singleton extensions, and leaves private use last.
The requested legacy `co-phonebook` spelling is normalized before the standard
eight-character type-subtag bound. The parser does not consult service data:
unknown well-formed identifiers remain valid. It keeps the reference's
512-byte/64-subtag limits; complete CLDR alias coverage is not claimed.

Locale instances remain ordinary task-owned objects with a private, immutable
`locale_data` record initialized before allocation. This avoids a new heap kind
and reuses ordinary properties, prototypes, integrity operations and publication
checks. Slot data owns only strings and containers with no heap edges; existing
ordinary-object tracing retains the prototype and property values. A Locale
cannot be published between tasks even when frozen, since its ordinary property
store remains task-owned. Prototype changes cannot forge or remove the brand.

`vm::native::intl` drives Locale construction and CanonicalizeLocaleList as saved
stages. Every getter, tag/option ToString, and object-valued list-length ToNumber
returns an action to the trampoline, so no Rust stack survives JavaScript,
waits, budget yields, GC or thread migration. The continuation traces the source,
options and intermediate result values; duplicate keys are private Rust strings
with no user comparison. Locale list reads use HasProperty before Get to skip
holes while including inherited indices, and deduplicate only after canonicalizing
each element. Locale internal records bypass user-overridden string methods.
The shared list operation returns the ordinary array consumed by the public
`Intl.getCanonicalLocales` built-in and can be called by later service natives.

Locale's namespace/prototype functions and branded accessors are appended to the
realm, including `Symbol.toStringTag = 'Intl.Locale'`. The existing restriction
on intrinsic mutation still applies. General Object.prototype.toString tag
consumption remains deferred. Information methods,
`variants`/`firstDayOfWeek` options and formatting service resolution belong to
later pellets. Numeric absent from the tag/options yields `false`, matching
the pinned QuickJS getter rather than assuming another engine's output.

## BigInt division and exponentiation (clj-quickjs-199)

`JsBigInt::checked_div_rem` divides unsigned magnitudes, then applies quotient
sign XOR and the dividend's remainder sign. Normalization restores inline i64,
canonical zero and shared immutable large results. One-limb divisors use a
single high-to-low u64 division pass. Multi-limb divisors use normalized
base-2^32 long division (Knuth Algorithm D): shift the divisor's highest bit
to bit 31, estimate a quotient limb from two leading dividend limbs, correct
it using the next divisor limb, subtract the estimated product, and add back
one divisor on underflow. Complexity is O((m-n+1)n) limb operations for m/n
input limbs, with linear scratch storage. It processes quotient limbs rather
than individual bits and supports the roughly 13,000-bit pi-example operands.
The normalized dividend has one guard limb; it is a workspace encoding, not
an oversized mathematical BigInt result. Quotient/remainder magnitudes are
bounded by the inputs, and all returned values use `from_sign_magnitude`.

`checked_pow` rejects negative exponents first, handles zero exponent and
bases 0/1/-1 directly (including huge exponents), and checks the result-size
lower bound `(base_bits - 1) * exponent + 1` before computing. Powers of two
have an exact bit-size bound and are constructed directly. Other bases use
exponentiation by squaring, with the existing checked multiplication cap on
every intermediate; no unused final square is computed. The lower bound never
rejects a result that fits; ambiguous sizes are resolved by checked products,
which never store oversized magnitudes. Overflow is RangeError with `BigInt
is too large`; zero divisors and negative exponents have distinct RangeErrors.
`Div`, `Rem` and `Pow` dispatch matching BigInts to these leaf operations,
consuming one instruction each. The existing ToNumeric conversion frames,
compound lowering, rooting and publication paths remain in use. No dependency,
heap kind, opcode, JavaScript callback or internal budget poll is added.

## Integer typed-array views (clj-quickjs-166)

`HeapObject::TypedArray` holds an ordinary property store and a `View`:
element type, ArrayBuffer ObjectId, byte offset, and fixed element length.
It owns no bytes. `Trace` visits the buffer plus ordinary properties and
prototype; the GC inventory maps these edges to `typed_array_retains_buffer_and_properties`
in `tests/gc.rs`. Views are task-owned (including detached and empty views),
reject foreign-task access and transitive publication, and count separately
in heap usage; backing capacity is counted once on the ArrayBuffer.

Every element operation rereads the retained buffer's detached state under
a short heap access. No view caches byte pointers. Reads/writes decode and
encode little-endian bytes explicitly, on every platform. Unsigned modulo
2^32 conversion supplies the low bytes for all wrapping integer types;
Uint8Clamped uses bounded round-to-nearest, ties-to-even. Canonical numeric
keys intercept ordinary property/prototype paths; noncanonical strings and
symbols use the ordinary store. Element metadata, enumeration, descriptors,
delete, prototypes and integrity reflection integrate with existing VM paths.

Concrete constructor/prototype pairs inherit from `%TypedArray%` and its
shared branded-getter prototype. All intrinsics remain immutable and shared.
Constructor offset/length coercion and object-valued element/descriptor writes
run in traced `NativeState::Convert` continuations. Native callers such as
generic `Array.prototype.push` use `Action::SetProperty`, which applies the
same coercion and setter protocol while keeping their cursor in a traced frame.
Writes retain both the
receiver and original RHS (assignment result); a callback may detach the
buffer, wait or erase the caller's binding without retaining a Rust stack or
losing the only buffer edge. ArrayBuffer's fallible 16 MiB allocation limit
applies to newly allocated view backing stores. Resizable and
BigInt views and other typed-array methods remain deferred; source construction
and set/fill/join/toString are described under clj-quickjs-168 below.

Own-key reflection and `for`-`in` share a detached-aware element snapshot;
the iterator rechecks each descriptor before visiting it. Inherited writes
stop at a typed array's canonical numeric keys: valid elements allow an own
property on the receiver, while invalid indices are successful no-ops without
value conversion or further prototype traversal.

## BigInt bitwise and shift execution (clj-quickjs-200)

`JsBigInt::checked_and`, `checked_or` and `checked_xor` stream the finite
operand words from sign/magnitude into two's complement, applying invert plus
carry for negative inputs. The result sign is computed separately from the
infinite sign words. Each output word is converted back immediately; no
infinite sign extension or extra sign-word allocation is needed. A final
magnitude carry is checked against the cap before extending the buffer.
`checked_not` uses `-x - 1`, with checked subtraction for the extra result bit.
All returned results use the canonical `from_sign_magnitude` boundary.

`checked_shl`/`checked_shr` read the count's exact sign and magnitude. Negative
counts reverse direction; counts beyond the cap are resolved directly to an
overflow or a right-shift sign result (0/-1), without narrowing or allocating
for the count. Zero operands and zero counts return immediately. A left
shift's exact bit length is checked against 1 Mbit before allocating its
buffer. A right shift drops magnitude words/bits and increments a negative
magnitude if any discarded bit was nonzero, implementing floor division by a
power of two. All these computations use linear limb time and bounded storage.

The existing `BitNot`, `BitAnd`, `BitOr`, `BitXor`, `Shl` and `Sar` dispatch
paths accept BigInts after resumable ToNumeric. `UShr` rejects BigInts with
catchable TypeError; mixed numeric types also throw TypeError. Oversized
results throw RangeError with `BigInt is too large`, before any compound
assignment store. Each leaf operation costs one instruction with no internal
budget poll. The existing continuation roots, GC/publication rules, object
conversion order and Number semantics remain in force; no opcode, dependency
or heap kind is added. Source tests include conversion waits, instruction
budget yields, collection and migration, plus cap-boundary and native i128
comparisons for complement carries, signs, rounding and word-aligned shifts.

## Locale likely subtags (clj-quickjs-211)

`intl::likely::{maximize,minimize}` are pure operations on parsed `LanguageTag`
records. `lookup` performs a binary search over the existing, complete 7,788-row
likely-subtag table, separately reading the key and value from each embedded-NUL
byte pool through checked UTF-8 accessors. Tests verify ordering and lookup of
every row independently from the semantic Add/Remove Likely Subtags tests. No
data conversion or generated representation changes are needed.

Expected strings follow `intl/locale.c` at the pinned QuickJS revision, including
its search details: a specified language/script/region triple is already maximal;
otherwise try `und-script-region` when language is unspecified, then
`language-script`, `language-region`, and `language` (using `und` for an
unspecified language). `Zzzz` and `ZZ` count as missing script/region. An unknown
language has no fallback to `und-script`, and a failed search leaves the original
identifier intact. This deliberately follows the agreed QuickJS-output policy
where the pellet's generic UTS 35 lookup-order description differs from that
snapshot. Explicit subtags win over inferred data. Minimize first maximizes, then
tries language alone, language-region and language-script, checking each by
maximizing again. Variants and every extension survive both operations.

Two appended realm methods call these functions as leaf built-ins. The brand
and ownership check clones the immutable record before any allocation, then a
fresh ordinary task-owned Locale uses the intrinsic prototype. No JavaScript
getters, overridden constructors or coercions run; no continuation, heap kind or
GC edge is added. Existing ordinary-object tracing and publication rules apply
to the result. Before storing its slots, the result applies the same base-alias
normalization as construction: an inferred language can activate a variant alias
(for example, `und-Hant-hakka` maximizes to `hak-Hant-TW`). All extensions remain
intact through this normalization. The realm retention and GC inventory
assertions count the two new function objects. Formatting service negotiation and
`supportedLocalesOf` remain with clj-quickjs-212.

## Date time-value core (clj-quickjs-179)

`src/vm/date.rs` implements pure TimeClip/MakeDay/MakeTime/MakeDate arithmetic
and the UTC core bindings. MakeDay uses Gregorian leap counts with floor for
negative years and normalizes months without iterating over years. It does
not clip intermediate days or years; large components can cancel before final
TimeClip. Floating-point multiplication and addition preserve the specified
operation order, with no fused multiply-add. The unit test
`date_time_arithmetic` and source test `date_utc_and_time_value` cover all
upstream UTC precision/cancellation rows and clipping boundaries.

Date instances reuse `HeapObject::Plain` with `OrdinaryObject::date_value`, an
optional scalar f64 slot independent of own properties and prototype changes.
This follows the wrapper/Error slot model, needs no new heap kind or root
category, and uses existing ordinary tracing for properties/prototypes and
TaskOwned publication classification. Date.prototype is an unbranded intrinsic
ordinary object without a Date slot. The constructor/prototype cycle and five built-in
functions append to the realm/global registry and its immutable views/roots.
Intrinsic slot mutation is rejected; freezing an instance only freezes its
properties. Both slot reads and writes enforce task ownership.

`native::Convert` adds Date constructor, UTC and setTime completion targets.
It retains arguments, pending conversion progress and setTime's receiver as
precisely traced operands. UTC completes each primitive ToNumber before
visiting the next argument, even when previous results were non-finite;
only the first seven arguments participate. Date copying bypasses all user
hooks, and invalid method receivers fail before argument coercion. Existing
callback/unwinding and native-frame depth limits apply. Arithmetic work is
bounded (seven fields and constant-time calendar arithmetic).
UTC/setTime cap numeric strings at 4096 UTF-16 units before the shared leaf
StringToNumber scan or rope flatten, throwing RangeError after ToPrimitive
and before subsequent conversions/mutation. This bound is temporary until
budgeted primitive numeric parsing (clj-quickjs-265).
`date_coercion_waits_collects_and_migrates` collects at every yield/wait and
moves execution across workers, including an otherwise unreferenced receiver.
`date_objects_are_task_owned` checks publication and foreign slot access.

The clock/zone contract in `vm/date.rs` is implemented by clj-quickjs-182
(documented below): an unrecorded host leaf clock effect, one generated rule-table
zone per runtime, UTC without a host, and explicit rejection of uncovered zones.
Parsing continues to fail explicitly;
Date toString/toJSON value lookup and default coercion also reject deferred
formatting and Symbol.toPrimitive behavior rather than returning Object's
tag/numeric value or serializing a Date as an ordinary JSON object. Presence
checks report the deferred methods without reading them, while own toJSON
overrides still participate in resumable JSON serialization.

## Date UTC decomposition and ISO formatting (clj-quickjs-180)

`vm/date.rs::utc_fields` decomposes clipped time values into year, zero-based
month, date, hours, minutes, seconds, milliseconds and weekday (Sunday = 0).
All arithmetic after the exact i64 millisecond conversion is integer:
Euclidean day/remainder division handles negative timestamps, a binary search
over Gregorian year starts takes at most 20 iterations, and month selection
takes at most 11. These bounds hold over the entire ±100,000,000-day TimeClip
range, including negative years and Gregorian century/leap boundaries.
The shared field structure is available for the UTC getter/setter pellet.

The append-only `DateToISOString` built-in is a bounded leaf: it reads the
existing branded, task-owned slot, rejects invalid dates with the standard
RangeError (`Invalid time value`), and creates a 24- or 27-unit UTC string.
Years 0..9999 have four digits; other years have a sign plus six digits.
It calls no user code, so requires no native continuation. The method is
appended to the intrinsic root list after existing Date methods and preserves
normal method metadata/flags. Tests cover required decompositions, month
boundaries across two Gregorian cycles, expanded year transitions, invalid
dates, receiver branding, descriptors and foreign-task access. Other Date
formats and `toJSON` remain explicit gaps.

## Floating-point typed-array storage (clj-quickjs-167)

`ElementType` appends Float16, Float32 and Float64; all use the existing traced,
task-owned `TypedArray`/ArrayBuffer representation and constructor/write
continuations. The legacy `Builtin::IntegerTypedArray` registry entry also
dispatches these Number element kinds. No new heap category or continuation
state is needed. Encoding and decoding use explicit little-endian bytes.

Binary16 conversion is dependency-free safe Rust with no unstable `f16` type.
`to_binary16` extracts the binary64 sign, exponent and 53-bit significand.
It discards 42 significand bits for normal values, or 43..53 bits to round
subnormals at a fixed 2^-24 quantum. Comparing the discarded remainder with
half a quantum, and incrementing an odd retained significand at equality,
implements round-to-nearest, ties-to-even. Integer carry handles the smallest
normal and infinity boundaries. Rounding directly from binary64 avoids the
double-rounding error of converting through binary32. Smaller values become
signed zero. `from_binary16` widens normals by assembling binary64 bits and
subnormals by exact scaling of the ten-bit fraction by 2^-24.

Binary32 stores use Rust's IEEE narrowing cast; binary64 stores retain their
Number bits. All three explicitly store positive canonical quiet NaNs:
0x7e00, 0x7fc00000 and 0x7ff8000000000000. Loads accept every NaN encoding,
including payloads written through overlapping integer views. Signed zero,
infinities and subnormals survive exact widening. Unit tests cover every
binary16 encoding and all finite positive adjacent-value midpoints with
binary64 neighbors on either side, plus negative ties and overflow. Source
tests cover shared bytes, metadata, errors and coercion across waits, GC and
worker migration. `Math.f16round` and further typed-array methods/forms remain
outside this slice.

## RegExp objects (clj-quickjs-188)

RegExp instances use task-owned `OrdinaryObject` storage (`HeapObject::Plain`)
with an immutable `Arc<regexp::object::Compiled>` internal slot. Programs,
UTF-16 source text, flags and capture names have no heap edges. The ordinary
object traces prototype and property edges; `lastIndex` is an own writable,
non-enumerable, non-configurable data property. Publication rejects instances,
even when frozen; only immutable code is shared. RegExp's realm intrinsics
are built before publication and never changed.

The append-only `RegExpLiteral` opcode (`0x73`, u16 string-constant index)
uses a constant containing flags, NUL, then raw pattern code units. Loading
compiles each referenced constant once into the executable's immutable
program table, retaining failures for catchable runtime SyntaxError/RangeError
construction. Executable serialization retains the constant and recompiles
on loading; repeated evaluation allocates fresh objects sharing the program.
Pattern early errors at literal sites remain clj-quickjs-56.

Constructor argument conversion, `exec` input and `lastIndex` conversion,
generic `test` dispatch, `toString` and `flags` getters use traced native
continuations. JavaScript getters, ToPrimitive methods and overridden exec
run as ordinary VM frames and may wait, yield, collect and migrate. Matching
runs as a leaf with `MAX_MATCH_STEPS = 1_000_000` for each search (all starting
positions combined); compilation has its own allowance. Exhaustion throws a
catchable RangeError and never becomes a failed match. Parsing, source
escaping, capture copying and string flattening are synchronous, bounded by
the runtime's string length limit, like existing string leaf operations.

Objects support the code-unit and Unicode matcher, d/g/m/s/u/y and iu, named captures with a
null-prototype groups object, and explicit undefined elements for unmatched
captures. Plain i and v constructors and literals throw SyntaxError naming the
unsupported flag; lookbehind also fails explicitly. Symbol.match
recognition, symbol-based string consumers, non-Unicode folding and Unicode sets
remain later work. Direct and indirect eval are unavailable, so the raw NUL
upstream case is exercised through the constructor and a literal containing
an actual NUL instead.

## BigInt conversion and string comparison (clj-quickjs-201)

The append-only global/Builtin registries install callable, nonconstructible
`BigInt` (length 1). Its ordinary `BigInt.prototype` has a constructor link and
inherits from Object.prototype. Both are rooted and copied into the existing
immutable realm views. BigInt primitive property lookup and prototype reflection
use that prototype without allocating a wrapper or introducing a heap kind.
`toString` and `valueOf` are installed by clj-quickjs-202 below, so the
unbranded prototype rejects conversion with TypeError.

`JsBigInt::from_integral_number` validates finiteness/integrality and decodes the
binary64 significand and exponent directly into canonical limbs (at most 1024
bits). No i64 narrowing or decimal formatting rounds the argument. The builtin
also handles booleans and existing BigInts, rejects null/undefined/symbols, and
uses existing hint-number ToPrimitive continuations for object arguments.

`StringToBigInt` is a shared task-owned parser in `src/value/bigint.rs`. It trims
with StringToNumber's whitespace predicate, validates the complete UTF-16 input,
skips leading zeros, then constructs canonical sign/magnitude limbs. Signed
inputs must be decimal; radix-prefixed inputs must be unsigned. Invalid grammar
is reported before resource limits, even for oversized digit prefixes. A
significant-digit bound rejects clearly oversized decimal strings before any
quadratic computation. Power-of-two radices pack bits linearly; decimal input
uses multiply/add groups of nine digits, resuming inside the limb loop. Limb
storage never exceeds 1 Mbit. Each advance performs at most 256 bounded scan/limb
steps (a decimal group reads at most nine digits);
normalization, capped buffer allocation, and final immutable limb transfer are
bounded by the magnitude cap. The existing JsString flatten cache may perform
one linear traversal before scanning a rope, without a retained Rust stack.

The native BigIntParse frame owns the parser, comparison operands and original
instruction. It has no additional heap edges beyond its traced operands;
JsString and limb buffers contain no traced pointers. It returns a BigInt for
a function call, or resumes a comparison with the parsed operand. Invalid text
throws SyntaxError for the function and becomes NaN (unequal/unordered) for a
comparison. Oversized valid text throws RangeError in both contexts. Object
comparisons finish both observable ToPrimitive conversions in source order
before entering this parser. Budget boundaries preserve parse progress across
GC, migration and cancellation. Arithmetic retains its existing one-instruction
leaf policy; long StringToBigInt operations explicitly use native steps.

## Date UTC field getters and setters (clj-quickjs-181)

Two append-only parameterized built-in kinds install fifteen methods after
Date's existing intrinsics. The getter index selects one of the eight shared
UTC decomposition fields. The setter index selects its first field and a
metadata table supplies the maximum argument count and method length.
Getters are bounded leaves; the shared integer decomposition covers the full
TimeClip range without host input.

`native::ConvertTarget::DateSetUtcField` retains the original numeric time
before callbacks run. Its ordinary traced operand vector retains the receiver
and up to four arguments, including arguments still awaiting conversion.
The receiver is branded before conversion and excluded from ToPrimitive.
Each argument completes ToNumber (with the existing numeric-string limit)
before the next begins, even for invalid dates and non-finite earlier numbers.
Missing first arguments become undefined; absent optional fields are retained.

Completion uses the original time's decomposition, replaces supplied fields,
then calls shared MakeDay/MakeTime/MakeDate and the existing TimeClip slot
writer. Invalid original times return NaN without writing the slot, except
that the full-year setter uses the epoch as its base. This preserves callback
slot mutations where required while normal completion overwrites them using
the original snapshot. No new heap kind or host effect is introduced.

## Typed-array native continuations (clj-quickjs-168)

`NativeState::TypedArray` holds construction, set and fill stages in
`src/vm/native/typed_array.rs`. Source/target objects, iterator, cached next
method, iterator result, collected values and pending arguments are traced
through `NativeFrame`. No Rust stack or heap borrow survives a callback.
All element loops visit at most one element per native step, within the
256-step bound used by other natives; each native step consumes instruction
budget. Numeric allocation retains ArrayBuffer's existing fallible 16 MiB
bound; iterable collection rejects more elements than fit in that bound.

Typed-array sources bypass iterator lookup. Copies snapshot one element per
step before any target writes, handling shared buffers with different element
widths. Same-type snapshots retain up to eight raw bytes per element so NaN
payloads survive; cross-type copies read Numbers and convert to the target
representation. Iterable construction gathers values before conversion;
array-like construction and set interleave indexed reads and conversion.
Fill caches its converted Number and clamps start/end against the initial
internal length, rechecking detachment after conversions.

Typed-array join validates its receiver and seeds the existing `ArrayJoin`
continuation with the internal length, bypassing shadowing `length` properties.
Element reads and final `StringBuild` copying are budgeted. The shared
prototype references Array.prototype.toString's existing function object and
appends three method functions to the realm's intrinsic allocation sequence.
Other typed-array methods and BigInt/resizable/shared views remain deferred.

Typed-array construction now uses the general Array iterator intrinsic
(clj-quickjs-237); its former local array fallback has been removed. Explicit
null/undefined iterator overrides still select array-like behavior.

## Synchronous iterators and for-of (clj-quickjs-237)

`src/vm/native/iterator.rs` exposes reusable resumable GetIterator state to
native consumers. `get_iterator` (`0x74`) returns a hidden task-owned record
holding the iterator and cached next method. For-of keeps these in frame
locals and emits ordinary property reads and calls for next/done/value.
`require_object` (`0x75`) checks protocol results; `pending_throw` (`0x76`)
queries the current finally completion so IteratorClose can suppress a close
failure only for a pending throw. No enum discriminants were repurposed.

One finally handler surrounds the loop. A hidden local distinguishes the
body/binding assignment from next/done/value evaluation: throwing during the
latter skips close; cancellation still closes. Lowering's IteratorLoop exit
entry runs the handler for breaks and jumps leaving it, and keeps it for its
own continue. Exhaustion removes the handler without calling return.
The generated close block reads and calls optional return, validates its
result, catches errors only to preserve a pending throw, then resumes the
original completion through EndFinally. Nested user finally blocks keep
their existing order and can override jumps before close is reached.

Built-in array/string iterators are ordinary task-owned objects with a boxed
internal `iterator::Data` slot (source, cursor, kind, exhaustion flag).
OrdinaryObject tracing visits the source; publication rejects these objects
under the existing TaskOwned classification, even after Object.freeze.
Next uses a traced native continuation for length/index getters and object
length coercion. It advances the cursor before the indexed getter, reads live
array lengths, and permanently releases the source on exhaustion. Typed
arrays use their view's internal length and validate detachment per step.
Borrowing Array.prototype.values onto a typed array selects the same checks
when next runs, without validating detachment prematurely in the generic factory.
Strings preserve UTF-16 code-point boundaries, including lone surrogates.
No lock or Rust frame survives user code, a wait or a safepoint.

The realm appends shared IteratorPrototype, ArrayIteratorPrototype and
StringIteratorPrototype intrinsics; identity Symbol.iterator, next and
toStringTag are inherited. Array/typed-array values methods are their
Symbol.iterator methods by identity. Iterator callbacks and all hidden
locals/native state are traced by the existing task root inventory.

QuickJS references and provenance: `research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `js_parse_for_in_of`,
`OP_for_of_start`, `OP_for_of_next`, `OP_iterator_close`, `emit_break`,
`emit_return`, `JS_IteratorClose` and built-in array/string iterators were
consulted as semantic references. Implementation and synthetic tests are
written fresh; no C stack frames, operand catch offsets or code are copied.

## String replacement (clj-quickjs-189)

Two appended built-ins expose `replace` and `replaceAll` on String.prototype.
`vm::native::string_replace::State` retains/traces the receiver, search and
replacement values. Other saved fields are immutable JsStrings/compiled
regexp programs, capture ranges and Rust-owned buffers without heap edges.
Callbacks receive fresh task-owned null-prototype named-group objects; call
arguments and returned objects stay rooted by the ordinary VM/native frames.
No heap kind or publication category changes. Coercions and callbacks use
Action::ToString/ToNumber/Call and may suspend without a retained Rust stack.

Dispatch deliberately checks internal RegExp slots rather than observable
Symbol.replace/exec/flags hooks (see grammar for the deviation). Non-RegExp
objects use resumable ToString. Matching collects immutable capture ranges
before any replacer calls, so global lastIndex reaches zero before callbacks.
The own non-configurable lastIndex data property uses strict writes and its
value uses resumable ToLength, including nonglobal calls. Global empty-match
advancement implements AdvanceStringIndex: code points with u, UTF-16 code
units otherwise (clj-quickjs-190).

String searches reuse the existing incremental KMP prefix table and restart
after each non-overlapping match, at most 256 comparisons per native step.
Regexp collection executes one MAX_MATCH_STEPS-bounded search per step.
Rendering handles one match/callback per step; its synchronous copying and
GetSubstitution work share a 1,000,000-unit aggregate allowance, including
capture-range storage, callback substring copying, name comparisons and
output. Receiver/search/replacement and callback result strings are checked
against 65,536 units before flattening. These documented implementation
limits also apply to lastIndex strings, including strings returned by a
suspending ToPrimitive, through the shared Action::ToNumberLimited path
before leaf StringToNumber. Limits throw catchable RangeError; no dependency
or instruction budget can turn exhaustion into a successful partial
replacement. Realm/GC inventory
counts grow by the two additional immutable built-in functions.

## BigInt radix formatting and ToString (clj-quickjs-202)

`BigIntToString` owns a magnitude copy, a division cursor/remainder, reverse
chunk digits and an output buffer; none contains heap pointers. Decimal
conversion divides by 10^9, emitting nine digits per pass except the leading
chunk. Other non-binary bases use the largest radix power fitting u32.
This chunked schoolbook conversion remains quadratic in the number of limbs,
but avoids per-digit whole-magnitude division: the 6,700-bit/2,001-digit
acceptance case completes inside the ordinary source test. Bases 2/4/8/16/32
extract groups of bits directly in linear time, including at the 1 Mbit cap.
Each advance performs at most 256 limb divisions or digit copies (a completed
chunk emits at most 31 digits). Reverse-digit copying is also incremental;
finishing transfers the owned UTF-16 buffer to JsString without copying it.
Initial magnitude copying is bounded by the existing 1 Mbit cap. Host Display
retains its synchronous decimal formatter; JavaScript formatting uses native
steps, each charged as one instruction. Arithmetic keeps its one-instruction
leaf policy.

The append-only Builtin registry adds `BigIntPrototypeToString` and
`BigIntPrototypeValueOf`, with two functions appended to the realm root list.
No heap kind, opcode or dependency is added. Methods require a primitive
BigInt receiver; prototype branding is checked before any radix callback.
The existing Number radix helper is shared for ToIntegerOrInfinity and
RangeError validation. Only an originally missing/undefined radix defaults
to 10; an object returning undefined converts to NaN and throws RangeError.

The shared native Convert state retains an optional formatter alongside its
traced operands. Targets that apply ToString format BigInt primitives before
continuing, including values returned by object callbacks. Binary `+` first
obtains both primitives using hint default, then formats if either is a string.
Array join, String.raw and Intl option coercions request the same resumable
ToString path. The primitive leaf helper can finish only a single bounded
formatter advance; long conversions must use the native state. Its retained
operands and all pre-existing continuation fields stay traced across yields,
waits, collection and thread migration. Failure/cancellation drops the buffers.
JSON still rejects BigInt serialization. Wrappers and locale-sensitive
BigInt.prototype.toLocaleString remain deferred.

## Unicode RegExp mode (clj-quickjs-190)

The immutable matcher program stores a Unicode-mode bit (`u` or future `v`
IR), and character instructions retain u32 code points. The shared forward
reader decodes UTF-16 pairs and preserves lone surrogates. Literals, dot,
classes and backreferences use that reader; the search loop uses the same
AdvanceStringIndex operation as global replacement's empty-match progress.
Compilation validates code points through U+10FFFF in Unicode mode and code
units otherwise. The v parser/object flag remains explicitly unsupported.

`matcher::exec` maps a starting offset within a pair to its lead, matching
QuickJS `lre_exec`; this applies to both exec/test and sticky replacement.
All machine positions, undo trails and captures remain UTF-16 offsets, so
result extraction and lastIndex updates need no conversion tables or copies.
Backreferences compare code points so a captured lone surrogate cannot match
half of a later pair. Assertions include Unicode ignore-case word classification
(clj-quickjs-191); lookbehind stays deferred. The existing single search step budget
covers candidate scans and comparisons, with catchable RangeError on exhaustion.
No heap representation, native continuation or publication rule changes.

## Shareable Promise core (clj-quickjs-246)

`HeapObject::Promise` is an explicit concurrent reference, never a dependency
cell. It holds a fixed intrinsic prototype, an optional fulfilled/rejected result, an `is_handled` bit and an ordered list of
reaction registrations. These fields are inspected or changed only under the
heap's short object-table metadata lock. There is no per-Promise Arc/Mutex,
interpreter lock or JavaScript execution inside that lock. Resolving functions
are immutable built-in function objects tracing a `PromiseResolving` guard;
the guard holds the Promise identity and the pair's shared first-call bit.
Claiming it precedes observable `then` lookup, so reentry cannot resolve twice.

A Promise can be published while pending or settled. Every settlement runs
`share::check` before storing its result: shareable results are available to
all registered tasks; an unshareable value is stored in an opaque owner-bound
result envelope carrying the settling task's identity. It is a traced lifetime
edge, not a permission to read that object's graph from another task. Only
that task's reactions can extract the value. A foreign reaction rejects its
derived promise with a locally created TypeError. This extends the opaque
owner-bound callable rule used by the job inbox to result storage, preserving
ordinary same-task object/error identity without permitting shared mutable
JavaScript objects. The publication walk stops at the concurrent Promise;
it does not walk through either opaque result or reaction envelopes.

Each registration creates a traced `PromiseReaction` record containing source,
derived Promise and both handlers, and a built-in job callable tracing that
record. Non-callable handlers are treated as identity/thrower when the job
runs. A registration holds the callable and a weak reference to the
registering task's inbox. It never keeps that task alive, imports its closure
into the settling task, or runs its JavaScript there. Settlement appends
zero-argument reaction jobs to those inboxes while still holding the Promise
metadata lock, preserving registration order even against a racing `then`.
Inbox push is a leaf metadata operation; it performs no heap lookup, JS, host
effect, dependency wake or callback. The job reads the source result through
the owner check on its registering task. Dead/cancelled/completed destinations
silently discard delivery; settlement never resurrects a finished task.

Executors, handler calls, resolving `then` getters and generic `catch` getters
use task-owned native continuations and ordinary VM call frames. A native
catch boundary is inserted immediately before an executor/handler/getter
call. Inner JavaScript catch/finally entries run first; the boundary then
stores a thrown value in the native continuation, which rejects on its next
budgeted step. Normal returns remove the boundary. Cancellation skips these
boundaries and uses ordinary finally cleanup, dropping queued jobs. Native
continuations and the inbox's weak routing link survive activation switches,
yields, waits, collection and migration. No Rust callback stack survives.

`Promise.resolve` preserves an intrinsic Promise argument's identity;
`Promise.reject` creates a new Promise. Non-thenable results fulfill; callable
thenables are adopted through resolve-thenable jobs (clj-quickjs-247). Promise
subclass capabilities/species and mutable own properties are explicit gaps.
Self-resolution and inaccessible foreign results reject with errors built by
the intrinsic TypeError constructor, including its ordinary `stack` property.
The two internal rejection-tracking transitions are reserved for -249;
quiescence reporting and its fixed-size list are not implemented here.

Root inventory additions: `pending_promise_reactions_are_traced` retains a
pending Promise reachable only through a closure, the derived Promise, both
handlers and their captures; it reclaims the complete unreachable cycle.
`settled_promise_results_and_resolvers_are_traced` retains/reclaims object
results through a Promise closure and through a resolving function alone.
`promise_reaction_waits_and_resumes` covers active native frames and queued
reaction records across GC and migration. `promise_cancellation_releases_queued_jobs_and_runs_finally`
checks reclamation after cancellation; `promise_publication_and_cross_task_settlement_do_not_run_callbacks`
checks shared publication, remote settlement and whole-task wait isolation.
Results and prototypes are traced from Promise objects; source/derived/handler
edges from reaction records; resolver and reaction identities from dynamic
built-ins; native pending values from TaskFrame roots; queued jobs and saved
body values use the existing job root inventory. Weak inbox references add
no collector roots, and no Promise creates hidden persistent roots.

QuickJS references and provenance: consulted `research/quickjs/quickjs.c`
`promise_reaction_job`, `fulfill_or_reject_promise`,
`js_create_resolving_functions`, `js_promise_resolve_function_call`,
`js_promise_constructor`, `js_promise_resolve`, `perform_promise_then`,
`js_promise_then` and `js_promise_catch`, and
`research/quickjs/tests/test_std.js` `test_async_gc` /
`test_async_promise_rejection`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The implementation is written
fresh. The opening rejection-counter case in `tests/promises.rs` adapts
`test_async_promise_rejection`, replacing its timer with a final reaction
job; that file preserves the QuickJS LICENSE notice. Other fixtures are
synthetic and were written for this runtime.

## Date host clock and runtime zone (clj-quickjs-182)

The leaf Host methods `current_time_millis` and `default_time_zone` run under
HostEffect's thread-local guard: no VM reentry, heap access or suspension.
Clock reads are unrecorded effects, with no dependency edge or world/version
identity; rerunning a task may observe another time. Host errors become
catchable RangeError; NoHost supplies no clock. Host clock numbers undergo
TimeClip. The engine never reads the OS clock or OS timezone.

`Heap::date_zone` is a OnceLock of a validated immutable `host::TimeZone`, with
no heap edges. The first local Date operation obtains it from the current host;
NoHost chooses UTC. Tasks, worlds, runtime clones, yields and thread migration
all share that choice. Concurrent first use selects whichever host initializes
the OnceLock; embedders should supply a consistent zone. No locale or timezone
is read while holding a dependency-cell or heap-object lock. Explicit virtual
fixed offsets preserve seconds and must have magnitude less than 24 hours.
`SimTimeHost` has a mutable virtual clock, read count, and configurable zone.

Named zones use `src/host/timezone/tables.rs`, repacked without changing values
from the verified QuickJS exporter. Of 598 inventory names, 59 canonical zones
have transitions and 36 aliases resolve to these records. UTC/GMT names are
also accepted; other names fail in the embedding API instead of falling back.
Offsets retain source seconds. For each zone, the first offset applies before
the first finite transition and the last applies forever after the final one;
there is no future DST recurrence extrapolation. Individual ranges and names
are explicit in the generated tables. This is limited snapshot support, not a
complete IANA timezone implementation.

`TimeZone::local_time_millis` and `utc_time_millis` implement LocalTime and UTC(t)
for future parsers/formatters too. UTC(t) chooses the earlier valid instant in
an overlap and uses the offset before a forward jump for a gap. UTC lookup is
binary search; inverse lookup is bounded by one zone's finite transition table
(at most 243 entries in this snapshot). Local fields may exceed TimeClip by
one offset; clipping occurs after conversion to UTC, never on intermediates.
Local setters capture the receiver's time before ordered resumable coercion,
then reuse UTC setter calendar arithmetic with conversion at its boundaries.
Invalid setFullYear starts from wall time +0; other invalid setters still coerce
arguments without overwriting mutations made during those coercions.

Date() string calls remain explicitly unsupported until the formatting pellet.
Tests in `tests/date_host.rs` cover fixed zones, actual Paris gap/overlap rules,
historical seconds, missing/unsupported inputs, clock leaf guards, one-zone
selection, tiny budgets and coercion through waits, GC and worker migration.

Snapshot canonical zone coverage (prefixes apply to every name in each row):

| Prefix | Names |
| --- | --- |
| `Africa/` | `Monrovia`, `Tunis` |
| `America/` | `Anchorage`, `Argentina/Tucuman`, `Boa_Vista`, `Fortaleza`, `Los_Angeles`, `Maceio`, `New_York`, `Noronha`, `Recife`, `Sao_Paulo`, `St_Johns`, `Toronto`, `Vancouver` |
| `Antarctica/` | `Casey` |
| `Asia/` | `Kolkata`, `Riyadh`, `Singapore`, `Tokyo` |
| `Australia/` | `Lord_Howe` |
| `Etc/` | `GMT+1` through `GMT+12`, `GMT-1` through `GMT-14` |
| `Europe/` | `Berlin`, `London`, `Moscow`, `Paris`, `Riga`, `Simferopol`, `Tirane`, `Vienna` |
| `Pacific/` | `Apia`, `Niue` |
| no prefix | `GMT`, `UTC` |

The additional `Etc/UTC` and `Etc/GMT` spellings select UTC. The 36 snapshot
aliases are listed in `ti_tz_aliases` in the generated module. Across all zones,
finite recorded transitions range from 1847-12-01T00:01:15Z through
2037-11-01T10:00:00Z; individual zone ranges vary, and fixed zones contain only
a sentinel record. Dates outside those ranges use the documented first/last
offset policy, which may differ from real historical or future civil time.

## BigInt to binary64 conversion (clj-quickjs-203)

`JsBigInt::to_number` borrows the existing canonical sign/magnitude view,
using stack scratch for inline values. Values of at most 53 bits convert
exactly. Larger values extract the top 53 bits, a guard bit and a sticky bit
from all remaining low limbs, then round once with ties to even. Significand
carry advances the exponent, including overflow to signed Infinity. Magnitudes
above 1024 bits immediately return signed Infinity; otherwise at most 32 limbs
are inspected, without copying or allocating magnitude storage. Zero is +0.
This bounded leaf computation needs no additional native continuation or heap
roots. Exact BigInt/Number comparisons continue using `cmp_number`, without
rounding either mathematical operand.

`primitive_constructor` dispatches BigInt directly to this helper for both
Number calls and construction, including after the existing resumable
ToPrimitive native frame. Other primitives continue through `to_number`, whose
BigInt TypeError is unchanged. All existing Math operations, global numeric
predicates, Number methods, Date numeric setters and typed-array stores retain
that rejection; `docs/grammar.md` lists the numeric entry points. Tests exercise
rounding across every binary64 integer exponent, the overflow midpoint,
Number wrappers and callback continuations across yields, dependency waits,
collection and worker migration. No builtin/realm entries or heap kinds change.

## Decimal NumberFormat (clj-quickjs-213)

`intl::number_format` holds immutable formatting options and decimal arithmetic;
`vm::native::number_format` embeds the existing CanonicalizeLocaleList state and
saves option getter/coercion and FormatNumeric argument-coercion stages. A new
native ToPrimitive-with-number-hint action preserves String and BigInt results
instead of converting them through binary64. The trampoline reads the guarded
host default locale only when requested locales cannot resolve, then captures
it in the instance. No callback or host input retains a Rust stack over a wait.
The service supports the policy's locale set with lookup for both matchers.
Numbering-system names and digit mappings are imported from pinned QuickJS
`intl/intl.c` through the existing C/Rust verification pipeline. Unicode nu
extensions and explicit options resolve together; unknown well-formed systems
fall back to the selected system. Default digits remain Latin, including ar.
Algorithmic systems with no decimal digit mapping retain Latin digits, as
QuickJS does.

NumberFormat instances are ordinary task-owned objects. Their private record
owns strings and static locale-data references; a separate cached bound-function
slot is traced. The bound callable retains its formatter through its immutable
`this`, so a detached function keeps the formatter alive. The collector handles
that cycle; publication rejects both the mutable formatter and a callable
capturing it. Brand checks enforce task ownership even through borrowed methods.
The append-only realm adds one prototype and five built-ins, including a hidden
bound-call target. The public format getter lazily allocates a nonconstructible
bound callable with name "" and length 1; freezing ordinary properties does not
prevent initializing that internal cache.

Numbers reuse `value::string::number_to_string` before decimal rounding, matching
QuickJS's Intl mathematical-value path rather than scaling binary floats.
Coefficient digits and point position support all nine rounding modes, including
carry, significant digit padding and negative zero. BigInts and decimal strings
bypass f64. BigInt arguments and options use the shared native ToString
continuation before validation or formatting, so decimal conversion remains
budgeted across collection and worker migration. Symbols/group sizes come from the existing pinned locale tables;
auto groups four-digit Spanish numbers as QuickJS does, while min2 requires a
two-digit leading group. The digit data extends the existing generated number
tables; locale records remain unchanged. Input coefficients and exponents have
explicit resource bounds (100,000 characters and magnitude
10,000); valid oversized inputs throw RangeError. JavaScript whitespace is
trimmed with the shared StrWhiteSpace predicate, and malformed mantissas or
exponents produce localized NaN before the resource checks or coefficient
allocation. No dependencies or OS locale access.

Unit style and nonstandard notations,
formatToParts/ranges, and legacy ChainNumberFormat receivers remain deferred.
Unit style and nonstandard notations throw RangeError. General
Object.prototype.toString tag consumption remains the existing explicit gap.

## Date non-locale string formatting (clj-quickjs-183)

`vm/date.rs::format_string` shares bounded calendar decomposition with ISO
formatting and field access. It emits QuickJS's local date/time parts or its
complete UTC form, including English day/month names, signed negative years
with at least four digits, and `Invalid Date` for NaN. The formatter truncates
the existing runtime zone's seconds offset toward zero to minutes before
decomposition, matching QuickJS's `get_date_fields` and `get_date_string`.
Zone lookup also truncates epoch milliseconds toward zero to whole seconds,
as QuickJS does, including just before negative-epoch zone transitions.
It prints `GMT±hhmm` without an abbreviation; UTC prints `GMT`. No tables,
dependencies or heap kinds are added. Local getters keep their second precision.

Four append-only built-ins are rooted in the Date realm. `toUTCString` is a
bounded leaf; `toGMTString` aliases that exact function. Local formatting uses
the existing traced native conversion frame with no argument conversions so
the embedding host can supply the runtime zone. The new Date function target
ignores arguments, reads the host clock once and formats the clipped time
without allocating a Date object or invoking overridable prototype methods.
Both effects follow clj-quickjs-182's unrecorded leaf host boundary. The Date
slot and normal access checks remain unchanged. `toJSON` and default Date
coercion still fail explicitly; explicit string conversion can call `toString`.

Source tests `date_to_string_formats`, `date_string_format_receivers_and_descriptors`
and `date_string_named_zone_formats` cover fixed/named zones, DST, minute
truncation, invalid dates, calendar endpoints, alias identity, metadata, argument
ignoring, and clock reads. One-instruction yields with collection check the
native frame's receiver roots. Existing intrinsic/GC counts grow by four.

## Unicode simple folding and version policy (clj-quickjs-191)

The shared Unicode version is 17.0.0 for identifiers and folding; properties
(-192), normalization, collation and segmentation (-209 and their consumers)
must use the same version with service-specific verification. Locale/CLDR
snapshot provenance remains separate; this establishes no CLDR release claim.
The former Perl Unicode 16.0.0 identifier generator is replaced by the same
pinned export/repack pipeline as folding. Complete-domain comparison verifies
both ID properties and all C/S folding mappings against hash-pinned Unicode
17.0.0 CaseFolding.txt and DerivedCoreProperties.txt, including new Beria Erfe
letters. Regeneration rejects unexpected hashes or version headers.

`scripts/quickjs-data/repack-unicode.pl` expands the imported `case_conv_*`
and identifier tables using the hash-pinned QuickJS decoder only offline.
The snapshot chooses FB05 rather than FB06 as its ligature representative;
the repacker asserts exactly that exception and emits the normative C/S
representative with unchanged equivalence. Runtime Rust uses an immutable
sorted 1,512-entry mapping and binary search, preserving lone surrogates and
excluding full/Turkic folds. Verification compares the actual Rust folding
and identifier consumers against the pinned reference at all 1,114,112 code
points. Commands, input hashes and notices are in the data pipeline README
and `src/regexp/case_fold_provenance.txt`. No runtime C, network or dependencies.

iu (and scoped i in u mode) canonicalizes literals, input and backreferences.
Class compilation retains each range and adds the folded targets of mapping
sources inside it; literals are folded directly. Runtime membership uses a
folded input, then applies class inversion. Word escapes and boundaries thus
include long s and Kelvin sign; negative escapes complement that membership.
All added expansion work and members share the existing compilation/search
budgets. Table lookups have a fixed logarithmic bound. Plain i remains rejected
with catchable SyntaxError, including scoped i outside Unicode mode; the
canonicalize helper's false mode is identity and is not a legacy-i operation.

## Promise thenable adoption (clj-quickjs-247)

Resolve reads an object's `then` once on the resolving task. Getter calls
use ordinary resumable frames and a Promise catch boundary: a thrown value
rejects the Promise. Non-callable results fulfill with the object. Callable
results enqueue a `PromiseResolveThenable` built-in job in that task's FIFO
inbox, with the thenable and saved method as traced job arguments. Changing
`then` afterward does not change the scheduled call. `Promise.resolve` on a
non-Promise and normal reaction returns use this same resolving path.

When the job starts it allocates a fresh `HeapObject::PromiseResolving`
record and two immutable resolver built-ins tracing that record. The record
is an explicit concurrent reference containing only a Promise edge and an
already-resolved flag, inspected/claimed under the heap metadata lock. Its
trace visits the Promise; publication stops at the concurrent boundary,
whose only edge leads to another concurrent reference. Unlike storing one
flag on the Promise, a new flag per pair permits nested adoption while
keeping every older resolve/reject pair permanently inert after its first
call. This also preserves cross-task resolver publication and racing claims.

The job calls the saved method with the thenable receiver and fresh pair
through `Action::Call`; no Rust stack or lock survives the call. Callback
returns are ignored. Throws reject only if that fresh guard remains
unclaimed, so resolving with another thenable and then throwing preserves
the pending adoption. All nested calls, getters, finally handlers and
`__rt.need` waits retain normal task-owned VM continuations. Jobs created
by resolution run on the resolving task; reactions still run on their
registering task. Dependency completion only publishes and wakes tasks.

A reaction registered with a matching engine resolve/reject pair also traces
that pair's guard as a forwarding capability. If the source holds a foreign
task-owned result or rejection reason, the reaction rejects both its derived
Promise and the forwarding target with a local TypeError, claiming the guard
first. This prevents intrinsic Promise adoption from leaving its outer Promise
pending when the otherwise ignored intermediate Promise rejects at the
ownership boundary. Ordinary user handlers still cannot receive foreign
results or reasons. `adoption_of_foreign_task_local_result_rejects` covers
both states and preserves the ordinary-handler policy.

Root inventory: `thenable_resolving_functions_are_traced` retains an object
result through a saved fresh resolver, its resolving guard and Promise,
then reclaims that chain. `thenable_jobs_preserve_saved_then_across_budgeting_and_collection`
collects at every single-instruction yield, covering queued arguments,
active native frames, nested adoption and ignored late rejects/throws.
`thenable_then_waits_and_resumes` collects during a method's dependency wait,
migrates the task, and asserts exactly one outer settlement. The `js_source`
case `promise_thenable_adoption` checks exact job order, once-only synchronous
getter lookup, asynchronous method calls, receiver identity and rejection.
`thenable_resolvers_settle_after_then_returns` collects after every instruction
while a saved fresh pair remains unclaimed after `then` returns 99, then checks
later resolve-first and reject-first calls each settle exactly once.

QuickJS references and provenance: semantic reference only, no source or test
text copied: `research/quickjs/quickjs.c`,
`js_promise_resolve_function_call`, `js_promise_resolve_thenable_job` and
`js_create_resolving_functions`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).

## BigInt width wrapping (clj-quickjs-204)

`JsBigInt::checked_as_int_n` and `checked_as_uint_n` borrow canonical magnitude
limbs, short-circuit already-fitting values, then stream at most the requested
low two's-complement words through the existing `complement_word` helper.
The high word is masked at partial widths; signed results convert back to
sign/magnitude in place and pass through `from_sign_magnitude`, restoring the
inline form whenever possible. No 2^bits intermediate or oversized magnitude
is constructed. Widths remain u64 until bounded by MAX_BIGINT_BITS, so even
2^53 - 1 is handled without host-index narrowing. Negative unsigned inputs at
widths above 1 Mbit throw RangeError before buffer allocation; at the cap they
fit. This is the same checked-size behavior as QuickJS `js_bigint_asUintN`,
using our magnitude cap rather than its allocation limit.

Two append-only Builtin variants and two realm function roots expose methods
with length 2 and ordinary method attributes. The existing ToIndex operation
is shared with typed-array conversion via `dispatch::to_index`, retaining the
array-specific platform/allocation checks. `builtin::to_bigint` shares the
BigInt constructor's non-Number primitive conversions; only the constructor
explicitly accepts integral Numbers. The native Convert frame finishes and
validates ToIndex before starting the value's number-hint ToPrimitive. Strings
continue through the existing budgeted StringToBigInt parser, which now retains
an optional width/signedness completion. Even a zero width performs ToBigInt,
including parser errors and side effects, before producing zero.

Wrapping itself remains a single bounded leaf computation, matching the
one-instruction arithmetic policy of clj-quickjs-196. Conversion callbacks
and string parsing can yield, wait, collect and migrate using traced native
frames; no Rust stack is retained and no new heap kind or edges are added.
Tests cover every pinned upstream width assertion, 65-bit cases, independent
small-integer modulo checks, canonical forms, reflection, conversion ordering,
cap boundaries, and callback/parser continuations under collection/migration.

### Intl service construction and PluralRules (clj-quickjs-212)

`intl::service` owns locale lookup over the policy coverage, relevant Unicode
extension filtering, shared SupportedLocales, GetOptionsObject, enumerated
string/numeric validation, and resolved-object field insertion in caller-supplied
spec order. CanonicalizeLocaleList remains the reusable saved `native::intl`
continuation. `native::intl_service` reads an option or returns its getter call,
and returns ToString/ToNumber conversion actions; each caller saves the option
index and result stage before entering JavaScript. NumberFormat now uses these
helpers without output changes and keeps its pre-existing primitive options
boxing; PluralRules uses the pellet's strict undefined-or-object options policy.
Default locale lookup returns `Action::DefaultLocale`, preserving the existing
leaf host effect boundary, with `en-US` as the final fallback. PluralRules
finishes the localeMatcher getter and ToString before requesting the default,
so host changes across either callback wait are observed.

PluralRules stores a cloned immutable Rust record in an ordinary object's
internal slot, reusing NumberFormat's exact decimal rounding engine. The record
contains strings, primitive options and static locale-data references; it has no
GC edges. The object retains the ordinary `Plain` TaskOwned publication category
in `value::share::classify`, including after Object.freeze. Saved native states
trace locale arrays, input/options objects, receivers, arguments and nested
canonicalization state. No heap kind was added. Realm/builtin entries are
appended. Collection, waits, one-instruction budgets and migration are exercised
by source tests.

The standard-notation operand reader preserves exact integer/fraction strings,
visible digit counts v/w, n/i/f/t and exponent e=0. CLDR rule families for the
recorded coverage are evaluated as safe Rust predicates; no external locale
library, C wrapper, runtime generation or OS locale lookup is involved. The
pinned QuickJS snapshot does not carry CLDR plural rule tables and its shortcut
selector cannot meet this pellet's fraction/ordinal acceptance. The finite
predicates were checked against Unicode CLDR release-48 supplemental plural and
ordinal rules, separately from the snapshot's unverified CLDR provenance; see
attribution. Standard notation and default halfExpand rounding are explicit;
compact/scientific exponents and additional rounding controls are deferred.

## Unicode RegExp property sets (clj-quickjs-192)

The shared Unicode 17.0.0 repacker consumes the verified export's NUL-separated
binary/General_Category alias pools and expands their compressed and derived
sets with the pinned offline QuickJS decoder. Only ECMA-262's 53 binary
properties and all 38 category values are exposed. Every set is independently
compared across U+0000..U+10FFFF with pinned UnicodeData, DerivedCoreProperties,
PropList, DerivedBinaryProperties, DerivedNormalizationProps and emoji-data.
QuickJS range sentinels above U+10FFFF are clipped to the ECMAScript domain.
Script/Script_Extensions remain rejected, consistently with the -191 policy.

`src/regexp/property_data.rs` holds sorted inclusive ranges and exact aliases.
Each set also stores canonical targets contributed by nonidentity folds of
members and of nonmembers separately. The matcher canonicalizes its input,
tests raw membership (or its complement), and unions the appropriate folded
contributions. This implements complement-before-folding for P without
changing outer class inversion or expanding a property at runtime. Each
property member costs one existing search step and at most two bounded binary
searches over immutable ranges. Compilation retains a small validated property
identity in the IR and creates no heap edges or dependencies.

The optional UCD verification in `scripts/quickjs-data/verify.sh` compares every
installed Rust property, complement and iu image against the full-domain
reference, checks every alias, proves corruption detection and repeats the
expansion byte-for-byte. Ordinary Cargo builds need only checked-in Rust data.

## Percent and currency NumberFormat (clj-quickjs-214)

The existing immutable NumberFormat record also stores style, normalized
currency code, display and sign options. Construction uses the shared Intl
option reader/converter and allowed-string validator; all new getters and
ToString conversions resume through explicit native stages. Currency presence
is checked after display/sign options and before notation; malformed codes
fail before reading later options. Default fraction bounds are adjusted against
an explicitly supplied opposite bound, using the snapshot's minor-unit rules
(including QuickJS's CLF=3 rather than modern ISO's 4).

Finite percent values shift the exact decimal point by two before rounding;
zero keeps its canonical coefficient. Finite outputs substitute tokens in the
snapshot's decimal/percent/currency/accounting patterns exactly once, preserving
all literal spaces, parentheses and bidi marks. A bounds-checked unsigned
LEB128 reader consumes each locale's currency code/symbol index pairs from the
existing generated pools. Absent symbols fall back to the normalized code.
No generated representation, provenance, locale coverage, heap kind or tracing
rule changes. The record owns only Rust strings and static data references;
its existing task ownership and bound-function tracing rules continue to apply.

QuickJS compatibility deliberately means narrowSymbol/name select the same
symbol as symbol, literal currency codes receive no additional spacing, and
NaN/infinity bypass style patterns. Unit style remains an explicit RangeError.
Consumer tests validate every selected locale currency record, decoder bounds,
locale acceptance strings, minor units, accounting/display options, percent
scaling and resumable callbacks across collection and migration. No generator,
OS locale access or dependency is added.

## Date ISO parser (clj-quickjs-184)

`vm/date.rs::IsoParser` is a pure resumable UTF-16 parser returning integer
calendar fields, an explicit offset, and a local-time flag. Fixed-width fields
require constant work; arbitrary-length fractions validate at most 256 digits
per native step and retain only truncated milliseconds. Total scan work is
O(input length), with constant ISO parser state and no new length limit.
`value/string.rs::StringFlatten` prepares ropes in budgeted node/copy steps,
using an explicitly linked traversal stack so deep trees cannot trigger an
unbounded stack-buffer growth copy. Its output buffer is reserved once without
filling units, as in StringBuild, and transferred to immutable storage without
a final copy. Flat inputs share their existing storage. Rope preparation uses
O(input length + tree depth) storage and visits every node, retaining children
before releasing each parent to bound destruction of uniquely owned chains.
No UTF-8/lossy conversion, NUL termination, or prefix
truncation can discard a trailing invalid code unit.

`date::Parser` is the shared string parsing entry point for `Date.parse` and
single-argument string construction, ready for the fallback grammar in
clj-quickjs-185. `NativeState::DateParse` holds immutable strings, Rust buffers
and scalar parser state, with no collector edges. Conversion preceding it
uses the existing traced Convert operands and resumable ToPrimitive method
lookups/calls. Date.parse uses the string hint (including budgeted BigInt
formatting); construction retains default-hint conversion and Date-slot copy.
After a successful local parse, the continuation obtains the runtime's zone
and calls the existing `TimeZone::utc_time_millis`, preserving earlier-overlap
and pre-transition-gap disambiguation. Date-only and explicitly offset strings
do not select or freeze the runtime's zone. Calendar/offset arithmetic finishes
before TimeClip; invalid parsing returns NaN or constructs an invalid Date.
The new Date.parse intrinsic is allocated after previously rooted intrinsics,
preserving their identities. Source tests cover UTC and UTC+01:00, long
fractions, metadata, coercion order, waits/GC/migration, clipping, and named-zone
DST transitions. Non-ISO fallback parsing remains deferred.

## Array and argument spread (clj-quickjs-130)

The parser represents a spread element/argument as `ExprKind::Spread`,
restricted to array, call and constructor lists. Lowering reuses `GetIterator`
(`native::iterator::State::get_iterator`) and the for-of protocol: iterator,
cached next and result occupy hidden frame locals; `CallMethod`,
`RequireObject` and ordinary property reads implement each step. Each back
edge, call and getter is budgeted and resumable, with no retained Rust frame
or second iterator implementation. Destination arrays stay on the operand
stack. These existing traced roots retain all intermediate values across
collection, dependency waits and worker migration. No new heap kind or root
category is introduced.

Array spread uses the existing own-element append operation. Call spread
builds an inaccessible dense array in source order. `CheckSpreadArguments`
(0x7A) bounds its length after every append to 65,535. Operand-less
`CallSpread` (0x77), `CallMethodSpread` (0x78) and `ConstructSpread` (0x79)
copy its elements directly, check available stack room, and enter the existing
call/construct dispatch. They preserve forwarding, bound arguments, native
continuations, constructor return overrides and method receivers. This final
copy is a bounded leaf operation; all observable iteration and argument
expression evaluation already ran through budgeted VM instructions. The
packed list cannot invoke getters or iterate again at call entry.

## RegExp indices construction (clj-quickjs-193)

`regexp::object::exec` constructs indices directly from the matcher's bounded
capture ranges. With `d`, it allocates one task-owned array plus at most one
two-element array per matched capture, and a null-prototype groups object
when names exist. Each named value and numbered slot hold the same pair
object; duplicate-name selection follows the existing capture groups logic.
No extra capture-range buffer or heap kind is introduced. Existing array
and ordinary-object tracing retains all edges across yield, wait, collection
and migration. Construction is a leaf without safepoints or user callbacks;
it bypasses inherited setters through CreateDataProperty semantics.

The immutable matcher program records the `d` flag. On success, its single
1,000,000-step search budget reserves five extra units per capture before
returning ranges: indices entry, pair allocation, two endpoints and a named
reference. This conservative charge applies even to unmatched/unnamed slots,
bounds allocation by the existing capture bound and prevents the indices
loop from escaping the search allowance. Exhaustion remains a catchable
RangeError. Matching and result offsets retain UTF-16 semantics.
