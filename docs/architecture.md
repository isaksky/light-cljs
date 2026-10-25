# Architecture

A concise map of the runtime's boundaries as implemented so far. Requirements
are in [`design.md`](design.md); the supported/deferred list and exact commands
are in the [README](../README.md). This note describes what exists and labels
planned shapes as planned.

## Optional chains (clj-quickjs-134)

The frontend stores the first optional link and all remaining links in one
`ExprKind::OptionalChain` with a base and a flat list of member, index and call
links. Each link records whether it is optional and its operator-token span.
Parentheses end that list; they preserve the chain node for reference-aware
calls and delete. Chains are never assignment or update targets.

Lowering walks links once and emits strict null/undefined tests followed by
branches to the common chain exit. Short-circuit branches drop the live value
and receiver and produce undefined, or true for delete. Keys and arguments
are emitted after the guard. A member immediately used as a callee retains
its base below the function; reference-mode lowering also does this for
parenthesized member chains. Calls reuse `CallMethod`/`Call` and their spread
variants. Delete lowers the final property as a reference without reading it.
No new opcodes, heap kinds or native continuations are needed: getters, key
conversion, spread iteration and calls use their existing resumable paths.
Native iterator startup and steps bypass the unrelated built-in constructor
and native-operation temporaries. This keeps optional calls with spread
arguments within 64 KiB execution stacks after realm initialization, including
dependency waits and migration.

Semantic references: `optional_chain_test` and `js_parse_postfix_expr` in
`research/quickjs/quickjs.c`, and `test_optional_chaining` in
`research/quickjs/tests/test_language.js`, revision `7d1c13c5d8f8`.

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

### Weak slots and ephemerons (clj-quickjs-172)

`Trace::trace` keeps its strong-edge meaning. A heap kind or `NativePayload`
enumerates mutable conditional storage in `Trace::trace_conditional` by
calling `ConditionalVisitor::weak_slot` and `ephemeron_table`. The visitor
owns discovery and clearing; implementations only enumerate fields, with
the same no-callback/no-heap-access rule as strong tracing. `HeapObject`
forwards the method to its payload. No new object kind is needed; native
payloads with this mutable storage declare `TaskOwned`, and ordinary Map
entries keep their existing strong semantics.

- `WeakSlot::new(target)` holds an optional `ObjectId` without retaining it.
  `get()` returns that identity until collection empties the slot. Reading
  or upgrading requires the owner's normal mutator/heap access discipline;
  root the target before leaving that access. This differs from the
  embedder's lazy, generation-checked `WeakHandle`.
- `EphemeronTable` stores private identity keys and arbitrary `Value`s,
  with `insert`, `get`, `remove` and `len`. Neither keys nor values are
  reported as strong edges. The value marks only if both the table's owner
  and its key mark independently. A value pointing back to its own key,
  or a cycle involving several keys/tables, cannot establish that initial
  reachability.
- Marking drains the strong grey stack and discovers conditional storage
  only in reached owners. An entry with an already-marked key queues its
  value immediately; otherwise a temporary index stores it under the key.
  Marking that key awakens its waiting values once. Values can reach more
  tables, whose entries join the same index. Empty worklists give the least
  fixpoint. Every reached entry is discovered once and activated at most
  once, including the 100-table chain. Expected marking/clearing work is
  O(live objects + strong edges + reached entries/weak slots), with O(live
  objects + reached entries) temporary storage. Sparse tables shrink after
  deletion to keep iteration capacity proportional to entries; the existing
  sweep scans all object slots.
- Before sweep, inside the same pause, the visitor removes entries with
  unmarked keys and empties slots with unmarked targets. Thus surviving
  conditional storage never points into reclaimed/reused slots; generation
  checks remain a second defence. Unreachable owners are swept normally.
- `WeakSlot::notifying(target, token)` requests a `ClearedWeakSlot` when
  emptied. The owner supplies a token unique to that registration. The
  internal `CollectionReport::cleared_weak_slots` list contains owner/token
  pairs, never dead-target identities. Every record registers a root source
  in the appended `WeakNotification` category before the pause ends. It
  roots the owner, whose ordinary strong edges must retain callback/held
  data; it never revives the target. Retaining/cloning a record preserves
  that root through concurrent/reentrant collections, and dropping the last
  copy consumes it. Queues retaining records belong outside the heap, like
  persistent roots; storing them in payloads would root cycles forever.
  Records do not retain the runtime itself.

Observers receive the list only after the rendezvous and all heap locks
end, alongside the ordinary collection report. They may allocate and retain
notifications for later processing; no JavaScript, host callback or payload
destructor runs in the pause. The collector substrate has no JavaScript
surface itself: WeakMap builds on it (clj-quickjs-173); WeakSet remains
deferred, WeakRef uses task job keep-alives (clj-quickjs-176), and
FinalizationRegistry adds registration, held-data and owner-task jobs (clj-quickjs-177).
Collector tests use original native payload fixtures in `heap::gc::tests`.
QuickJS's `test_weak_map_cycles`, `test_weak_ref` and
`test_finalization_registry` (`research/quickjs/tests/test_builtin.js`,
revision `7d1c13c5d8f8`) informed the semantics; its reference-count/cycle
removal algorithm is not used and no upstream text was adapted.

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
| task global declarative environment | `TaskFrame` (`Cell`) | `task_global_lexicals_survive_job_replacement_and_reclaim` *(lib)* | `task_global_lexicals_survive_job_replacement_and_reclaim` *(lib)* |
| module linking continuation and environment cells | `TaskFrame` (`Cell`) | `module_linker_cells_retain_and_reclaim` (`tests/js_modules.rs`) | `module_linker_cells_retain_and_reclaim` |
| module namespace export cells and their values | `PersistentHandle` (`ModuleNamespace` → `Cell`) | `namespace_cells_retain_and_reclaim` *(lib, module::namespace)* | `namespace_cells_retain_and_reclaim` |
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
| ephemeron values, conditional on reached owner and key | `PersistentHandle` (`Native`, conditional storage) | `ephemeron_value_lives_only_with_key` *(lib, heap::gc)* | `ephemeron_value_lives_only_with_key` *(lib, heap::gc)* |
| unconsumed cleared-slot notification owner and held data | `WeakNotification` (`Native`) | `cleared_weak_slots_are_notified_after_the_pause` *(lib, heap::gc)* | `cleared_weak_slots_are_notified_after_the_pause` *(lib, heap::gc)* |
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
| Segments and Segment Iterator internal segmenter slots | `PersistentHandle` (`PlainObject` → Segmenter service) | `segmenter_slots_retain_and_reclaim` *(lib)* | `segmenter_slots_retain_and_reclaim` *(lib)* |
| DisposableStack resources, ordinary properties and prototype | `PersistentHandle` (`DisposableStack`) | `disposable_stack_resources_retain_and_reclaim` *(lib)* | `disposable_stack_resources_retain_and_reclaim` *(lib)* |
| bounded unhandled Promise/reason queue | `TaskFrame` (task inbox, including after completion) | `unhandled_rejection_roots_are_retained_and_reclaimed` *(tests/promises.rs)* | `unhandled_rejection_roots_are_retained_and_reclaimed` *(tests/promises.rs)* |
| WeakRef ordinary properties and prototype | `PersistentHandle` (`WeakRef`) | `weak_ref_properties_and_prototype_are_traced` *(tests/gc.rs)* | `weak_ref_properties_and_prototype_are_traced` *(tests/gc.rs)* |
| task kept objects for the active job | `TaskFrame`, running or suspended task state | `kept_objects_survive_need_wait` *(tests/gc.rs)* | `weak_ref_cleared_after_job_and_collection` *(tests/gc.rs)* |

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
  ordinary function (`ExecutableSpec::constructor_kind`; arrows, built-ins and
  everything else are a catchable `TypeError` "… is not a constructor").
  `ConstructorKind::{None, Ordinary}` (clj-quickjs-135) lives in the immutable
  executable spec and shared `Code`, survives spec round trips, and leaves room
  for class constructor kinds. Only ordinary function declarations/expressions
  receive `Ordinary`; generators, async functions, concise methods, accessors
  and arrows receive `None`. Bound functions check their unwrapped target.
  Generator prototype ownership remains separate from constructor capability.
  Compiler debug metadata maps each construct instruction to its identifier
  or member callee's source span, including computed members and spread calls.
  A rejection quotes that span without evaluating it again or reading `name`;
  its error position still uses the independent `new` token location. Other
  expressions and code loaded without source diagnostics use the value kind
  (or numeric value). No constructor flag or source label is stored per closure.
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

- **Synchronous disposal** (clj-quickjs-236). A statement list containing
  `using` owns a finally handler before any initializer runs. Hidden value
  and method locals retain each resource and its method snapshot; a method
  slot stays undefined until AddDisposableResource succeeds. `using_dispose`
  transfers those pairs into the same traced native disposal loop as
  DisposableStack (clj-quickjs-239), clearing the hidden locals. The loop
  calls backwards through ordinary VM frames under native catch handlers.
  `pending_exception` reads the pending throw; the intrinsic engine helper
  combines errors without running JS. An optional saved error distinguishes
  an empty error state from a thrown undefined. The existing completion stack
  retains returns, jumps, exception identity and cancellation; catches inside
  disposal keep later resources running. `pending_cancel` tells the loop to
  discard the final error during cancellation so `end_finally` can visit outer resource scopes;
  ordinary throws escaping a cancel finally would terminate all cleanup.
  `using_check` has a traced native continuation for a
  `Symbol.dispose` getter. Every getter and disposer can suspend and migrate;
  the collector traces resources, snapshots and errors through existing
  task locals/native frames. Module evaluation uses the same protected body
  around its module-cell bindings. No new heap kind or publication rule.
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
  `JS_STRING_LEN_MAX`, 2^30 - 1 code units; a longer concatenation throws
  catchable `InternalError` "string too long" (clj-quickjs-231).
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

## Interned property-key reclamation (clj-quickjs-259)

**Decision.** Property-key slots participate in the existing precise
stop-the-world collector through `TraceEdge::PropertyKey`. A traced
`PropertyKey` reports its slot and, for a symbol key, the symbol's independent
identity (which also retains symbol-keyed ephemerons). After the entire
strong/ephemeron mark, the collector removes unmarked names and symbols from
both lookup indexes, drops their immutable bodies, and puts the vacant u32
slots on a free list. Only then may mutators resume. No slot held by traced
storage is reissued, and live keys keep their identities and creation order.
Deleted properties and unsuccessful writes retain no slot after collection.

The key table is not a root. Its name/symbol indexes are reconstruction
metadata, never strong symbol edges. A rooted symbol *value* does not need
to retain an unused property-key slot: later writes intern that same symbol
identity again. `Symbol.for`'s separate registry keeps its existing lifetime.
Hash indexes shrink after large transient batches. Slot/free-list metadata
retains its allocation high-water mark and is reused; name and description
storage follows live keys, rather than all names ever written. Collection
scans the slot high-water mark, as the heap's object sweep does.

**Holder inventory.** Every ordinary property store traces keys alongside
values, accessors and prototypes. This covers plain and published/frozen
objects; arrays' named properties; Map, WeakMap, WeakRef, finalization and
disposable stores; ArrayBuffer, typed arrays and generators; and functions'
separate metadata/own-property objects. Promise edges, closure captures,
with-statement object environments, module binding cells and namespace
exports reach these stores through existing tracing. The realm traces its
intrinsic objects and explicitly traces the keys of its cached property
views. Template caches root their arrays, including named properties.

Executable constants, including eval/Function-generated code and child
functions, contain names or symbol values, not property-key ids; there is
no persistent inline key cache. Module records and per-world loader caches
also use strings and executable handles, while namespace exports use
`JsString`. The public embedder API exposes strings/symbols and rooted
values, not raw `PropertyKey`s. No additional key-root API is needed today.
Any future holder/cache of raw ids must implement `Trace` and be reachable
from the root inventory before it crosses a safepoint.

**Operation-local keys.** The `Interned` cache in `vm/property.rs`, key
enumeration snapshots, function-store builders and publication diagnostics
use ids only within a registered, non-safepoint operation. VM polls run
before these helpers; helpers return accessor callees before JavaScript is
invoked. Resumable descriptors, coercion and other native continuations
retain a `Key` name/symbol or a `Value`, and look up the slot again when
needed. A raw key in a Rust local alone is not a root, including inside a
scope that explicitly calls collection. Mutators take the key-table lock
outside other heap locks. The collector sweeps it with only the rendezvous
lock held after releasing its other tracing locks, before ending the pause.

**Alternatives.** Per-key reference counts would add bookkeeping to copied
ids and every temporary operation. Never reusing ids would release text but
eventually exhaust the u32 space under repeated garbage batches. Tracing
uses the existing pause and holder inventory instead. QuickJS's free-slot
list (`research/quickjs/quickjs.c`, `JS_FreeAtomStruct`, revision
7d1c13c5d8f8) is a reference only; its atom reference counting is not used.

Lib tests exercise repeated batches of computed writes returning to the
prior table size, surviving reads and stable live ids, slot reuse across
names and symbols, and collection at every instruction budget boundary
through ordinary/exotic stores, accessors and class fields. An ephemeron
chain retains its conditional value's named and symbol keys until the
activating key dies, then reclaims both slots.

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

**Property keys.** A `PropertyKey` carries a private `u32` issued by the
runtime-wide `PropertyKeys` table owned by the heap (one per runtime), plus
an optional symbol identity for tracing symbol keys without a table lock. Names
are interned on first definition (`PutField`, `DefineField`); `GetField`
only looks up, so reading a never-defined name grows nothing. Keys compare
and hash by their private id (derived Rust equality), never by a user hook;
the table's `HashMap<JsString, PropertyKey>` uses the standard randomized
hasher. Mutators consult its leaf `RwLock` before taking the object lock;
the stopped collector sweeps it with only the rendezvous lock held.
A symbol key (clj-quickjs-127) has an id without a name entry. They are QuickJS's `JSAtom`s but
are not called atoms (`crate::atom` is the concurrent reference).

**Tracing and publication.** `OrdinaryObject`'s `Trace` reports every
property key, value and the prototype; the root inventory lists the holder
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
- Unused interned names and symbol keys are reclaimed at collection
  (clj-quickjs-259, decision below), including keys from failed writes or
  deleted properties. Reads only look names up; array indices on arrays
  are never interned. Between collections the table can grow with new
  writes; after collection it holds only keys retained by traced storage.
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
  `Object.prototype`'s or `undefined`. `Function` compiles dynamic functions
  in the global scope (clj-quickjs-111) and `Reflect` is an intrinsic
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
  key table, each element would cost a key lookup
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
  arrays never do). The collector reclaims unused keys (clj-quickjs-259,
  "Interned property-key reclamation").
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
`src/vm/executable.rs` (`ExecutableSpec::constructor_kind`). Design: `design.md`
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
- **`prototype`.** Ordinary function declarations/expressions have
  `ExecutableSpec::constructor_kind == ConstructorKind::Ordinary` (not arrows,
  methods, async functions, scripts or hand-assembled code). Their function objects have an own `prototype`
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
code's `Code::strict`, as for writes). `delete super.x` throws ReferenceError (clj-quickjs-138); `delete a?.b` (clj-quickjs-134) returns true on a nullish
base and otherwise uses ordinary property deletion. `delete` of an
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
Non-strict `var` identifier heads may retain an initializer in the AST
(clj-quickjs-90, Annex B.3.5). Lowering uses the ordinary assignment path
before evaluating the object, preserving name inference and resumability
without new opcodes or per-loop runtime state. Strict, lexical, destructuring
and for-of initialized heads remain early errors.
Proxy enumeration traps are supported (clj-quickjs-91, below). Property targets are clj-quickjs-87.
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

- Unmodified function, scope and eval fixtures (clj-quickjs-113):
  `tests/fixtures/js/quickjs_test_closure.js` preserves the whole 221-line
  `research/quickjs/tests/test_closure.js`, including its original calls.
  `quickjs_test_language_functions.js` preserves lines 1-38, 271-280,
  445-577 and 641-666 of `research/quickjs/tests/test_language.js`.
  `quickjs_test_builtin_functions.js` preserves lines 1-123 and 403-455
  of `research/quickjs/tests/test_builtin.js`. All use revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
  Selected bytes were compared against that checkout; adapted rows: none.
  Only descriptive headers, the complete copyright/MIT notice from
  `research/quickjs/LICENSE`, and five language / three builtin case calls
  are added. The original caught `__loadScript` calls remain without a host
  loader. These are adapted QuickJS test text, synthetic fixtures, not
  light-cljs output. `tests/quickjs_functions.rs` checks embedded upstream
  line counts and runs the closure file intact and each language/builtin
  case independently, preserving sloppy/strict modes, with bounded
  instruction slices and collection between slices. The rewritten
  `quickjs_closure.js` remains a strict-subset regression.
- Strict nested function-expression name stores (clj-quickjs-93):
  `tests/js_functions.rs::named_function_expression_name_is_immutable_in_nested_closures`
  adapts the strict nested-arrow case of
  `research/quickjs/tests/test_language.js::test_function_expr_name` at revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, retaining the QuickJS MIT
  copyright/license notice beside the test. The test uses the default strict
  compilation mode. The nested function-expression, block-declaration,
  catchability and preserved-identity cases are synthetic fixtures, not
  light-cljs output. Existing lowering and VM behavior satisfy these cases;
  no engine change is required.

- Resource-management fixture (clj-quickjs-242):
  `tests/fixtures/js/quickjs_test_language_using.js` extracts lines 1-32 and
  677-993 of `research/quickjs/tests/test_language.js`, with calls from
  1022-1023, at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`
  (VERSION 2026-06-04). The fixture retains the full QuickJS MIT notice from
  `research/quickjs/LICENSE`. Helpers and `test_await_using` are unchanged.
  `test_using` is adapted: the writes to
  `SuppressedError.prototype.constructor` at upstream lines 866-869 and 882
  are wrapped in `assert_throws(TypeError, function() {...})`, honoring the
  immutable-intrinsics user decision from clj-quickjs-234. Every original
  assertion and expected value is retained; no other upstream lines change.
  `tests/exceptions.rs::quickjs_using_cases_pass` compiles in sloppy mode and
  runs bounded instruction slices, collecting between yields. It requires
  normal completion, no suspended async activations, and an empty
  unhandled-rejection report at quiescence. A never-settling final await is
  rejected by a separate negative control.
  The negative control changes one sync expectation and one async expectation
  independently. The async control observes the Error and its exact assertion
  message through a host effect in the final catch before rethrowing the same
  Error, then requires one unhandled rejection. These are synthetic fixtures,
  not light-cljs output. No engine representation or syntax is changed.

- Sloppy function-expression name stores (clj-quickjs-96):
  `tests/js_functions.rs::sloppy_assignment_to_function_expression_name_is_ignored`
  adapts the two non-eval sloppy cases of
  `research/quickjs/tests/test_language.js::test_function_expr_name` at revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, retaining the QuickJS MIT
  copyright/license notice beside the test. Remaining cases are synthetic.
  The existing `Lowerer::store` function-name branch leaves the assigned value
  on the stack in sloppy code; no new lowering or runtime code is required.
- Object environments (clj-quickjs-110): consulted `js_parse_statement_or_decl`
  (`TOK_WITH`), `js_has_unscopable` and `OP_with_get_var` and siblings in
  `research/quickjs/quickjs.c`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Nested eval cases in
  `tests/js_source.rs::with_statement_resolution` adapt
  `research/quickjs/tests/test_closure.js::test_with`; the file retains the
  QuickJS MIT notice. The label case follows `tests/test_language.js::test_labels`.
  The default Array unscopables names were checked against
  `js_array_unscopables_funcs` in `research/quickjs/quickjs.c`.
  Rust lowering, tracing and remaining fixtures are original. Object
  environments are raw traced captures, interleaved with static lexical
  scopes and eval's private cell-backed variable environments. Binding
  selection lowers to HasProperty and resumable property bytecode, retaining
  its selected object before evaluating RHS code. No Rust frame is retained
  across a getter or setter call.
- Await using (clj-quickjs-240): consulted `check_await_using`,
  `js_op_using_check`, `js_sync_dispose_wrapper` and `js_emit_dispose_scope`
  in `research/quickjs/quickjs.c`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Validation/fallback tests adapt
  `research/quickjs/tests/test_language.js::test_await_using`; the MIT notice
  is retained in `tests/js_source.rs`. Rust code and other cases are original.

- For-using-of heads (clj-quickjs-238): consulted
  `research/quickjs/quickjs.c` `js_parse_for_in_of`, `pop_break_entry` and
  `emit_break`, revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
  The validation and normal-completion cases in
  `tests/js_source.rs::for_using_of_loops` adapt
  `research/quickjs/tests/test_language.js::test_using`; its MIT notice is
  retained in that file. Lowering and remaining fixtures are original.

- DisposableStack (clj-quickjs-239): consulted `js_disposable_stack_*`,
  `js_get_dispose_method` and `js_dispose_resources` in
  `research/quickjs/quickjs.c`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The intrinsic-move scenarios in
  `tests/js_source.rs::disposable_stack` adapt
  `research/quickjs/tests/test_language.js::test_using`, with the MIT notice
  retained in that file. Rust code and other fixtures are original.

- AsyncDisposableStack (clj-quickjs-241): consulted `js_disposable_stack_use`,
  `js_get_dispose_method`, `js_disposable_stack_dispose`, `js_async_dispose_step`
  and `js_async_disposable_stack_proto_funcs` in `research/quickjs/quickjs.c`,
  revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The first five cases in
  `tests/js_source.rs::async_disposable_stack` adapt
  `research/quickjs/tests/test_language.js::test_await_using`; the MIT notice
  is retained in that file. Rust implementation and other fixtures are original.

- Synchronous `using` (clj-quickjs-236): consulted
  `research/quickjs/quickjs.c` `is_using`, `js_parse_var`,
  `js_emit_dispose_scope`, `js_op_using_check` and `OP_using_dispose`,
  revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The validation,
  snapshot and error-chain scenarios in `tests/js_source.rs::using_declarations`
  adapt `research/quickjs/tests/test_language.js::test_using`; its MIT notice
  is retained in that test file. Rust code and additional control-flow,
  module, suspension and cancellation fixtures are original. QuickJS's
  recursive disposal loop and reference-counting representation are not used.

- String length overflow (clj-quickjs-231) follows `string_buffer_realloc`,
  `JS_NewStringLen`, and `JS_ConcatString` in `research/quickjs/quickjs.c`,
  revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, as semantic references.
  No source or test text was adapted. `Fault::StringTooLong` maps to the
  appended `VmErrorKind::StringTooLong` and a catchable InternalError with
  message "string too long". Length guards precede output allocation, including
  rope construction. Heap exhaustion remains uncatchable OutOfMemory; existing
  per-feature argument, property-count and work limits retain their behavior.

- Disposal symbols (clj-quickjs-235) use `research/quickjs/quickjs.c`
  `js_symbol_funcs` and `research/quickjs/quickjs-atom.h` entries
  `Symbol_dispose` / `Symbol_asyncDispose` as semantic references at revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. No source or test text was adapted.

- `tests/fixtures/modules/{test_cyclic_import,fixture_cyclic_import,assert,
  hello_module,fib_module}.js` (clj-quickjs-251) are complete parse-only copies of
  `research/quickjs/tests/{test_cyclic_import,fixture_cyclic_import,assert}.js`
  and `research/quickjs/examples/{hello_module,fib_module}.js`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The full QuickJS license is
  prepended to each; source bodies are unchanged. The frontend tests check
  their exact static module tables without claiming module execution support.

- `tests/fixtures/js/quickjs_modules/` (clj-quickjs-257) contains whole-file
  execution fixtures from `research/quickjs/tests/test_cyclic_import.js`,
  `tests/fixture_cyclic_import.js`, `tests/assert.js`,
  `examples/hello_module.js`, `examples/fib_module.js`, and
  `examples/message.json`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Every JavaScript file has a
  prepended provenance/adaptation header and the full `research/quickjs/LICENSE`
  notice (Copyright 2017-2021 Fabrice Bellard and Charlie Gordon, MIT).
  The same notice applies to `message.json`, copied byte-for-byte without a
  comment header because JSON cannot contain comments. All upstream bodies
  remain byte-for-byte. These are adapted QuickJS module fixtures, synthetic
  fixture text, not light-cljs output. Harness adaptations: the cyclic entry's
  lines 1-5 retain the upstream negative-resolution metadata but expect normal
  completion; `hello_module.js` lines 6-8 use a recording `console.log` binding
  supplied by a synthetic setup module instead of QuickJS-libc output. No JSON
  attributes are added. Only the negative-control test changes source: line 2
  of `fixture_cyclic_import.js` replaces `2 *` with `3 *` in memory.
  `tests/js_modules.rs` runs the complete graphs through `ModuleLoader` and
  `Runtime::spawn_module`, with unlimited and seven-instruction budgets and
  collection between slices. A synthetic observer emits a host effect only
  after the cyclic entry's `assert(f(1), 4)` completes. The negative control
  must fail inside the untrimmed `assert.js` with the uncaught Error message
  `assertion failed: got |6|, expected |4|`, before the observer runs.
  The hello observer checks three recorded console calls, their arities and
  labels, the Fibonacci result 55, and the actual message object's `x` and
  array `tab` (length and each element). It emits primitive observations only
  after those structural assertions pass; it never compares console formatting.

- `new.target` (clj-quickjs-99) uses `research/quickjs/quickjs.c`
  `js_parse_postfix_expr` (`TOK_NEW`), `OP_special_object`
  (`OP_SPECIAL_OBJECT_NEW_TARGET`) and `tests/test_closure.js`
  `test_arrow_function` as semantic references at revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. No source or test text was
  adapted. The parser tracks a separate function context inherited by arrows
  and direct eval, including parameter defaults. `GetNewTarget` reads a traced
  task-owned frame value, initialized to undefined on calls and the resolved
  constructor on construction (bound constructors resolve to their target).
  Arrows capture that immutable value through intervening arrows using the
  existing closure representation and publication checks: undefined captures
  from ordinary calls publish without a binding cell, while a captured
  constructor with a task-owned property record prevents publication. An
  unused target introduces no capture. Direct-eval scope
  descriptors carry an optional source, distinguishing an absent binding at
  script scope from a function binding whose value is undefined. The native
  eval continuation traces the imported value until the eval closure owns it.
  Descriptor validation reserves space for the receiver and optional target
  within the u16 capture limit used by runtime compilation.
  Frames, detached generator/async activations and escaping closures retain
  their usual yield/wait/collection/migration behavior.

- `tests/fixtures/js/quickjs_test_generator.js`,
  `quickjs_test_async_gc.js` and `quickjs_test_async_promise_rejection.js`
  (clj-quickjs-250) extract `research/quickjs/tests/test_builtin.js`
  lines 1, 3–50 and 1007–1064, and `research/quickjs/tests/test_std.js`
  lines 5–20, 289–307 and 309–323, respectively, at revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. All extracted assertion helpers
  and case functions are verbatim, with the full QuickJS license in each
  fixture. Each gets its own invocation. The std fixtures omit line 1's
  shebang, replace lines 2–3's module imports with the runner's
  `Task::install_sim_host_bindings()`, and omit lines 22–23's optional external
  assertion loader. The generator fixture retains the strict prologue and
  `throw_error` support; it needs no `return()`/`throw()` generator methods.
  `tests/quickjs_async.rs` runs each as one task through jobs and virtual
  timers, requires completion without unhandled rejections, and checks both
  `std.gc()` execution and reclamation of the unreachable async activation
  while retaining the completed task. Negative copies change the timer
  assertion's expected counter from 3 to 4 and run that failing assertion in
  a Promise job too, exercising both task failure and rejection reporting.

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
- `tests/fixtures/js/quickjs_test_loop_statements.js` and
  `tests/fixtures/js/quickjs_test_language_labels.js` (clj-quickjs-92):
  `research/quickjs/tests/test_loop.js` lines 1-20, 23-252 and 374-383,
  followed by calls from lines 384-393; and
  `research/quickjs/tests/test_language.js` lines 1-35 and 399-426,
  followed by `test_labels();` and `test_labels2();`, respectively, at revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04).
  Selected bytes, helpers, caught `__loadScript` attempts and assertions
  are unchanged; adapted rows: none. Only descriptive headers, the full MIT
  notice from `research/quickjs/LICENSE` and trailing calls are added.
  Both are synthetic fixtures, not light-cljs output.
  `tests/js_source.rs::quickjs_loop_and_label_cases_run_unmodified` compiles
  each case independently in upstream non-strict mode with every definition
  retained, including the compiled but uncalled `test_cyclic_labels`.
  Runs use bounded instruction slices with collection between slices;
  failures are aggregated so one unsupported case cannot hide later cases.
  All twelve upstream cases complete unmodified, including the Proxy
  for-in case supported by clj-quickjs-91.
- `tests/fixtures/js/quickjs_test_loop_try_catch.js` (clj-quickjs-232):
  `research/quickjs/tests/test_loop.js`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` (VERSION 2026-06-04),
  lines 1-20 (`assert` and the caught `__loadScript` attempt), 254-372
  (complete `test_try_catch1`–`test_try_catch8`) and 395-402 (their calls
  in upstream order), byte for byte. Only the descriptive/license header
  and separators between excerpts are added; adapted rows: none. The full
  QuickJS MIT notice from `research/quickjs/LICENSE` is retained. This
  replaces the earlier re-encoded fixture (clj-quickjs-11/-227).
  `tests/exceptions.rs` compiles it in non-strict mode and verifies normal
  completion with `undefined`; a negative check changes only `"tfag"` to
  `"tfgx"` and requires the uncaught `Error: assertion failed: ...`.
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
- `tests/js_source.rs` `for_in_assigns_member_targets` (clj-quickjs-87):
  the fifth and sixth groups of `test_for_in` in
  `research/quickjs/tests/test_loop.js`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Modified: local `let`
  bindings and boolean completion assertions, including the final target
  value. The test carries the MIT notice. `research/quickjs/quickjs.c`
  `js_parse_for_in_of` (left-hand-side parsing and `put_lvalue` per
  iteration) was consulted as a semantic reference only. Additional
  evaluation-order, setter, failure and resumability cases are synthetic.
- `tests/js_source.rs` `sloppy_for_in_var_initializer` (clj-quickjs-90):
  the final group ("variable assigment in the for in") of `test_for_in`
  in `research/quickjs/tests/test_loop.js`, revision
  `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Modified: the assertion is
  the script completion value and also checks the final binding. The full
  MIT notice in the test file applies. Empty enumeration, evaluation order,
  abrupt completion, closures, name inference and suspension cases are
  synthetic fixtures. `js_parse_for_in_of` in `research/quickjs/quickjs.c`
  was consulted for Annex B initializer restrictions and ordering only;
  no implementation code was copied.
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
  and the asserted condition is the script's completion value, with a
  string-key check added. The original `push` / `toString` form is restored
  in clj-quickjs-264. The test carries the full notice.
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
the same descriptor/publication rules (clj-quickjs-101). Bound construction
sets `new.target` to the resolved target (clj-quickjs-99).

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
points. Total work is O(argument count plus total primitive-string
conversion work), with at most two output units per argument and
`MAX_STRING_LEN` enforced. Both static constructors always select the
existing `NativeState::Convert` continuation through `native::start_builtin`
after call/apply/bind resolution, including native callbacks that are built-ins
(clj-quickjs-265). Conversion finishes at most one argument per native step;
primitive strings, including those returned by object callbacks, use the
existing `StringToNumber` scanner in 256-unit/rope-boundary slices for
flattening, trimming, validation and numeric accumulation. Repeated references
to a long string therefore consume budget and permit GC between slices.
Each string operand moves into the scanner so uniquely owned rope nodes also
release during traversal, rather than all at once at conversion completion.
Only the final O(argument count) encoding/allocation remains a leaf, bounded
by the configured VM value-stack limit (default 2^20 values, with `apply`
checking stack room). Primitive-only access methods retain their leaf path;
object operands select conversion after forwarding resolution.
`NativeState::Convert` retains and traces all relevant operands. Access
converts the receiver with string hint, finishes primitive ToString, then
converts the position with number hint. Static arguments convert with
number hint and validate each code point before any later callback runs.
Completed operands are stored as strings or numbers and never replayed.
The final helper then accesses the string or encodes already converted numbers.
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
a `__proto__: value` prototype setter. Computed keys (-233) and super properties (-138) execute. Generator methods
execute through detachable activations (clj-quickjs-243 below); async methods
execute through promise jobs (clj-quickjs-248 below).

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
before reserving output storage; overlong results throw `InternalError` with
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
`2^30 - 1` with InternalError `"string too long"`. Completed parts move into the
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
retains no frame, function object, source text or task. Capture is bounded by
64 frames and 16,384 UTF-16 units including formatting (clj-quickjs-286).
Each metadata field is capped at 256 units: oversized function names become
`<truncated>` without traversing or flattening their rope, while filenames
retain a UTF-16 prefix followed by `…`. The final line may be truncated.
Smaller rope names use a fallibly reserved traversal buffer without filling
the string's flatten cache. The output buffer is reserved fallibly once,
formatted directly, and transferred through the same checked owned-buffer
conversion as engine messages. Reservation failure propagates as uncatchable
OutOfMemory through constructors, engine-error wrapping and disposal capture.
Constructed Errors store it as
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
JSON tokenization precedes grammar checks, preserving QuickJS's lexical
error precedence and message text (clj-quickjs-162). Tokens retain their
start offset/line/column while the cursor advances; malformed identifiers
and leading-zero numbers report that start, while invalid escapes and
incomplete fractions/exponents report the offending unit. Identifier scans
are budgeted, and unexpected-token messages use QuickJS's bounded 255-byte
message size rather than copying an arbitrarily long token. The inherited
512-container and 4096-member-name limits retain their explicit messages.

Before tokenization, the existing `StringFlatten` cursor reuses flat input
or visits/copies rope nodes/units in steps of at most 256 operations, without
a final buffer copy. Position tracking is constant work per consumed UTF-16
unit, including CR/LF/CRLF handling; failures never rescan the input. The
requested UTF-16/CR/EOF position convention takes precedence where the
pinned QuickJS `get_line_col` counts scalar values/LF only or unterminated
strings use their opening token's position.

`Fault::JsonSyntaxError` carries the input line/column separately from the
message. Dispatch uses the existing engine SyntaxError instance allocation
and prepends `    at <input>:line:column\n` to the usual snapshot of at most
64 JavaScript callers. The call site remains the VM diagnostic; no new heap
kind, error-message parsing, callback or input-length work is introduced in
error construction. The saved stack retains its normal property attributes
and survives frame teardown, rethrows, waits, collection and migration.
Unobserved failures still avoid allocating a JavaScript error instance.

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
value releases an ordinary symbol's body and description once the collector
has also reclaimed any unused property-key entry holding it (clj-quickjs-259).
Live symbol properties retain both the key slot and the symbol identity.
The shared u32 key space bounds the table to 2^32 occupied slots; reads of
absent symbols do not intern. The three well-known bodies (`iterator`,
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

Further well-known symbols
and well-known protocol dispatch are deferred to their own pellets. This change
exposes the three identities without changing iteration, instanceof or
OrdinaryToPrimitive protocol lookup.

Disposal symbols (clj-quickjs-235) extend the same private well-known table with
`dispose` and `asyncDispose`, each backed by its own process-wide OnceLock.
Engine code retrieves their stable identities through
`value::symbol::well_known("dispose")` / `well_known("asyncDispose")`, without
looking up a JavaScript global. Realm construction exposes both with FIXED
property flags. Descriptions match QuickJS (`Symbol.dispose` and
`Symbol.asyncDispose`); these immutable leaves retain the existing publication,
GC and property-key representation. No disposal callbacks, iterator disposal
methods, syntax or stack types are introduced.

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

Each native transition does one property/structural operation, visits at
most 256 rope nodes/input code units while quoting, or copies at most 256 final
output units/part boundaries. Values and member names use a saved UnitCursor
without flattening the input (clj-quickjs-289); the cursor takes ownership so
consumed nodes are released incrementally. One pending high surrogate
preserves pairing across rope boundaries and yields; end of input escapes it
if unpaired. Quoted chunks hold at most 1542 units, so their allocation and
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
Replacer arguments other than undefined/null fail explicitly. Boolean wrappers
remain an absent prerequisite. Symbol primitives follow the omission rules;
enumerated symbol keys are filtered before any value read. The bounded
property preflight includes stored symbol keys, keeping the shared leaf
enumeration bounded even when none of those keys contribute JSON members.

The shared `json::check_key_snapshot` preflight covers all property stores
that own-key enumeration reads (clj-quickjs-288), including collections,
generators, class instances and function property records. It counts virtual
typed-array and String-wrapper keys and namespace exports plus the namespace
tag before the leaf collection. Ordinary stores scan at most 4097 entries.
JSON's Proxy operation state carries the same bound through nested operations:
it rejects oversized unfiltered ownKeys result lengths before element reads
and rechecks each leaf target just before collecting keys, after any trap,
length getter or element getter has mutated it. Other Proxy consumers keep
their existing limits. Serialization still omits functions, converts
Number/String wrappers and streams array indices without key snapshots;
replacer arrays/functions still fail explicitly before reading the value.

Indentation (clj-quickjs-161) saves the original space value as a traced
continuation root until its optional Number/String wrapper conversion has
completed. These conversions run before root `toJSON` and use the existing
VM conversion frames, including the 4096-unit Number conversion bound.
The resulting gap owns at most 10 UTF-16 units; a saved UnitCursor visits at
most 256 rope nodes/units per transition and stops after the prefix without
flattening the original string. A cleanup stage releases at most 256 pending
cursor nodes per transition, detaching children before dropping uniquely owned
rope nodes so their destructors cannot cascade through a deep subtree; shared
subtrees require no cleanup walk.
Container depth supplies the indentation width without
recursive traversal or copying an accumulated indent per level. An explicit
indentation stage emits at most 256 code units per transition, saving the
remaining width and the next action (key, value or closing delimiter).
Object separators are emitted only after omission checks, so an all-omitted
object stays compact. Indent chunks use the same checked output parts and
budgeted final assembly as quoted strings; no new heap representation is added.

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
currently unbounded. This `Symbol.for` registry is separate from the
collector-reclaimed property-key table (clj-quickjs-259).

`Symbol.for` and `Symbol.keyFor` append to the built-in registry and are realm
methods with the existing METHOD flags. The leaf `for` path completes primitive
ToString before locking. Object keys use a distinct native conversion target
with string hint; getters and conversion methods are ordinary resumable VM
frames, including through dependency waits, GC and migration. The completed
primitive undergoes ToString before registry access, so returning a symbol
throws TypeError and returning undefined names the string "undefined".
`keyFor` validates its argument without coercion before looking up its identity.
Symbol.prototype is supported below (clj-quickjs-129); general well-known
protocol integration remains separate work.

## JavaScript Map objects (clj-quickjs-169)

`HeapObject::JsMap` holds a boxed `object::map::MapObject`, distinct from
`HeapObject::Map` (the immutable persistent HAMT). It embeds an ordinary
object for ownership, prototype, own properties and extensibility, plus a
tree of live key/value entries ordered by logical insertion position and a
private hash index from hash to entry positions. Only the owning task reads
or mutates it, under the same short object-table lock as ordinary objects; publication
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

Updating an existing key preserves its logical insertion position;
reinserting receives a new position. The reclamation policy below
(clj-quickjs-287) replaces permanent tombstone slots. `Trace` reports the
ordinary object's edges and both key and value of every live entry. The root inventory test
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
entry stage and a stable logical insertion cursor rather than a separate
cursor object.
The receiver is validated before the callback's callability, even for an
empty Map. The state traces the Map, callback and `thisArg` through the
existing native-frame root path.

Each budgeted step reads at most one live entry under a short heap
read, advances the index, and releases the read before returning a callback
action. Current keys/values
move to the ordinary callback arguments; callback return values are ignored.
After a callback, the next native step re-reads the Map, observing changed
values, deleted entries and appends. `clear` removes pending entries while
later appends are visited. Exceptions and cancellation use the existing
native unwind/drop path.

`map_for_each_live_iteration` covers live mutation and the adapted 1000-entry
deletion loop. `map_for_each_suspends_in_callback` waits on the second entry,
collects while parked, migrates to a new thread, and finishes under one-step
budgets without replaying effects, including an entry appended after resume.

### Map storage reclamation (clj-quickjs-287)

`MapObject` stores live entries in a `BTreeMap` keyed by monotonic private
`u64` insertion positions. A hash index still resolves keys by SameValueZero,
including collisions; its positions refer to the tree. Deletion eagerly
removes a tree entry, and `clear` releases all entries and hash storage.
There are no tombstones to compact or trace. Hash-table and collision-bucket
capacity shrink when capacity exceeds four times the remaining length,
amortizing rebuilds over deletions. Storage is O(live entries), independent
of lifetime mutation count. Clear and tracing visit O(live entries), and
ordered entry lookup/removal costs O(log(live entries)); key hashing and
collision comparison costs remain as described above.

A cursor saves the position after its last visited entry. Its next step
selects the first live position at or after that cursor, in O(log(live
entries)). Positions are never reset by clear or storage reclamation, so
multiple nested or suspended cursors see appends and reinsertion without
repeating earlier entries. No registration, drop cleanup or remapping of
native frames is required. `u64::MAX` is reserved as an end cursor; exhausting
positions throws RangeError on new insertion before changing storage,
while existing-key updates still succeed.

Reclamation runs only inside the existing task-owner-checked Map mutation
boundary under the heap object-table lock, without callbacks or safepoints.
Mutable JavaScript Maps remain unpublishable, including when frozen.
Persistent HAMT maps use separate immutable storage and are unchanged.
WeakMap uses a separate ephemeron table whose existing deletion/GC policy
shrinks capacity; it has no public enumeration. Map entries/keys/values,
Map for-of and JavaScript Set remain deferred. Future Map iterators must
save logical positions and latch completion rather than retain tree nodes.

`churn_compacts_without_live_cursors` reproduces both delete and clear churn;
`deletion_reclaims_storage_with_live_cursors` checks retained hash capacity
and two cursors after mass deletion. The synthetic source test
`map_for_each_compaction_preserves_suspended_cursor` checks nested cursors,
churn, deletion, update, clear and later appends across a dependency wait,
collection, one-step budgets and migration.

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
| Display names | Exactly `en` and `fr` translated data; DisplayNames (-223) preserves the shared test-locale resolution and supportedLocalesOf metadata required by its QuickJS acceptance rows, with English name-data fallback for other resolved languages. This refines the initial service-coverage policy without claiming additional translations. |
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
The owner clears WeakRef kept objects there (-176); terminal failure releases
the whole VM state. `Complete(value)` reports the original body's value only once the
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
test text was adapted. Promise settlement/reactions, async resumes, timers and
WeakRef kept-object clearing are implemented by subsequent pellets.
FinalizationRegistry producers use the same queue (clj-quickjs-177). Unhandled-rejection checks run
only at quiescence after jobs and timers drain (clj-quickjs-249 below).



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

Objects support the code-unit and Unicode matcher, d/g/m/s/u/v/y and iu/iv, named captures with a
null-prototype groups object, and explicit undefined elements for unmatched
captures. Plain i constructors and literals throw SyntaxError naming the
unsupported flag; lookbehind also fails explicitly. Symbol.match
recognition, symbol-based string consumers, non-Unicode folding
remain later work. The raw NUL upstream case is exercised through the
constructor and a literal containing an actual NUL; indirect eval is also
available since clj-quickjs-106.

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

For-using-of heads (clj-quickjs-238) reuse `disposable_scope`, the same
finally-based lowering used by statement-list `using` declarations. After
each step produces a value, lowering enters a fresh disposal scope inside
the loop's IteratorClose handler, validates/snapshots its method with
`using_check`, then initializes the fresh const binding. The loop exit entry
surrounds the disposal entry: same-loop continue disposes without closing;
break, return, throw, outer jumps and cancellation dispose before closing.
The body's scopes nest inside the head resource scope. A validation or
disposal failure occurs with the consumer flag set, so IteratorClose runs.
Registration retains resource/method snapshots in traced frame locals.
With clj-quickjs-239, scope exit transfers them into the shared `using_dispose`
native loop; its remaining resources and pending errors are traced there.

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

The immutable matcher program stores a Unicode-mode bit (`u` or `v`
IR), and character instructions retain u32 code points. The shared forward
reader decodes UTF-16 pairs and preserves lone surrogates. Literals, dot,
classes and backreferences use that reader; the search loop uses the same
AdvanceStringIndex operation as global replacement's empty-match progress.
Compilation validates code points through U+10FFFF in Unicode mode and code
units otherwise.

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
The two internal rejection-tracking transitions feed the bounded per-task
quiescence diagnostic described under clj-quickjs-249 below.

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
`test_async_promise_rejection`, using an optional simulated-host timer;
that file preserves the QuickJS LICENSE notice. Other fixtures are
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
task-owned result or rejection reason, the reaction rejects the forwarding
target with a local TypeError, claiming the guard first, and fulfills its
derived Promise with undefined (clj-quickjs-249). This models a resolver's
normal return and avoids a false unhandled rejection for the discarded
intermediate Promise during adoption. Ordinary user handlers still cannot receive foreign
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
DST transitions. Non-ISO fallback parsing is described below (-185).

## Date fallback parser (clj-quickjs-185)

After ISO failure, `date::Parser` switches to `FallbackParser` over the same
immutable UTF-16 buffer. ISO suffix recognition includes `Z`, `±HH:mm` and
`±HHmm` on date-only and time forms, so an ISO offset cannot be mistaken for
a fallback signed year. Its constant-size fallback state contains scalar fields,
three pending date numbers, presence flags and an explicit scan mode. Digits
(including arbitrary leading zeroes), whitespace/separators, ignored words and
nested comments advance incrementally; each native step allows 256 state
transitions. Month/zone prefix matching and time-zone lookahead have fixed
bounds. Numeric overflow attempts can revisit a digit run at most a constant
number of times. Overall work remains O(input length), with no fallback buffer
or collector edges and no new input-length limit. NUL is an invalid unit,
rather than an end marker. U+2212 maps to an ASCII minus for fallback syntax;
other code units are inspected without lossy conversion or prefix truncation.

Field ordering and two-digit year selection follow QuickJS, independently of
heap operations and zone lookup. The parser returns the existing `ParsedDate`
representation; the existing continuation obtains the runtime zone only for
local results and uses `TimeZone::utc_time_millis`. Explicit offsets never
freeze the local zone. Source coverage includes all three simulated fixed
zones, named-zone DST gaps/overlaps, `toString`/`toUTCString` round trips at
calendar/clipping boundaries, long scans, and waits/GC/worker migration.

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

## Resumable Array.from (clj-quickjs-207)

`NativeState::ArrayFrom` holds the source, mapper/thisArg, output, iterator,
cached next, current result, acquisition state and index in task-owned traced
storage. It selects iterable versus array-like after one @@iterator read and
uses `iterator::State::with_method`, the existing GetIterator continuation with
that observed method. No second iterator protocol is introduced. Indexed
reads, ToNumber of an object length, mapping, and return lookup/call yield
trampoline actions. Output append is CreateDataProperty through the existing
fresh-array append path; each element requires budgeted native steps.

NativeCatch now also delivers the complete Exception to ArrayFrom after the
callback's own catch/finally handlers. A boundary covers only mapper calls and
output append, then return lookup/call during close; next/done/value failures
propagate directly. Step failures at these boundaries are caught as well as
callback failures. Close retains and traces the original Exception, ignores
return errors/results, then rethrows with its original diagnostics. Cancellation
still discards natives and never invokes native catch or iterator return.
No Rust frame, heap lock or borrow survives a callback/wait. Tests collect
between every instruction, wait inside every observable operation, and migrate
to another host thread without replaying effects or losing output prefixes.

Array-like lengths above the existing u32 array limit fail before reading any
indices; arrays grow one element at a time rather than allocating the entire
length up front. Custom/subclass and bound constructor receivers explicitly
fail pending subclass support; Array/nonconstructor receivers create ordinary
arrays. Boolean primitive wrappers have no prototype support yet; Symbol
prototype access is supported (clj-quickjs-129).
The Array.from built-in and intrinsic allocation are appended to their existing
registries, preserving earlier intrinsic identities.

## Detachable task-owned activations and generators (clj-quickjs-243)

A generator is a task-owned heap object with ordinary properties, an owner task,
and one of SuspendedStart, SuspendedYield, SuspendedYieldStar (clj-quickjs-245),
Executing or Completed. Suspended
states own a `DetachedActivation`: shared `Arc<Code>`, the callee, receiver,
program counter, retained arguments, locals and operands, handlers (including
pending completions) and native frames. Frame indices, handler heights and native
stack/depth indices are relative to the saved origin. Detaching moves storage
out of `VmState`; attaching rebases it above the caller after resource checks.
This activation container is independent of the generator protocol and can be
reused for async suspension without retaining a Rust stack or worker identity.

The compiler emits a generator marker, normal parameter initialization and an
initial suspension instruction before body execution. Default expressions thus
run at call time, including through ordinary resumable callbacks and dependency
waits. Initial suspension allocates the generator with the function's object
`prototype`, or the realm's generator prototype for a non-object property.
Generator functions materialize their own prototype/property record at creation
and remain non-constructors. This task-owned record prevents publication of the
function before a call can need its prototype; failed publication (including
Promise settlement) leaves the function callable in its creating task.
`next` uses a native boundary: attach, run ordinary VM frames, then return an
ordinary iterator result. Yield saves the activation and delivers a value;
return completes it. A native exception boundary lets inner catch/finally run
before completing the generator and rethrowing the full original exception to
the next caller, preserving primitive-throw locations and call chains. Cancellation
runs attached finally blocks and completes abandoned generator boundaries.
Detached generators are inert traced objects; attached frames retain ordinary
`__rt.need`, budget, safepoint, migration and cancellation behavior.
Yielding from a finally block holding the task's pending cancellation abandons
that block and unwinds to enclosing finally blocks, like a dependency wait
during cleanup; it cannot detach the cancellation and revive the next caller.
A resume refused by VM limits preserves the suspended activation for retry.

`Trace` follows saved arguments, locals, operands, handlers, native frames, code,
callee, receiver and ordinary properties. Executing objects are retained by the
native boundary; unreachable suspended objects and their saved values are
collected together. `classify` marks generators TaskOwned; publication rejects
this kind and reports its owner, and foreign property access/next is rejected
with the existing ForeignObject/ForeignBinding policy. Freezing their ordinary
properties does not freeze their execution state or make them shareable.
The realm generator prototype exposes the inherited `Symbol.toStringTag` value
`Generator` with a non-writable, non-enumerable, configurable descriptor. Its
builtin tag fallback is Object when a custom prototype lacks that symbol;
general tag consumption by Object.prototype.toString remains explicitly deferred.

QuickJS references and provenance: semantic comparison with
`research/quickjs/quickjs.c` `async_func_init`, `async_func_resume`,
`js_generator_function_call`, `js_generator_next`, `OP_initial_yield`, `OP_yield`
and `js_create_iterator_result`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The four next-result rows from
`research/quickjs/tests/test_builtin.js` `test_generator` are expressed as new
synthetic assertions in `tests/js_source.rs::generator_next_and_yield`, with let
bindings and one combined failure check. No upstream source/test text copied.
Generator return/throw methods remain deferred; delegated yield is implemented
by clj-quickjs-245 below.

The generator prototype inherits the existing `%IteratorPrototype%` installed by
clj-quickjs-237, including its identity `[Symbol.iterator]`. The realm builder
returns that shared prototype with its array/string iterator prototypes; no
second iterator identity or iterator prototype is allocated. Generator objects
therefore work with the existing resumable GetIterator/for-of protocol.
Array and argument spread reuse that protocol as well; generator integration
tests exercise both consumers after clj-quickjs-130.

Root inventory: `suspended_generator_is_traced_and_reclaimed` in `tests/gc.rs`
retains an ordinary object solely through a detached generator's saved local,
collects while the generator is retained by a waiting task's local, verifies the
value after resume, then releases the generator and checks both reclamation and
live-generator counts. The append-only SuspendedGenerator inventory holder maps
this coverage to the task-frame root category and the Generator heap kind.
`suspended_generator_operands_and_completions_are_traced` additionally collects
with an object held only by a saved operand or pending return/throw completion,
then resumes from deeper callers and checks its value and reclamation.

## RegExp Unicode sets (clj-quickjs-194)

The parser emits `Node::UnicodeSet` as a flat postfix expression of code-point
members, strings, union, intersection, subtraction and complement. Nested
classes use explicit parser frames; static MayContainStrings tracks syntactic
possibility through each operator and rejects invalid negation. No recursive
Rust traversal or destruction depends on class nesting depth.

`regexp::sets` evaluates the expression under the current scoped `i` modifier
to sorted disjoint code-point intervals and finite strings. It reuses the
verified Unicode 17 property ranges and simple-fold mappings. Case folding
computes each operand's canonical image before set operations, including
complement, so `vi` property complements differ from `iu`. No data generation,
new runtime dependency or GC edge is introduced.

The compiler lowers strings to ordinary Char/Split/Jump alternatives in
longest-code-point-length order, then a code-point class, then an optional
empty match. Existing explicit backtracking, capture undo, lookahead and
repeat progress checks handle these alternatives. Interval scans, string
comparisons, sort bounds and instruction emission consume the compilation
budget; matching consumes the existing shared search budget. Exhaustion is
catchable RangeError. Programs retain the same immutable Arc storage boundary
and RegExp instances remain task-owned. String-property decoding is deferred
and unsupported string property names throw SyntaxError.

## Resumable Array.prototype.sort (clj-quickjs-208)

`vm/native/array_sort.rs` holds a task-owned snapshot, two merge buffers, run
width, merge cursors and pending conversion/string state. A bottom-up merge
sort moves one element per step and chooses the left run on equality; no Rust
stack represents a pending comparison. The collector traces the receiver,
comparator and both buffers, including objects removed from the receiver by a
callback. Cancellation or exceptions discard the native through the existing
unwind protocol, preserving the original callback exception.

Each scan, merge selection, write and delete is budgeted. Comparator calls,
ToString/ToNumber and accessors use ordinary trampoline actions; completed
comparisons are never replayed after waiting or migration. Default comparison
incrementally flattens ropes and inspects at most 256 UTF-16 units per step.
Undefined values bypass coercion/callbacks, sorting before holes omitted from
the snapshot. Writeback uses throwing resumable Set, followed by throwing
Delete for trailing holes. The built-in and realm allocation are append-only;
there are no new heap kinds or publication categories. QuickJS's
`js_array_sort`/`js_array_cmp_generic` in `research/quickjs/quickjs.c` at
7d1c13c5d8f8 were consulted as semantic references; no C code was adapted.

## Generator operands and handler rebasing (clj-quickjs-244)

The generic activation transfer introduced by clj-quickjs-243 already moves the
whole value-stack suffix starting at the frame's base, including operands above
its locals. Lowering leaves the left operands of `a + b * (yield c)` on that
stack; no spill locals are required. Yield removes only its outgoing value, and
next pushes the incoming value before continuing the expression.

Detachment moves all handler entries of the frame, subtracting the frame index
and stack base from their frame and height fields. Attachment adds the new
caller's frame count and stack height. Catch/finally targets and pending jump
resume PCs stay code-relative. Normal, return, throw and jump completions move
with their handlers; saved return values and exceptions are traced by
`DetachedActivation` when suspended and by `VmState` when attached. Native-frame
indices use the same transfer boundary. This representation remains independent
of the generator protocol for reuse by async activations.

`generator_yield_keeps_operands_and_handlers` in `tests/js_source.rs` locks down
these existing transfer rules, alternating one and three helper frames in both
directions and keeping extra caller operands below the generator. It covers
multiple pending operands, caught exceptions, throws after resume and yields
with normal, return, throw and jump completions in finally blocks.
`generator_finally_waits_after_resume` in `tests/exception_waits.rs` collects
while an object is held solely by a detached pending return/throw, then resumes
into a finally block that waits on `__rt.need`. A second collection and thread
migration preserve the attached completion; effect logs assert no cleanup replay
and the final iterator result or caught exception retains the object's value.
No dispatch stack-size or generator return/throw-method change is needed.

QuickJS references and provenance: `research/quickjs/tests/test_builtin.js`
`test_generator`'s f3 case, and `research/quickjs/quickjs.c` `OP_yield`,
`done_generator`, `async_func_resume` and `js_parse_assign_expr2`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The f3 test uses `let` in place of
upstream `var` as requested by the audit (clj-quickjs-94); its expected rows
remain `{1, false}` and `{6, true}`. The QuickJS copyright and MIT permission
notice accompany that adaptation in `tests/js_source.rs`. Other cases are
synthetic fixtures, not light-cljs output. No QuickJS implementation is copied.

## Array patterns (clj-quickjs-131)

`frontend::ast::Pattern` is shared by declarations, parameters, catch bindings
and assignment expressions. Identifier, array, default and property-target nodes
keep source spans and bounded heights. Binding-name traversal supplies scope
analysis and var hoisting; property targets occur only in assignment patterns.
Object nodes extend this representation in clj-quickjs-132.
An assignment ends at its RHS span; parentheses around the whole assignment
extend only its outer span. Cover grammar conversion uses that distinction to
reject `[(a = 1)]` while accepting `[(a) = 1]`, without enlarging AST nodes. Directive
checking borrows parameter names only when a strict directive requires them;
it does not clone parameter strings for every body statement.

Lowering consumes one value with `pattern_store`, either initializing bindings
or assigning references. Array consumers reuse the for-of/spread GetIterator,
advance and IteratorClose lowering. The iterator, cached next method, result,
exhaustion flag and close state are hidden frame locals traced by the VM.
Elisions skip the value getter; rest uses the existing array append operation.
Assignment property references are evaluated before stepping and remain on the
traced operand stack through defaults and calls. Nested patterns install nested
finally handlers. Computed keys remain unconverted until PutElem writes the
value, matching [EvaluatePropertyAccessWithExpressionKey](https://tc39.es/ecma262/multipage/ecmascript-language-expressions.html#sec-evaluate-property-access-with-expression-key).
Exhaustion and abrupt next/done/value access suppress closing;
consumer errors close live iterators, with the existing PendingThrow/catch
sequence preserving the original exception over a failure in return.

Pattern parameters reserve incoming argument slots separately from bound names,
initialize names from left to right with TDZ checks, and use the existing
parameter/body scope separation. Identifier-only parameter lists retain their
original slot layout and default lowering. No new opcode, heap kind, native
iterator protocol or Rust callback stack is introduced. Iterator methods,
getters, default expressions and target setters remain resumable VM calls,
including dependency waits, budget yields, collection and worker migration.

### Intl.Collator root order (clj-quickjs-218)

`intl/collator.rs` owns immutable root options and an incremental comparison
state, separate from the resumable locale/option/coercion stages in
`vm/native/collator.rs`. Service resolution and option getters use the shared
Intl helpers. One boxed slot payload holds immutable options and a cached
bound function, whose cycle back to the receiver is traced by the collector.
Keeping that cache out of ordinary objects' inline storage preserves execution
on the existing 64 KiB worker-stack tests. A GC test retains and then reclaims
both the receiver and detached-callable cases of the cycle.
The object remains task-owned even when frozen; all public entry points check
ownership and internal-slot brand. Compare copies the edge-free options into
its continuation, then requests two resumable ToString operations in order.
When used by `Array.prototype.sort`, this bound native continuation nests
inside the stable merge-sort continuation. Object coercions can wait while
both continuations remain traced across collection and worker migration.

`scripts/quickjs-data/repack-collation.pl` extends the verified snapshot pipeline
by offline-expanding its compressed NFD, full folding and combining-class
records through pinned `libunicode.c` / `cutils.c`. The generated immutable
`intl/data/collation.rs` is self-contained, with no runtime C, generator,
network or dependencies. `verify.sh` compares every emitted value to the pinned
reference decoder across the Unicode domain, completes one-step fold mappings
to an idempotent closure (including capital sharp S), repeats expansion byte for
byte and detects a deliberately corrupted value. Consumer tests separately
check cross-scalar canonical ordering, decomposed Hangul and supplementary
characters. Unicode policy remains the existing QuickJS Unicode 17 snapshot;
no independent CLDR/DUCET release is inferred.

The documented root rule set uses folded primary NFD scalars with the
snapshot's five combining-mark blocks removed, then folded accent keys, then
case-only weights derived from the shared pinned Unicode Uppercase property.
Case weights align with fold expansions, so same-case variants such as long s
and final sigma compare equally at case sensitivity. Lowercase precedes
uppercase by default; explicit upper reverses their weights. Only variant
sensitivity compares the final case-preserving NFD fallback. Numeric runs
retain digit identities in every key so leading zeroes remain equivalent.
Advertised locales use this root with the separate German/Turkish tailoring
stage described below (clj-quickjs-219). Complete UCA weights
remain deferred. Numeric digit-run ordering and stable getter identity follow
the acceptance contract, correcting the snapshot's unused numeric flag and
fresh-function getter. Punctuation filtering follows the snapshot, including
its removal of combining marks when ignorePunctuation is true.

Each comparison step charges at most 128 traversal/key/ordering operations.
`value::string::UnitCursor` explicitly visits rope nodes and UTF-16 units,
avoiding an unbudgeted lazy flatten; surrogate decoding preserves lone halves.
Single-code-point decompositions are statically bounded; cross-code-point
canonical reordering saves its insertion position after each swap. Keys and
all comparison cursors live in native state across yields. Digit runs scan
one scalar per operation, skip leading zeros, compare significant lengths and
then digits, with no float/integer overflow. No Rust stack or heap borrow
survives an option getter, coercion, wait, collection or worker migration.
Collation-option validation uses the same saved rope cursor in 128-step chunks;
fixed enumerated options reject overlong strings by cached length before the
shared option helper can flatten them.

### Intl.Collator German and Turkish tailorings (clj-quickjs-219)

The same collation repacker now pins `intl/collator.c`, extracts its pure
`transform_string` and classification helpers, and runs that transform only
offline with allocation adapters. Generated tables contain German phonebook
and default-sort primary/secondary weights for NFD A/O/U/a/o/u + diaeresis,
and Turkish dotless-i weights. Both levels are checked equal before emission;
the independent Rust verifier compares every emitted weight to the C stream.
Repeat generation, deliberate phonebook corruption and modified-source guards
run in `verify.sh`. No QuickJS service is called by the runtime.

Immutable Collator slots record the selected collation and locale flags.
Shared Intl resolution retains `co`, `kn` and `kf`; the service removes
unsupported keys, and conflicting explicit collation options remove `co`.
Only German sort selects `phonebk`. German search resolves to `default` while
using phonebook comparison weights, matching the snapshot's search transform.
Option validation keeps a bounded exact-name match alongside its budgeted
rope cursor, without flattening or storing an input-sized option buffer.

The key builder matches at most two NFD scalars per step, before full folding.
German phonebook/search emits ae/oe/ue; default German sort emits a/o/u plus
the snapshot's synthetic U+0001 weight. Consumed diaereses remain in the variant
fallback unless punctuation filtering removes marks. Case weights align with
the tailored expansion, using the existing Unicode case policy. Turkish ı
emits h + U+FFFF, placing it before i. I and İ retain root NFD folding, as in
the pinned QuickJS transform: the snapshot does not pair I with dotless ı at
base sensitivity or implement full Turkish contextual case folding. This
preserves the recorded snapshot-output policy. Azerbaijani is outside the
agreed test-locale inventory; additional collation types and full UCA remain
deferred. All expansions fit the existing three-scalar reserved key bound.
Tests cover normalization, option precedence, sort and search, plus tailored
rope comparison with waits, collection and worker migration.

## Exact fixed decimal formatting (clj-quickjs-150)

`src/number.rs` owns the shared `Decimal` coefficient/point representation,
decimal rounding modes and point layout formerly in `intl::number_format`.
NumberFormat and PluralRules retain their decimal-input semantics.
`Decimal::from_number` generates exact binary64 digits for Number formatting,
so `toExponential`/`toPrecision` can reuse the generator and rounding methods.
It extracts the unsigned significand and exponent, cancels denominator powers
of two, and uses the existing `JsBigInt` limbs: `m / 2^k` becomes
`(m * 5^k) / 10^k`, while nonnegative binary exponents shift the coefficient.
This adds no dependency or separate big-integer representation. Binary64
bounds the leaf computation to exponent magnitude 1074, coefficient size
2547 bits and 767 decimal digits; digit rounding scans at most that buffer.

`number::to_fixed` rounds those exact digits with `halfExpand` and pads up to
100 places. It records negativity with `x < 0` before taking the magnitude,
preserving a negative rounded zero while canonicalizing an input negative
zero. Nonfinite values and magnitudes at least 1e21 use the shared ToString.
The builtin validates `thisNumberValue` before converting its digit count,
then performs ToIntegerOrInfinity and the 0..100 check before formatting.
Object counts run through `native::ConvertTarget::NumberToFixed`; the saved
f64 and traced conversion operands survive yields, dependency waits, GC and
migration. The realm method has length 1 and ordinary method flags.
Digit-count strings are limited to 4096 UTF-16 units before leaf StringToNumber
can scan or flatten them, including strings produced by resumable callbacks.
Oversized strings throw RangeError after receiver validation and ToPrimitive;
generic budgeted numeric conversion remains clj-quickjs-265.

## Generator delegation (clj-quickjs-245)

The compiler lowers `yield* operand` into GetIterator followed by an ordinary
VM loop. Task-owned hidden locals hold the iterator, cached next method, current
result object and received argument. Each iteration calls next with the iterator
as receiver and exactly one argument (undefined initially), validates the result
as an object and reads done. If truthy, value becomes the expression result;
otherwise the append-only GeneratorYieldStar instruction detaches the activation
with a SuspendedYieldStar state. The next boundary returns that result object
unchanged, without reading value or wrapping it. Resuming either suspended yield
state uses the same generic activation attach/check/rebase implementation.
Protocol errors follow ordinary JS handlers, and every getter/call is resumable
through existing bytecode/native frames. Locals are cleared on normal exhaustion.
Cancellation continues to use task unwinding; generator return/throw methods
remain absent, so forwarding those resumes remains an explicit grammar gap.

`generator_yield_star_delegation` covers result identity, cached acquisition,
receiver and argument forwarding, array iteration, return values and catchable
TypeErrors. `yield_star_inner_next_waits` collects at instruction boundaries and
migrates a suspended task, checking exact effects and result sequence. The saved
activation GC test also retains a delegate's closure solely through hidden locals.
`yield_star_protocol_getters_wait` separately parks in the iterator getter/call,
cached next getter, done getter and final value getter, checking exact callback
counts, result identity and saved locals across collection and worker migration.

QuickJS references and provenance: `research/quickjs/quickjs.c`,
`js_parse_assign_expr2` (yield-star iterator loop), `OP_yield_star`, and
`js_generator_next` (SUSPENDED_YIELD_STAR returns the result object itself),
revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The f1/f2 rows from
`research/quickjs/tests/test_builtin.js::test_generator` at that revision are
adapted in `tests/js_source.rs::generator_yield_star_delegation` with `let` in
place of `var` and rewritten assertions. Its copyright and MIT permission notice
are retained in the test. This is a semantic reference; no QuickJS C code or
saved C/Rust stack is used.

## JavaScript WeakMap objects (clj-quickjs-173)

`HeapObject::WeakMap` wraps an ordinary task-owned property store and the
collector's `EphemeronTable`. Only the prototype and ordinary properties are
strong heap edges. Entry keys are private object identities or non-registered
symbol identities; values are conditional edges, enumerated through
`trace_conditional`. The existing key-indexed worklist computes one least
fixpoint across both kinds, including symbols reached as ephemeron values.
Unmarked entries disappear before sweep in the same collection as their keys.
There is no JavaScript enumeration, size property, clear method or iterator.
`Rooted::weak_map_live_entry_count` is a host-only test diagnostic.

Symbols retain their immutable Arc bodies outside the object table. Arc
ownership, the property-name intern table, and clones held by garbage do not
establish weak-key liveness. `TraceEdge` therefore distinguishes object slots
from process-wide symbol identities. Every traced `Value::Symbol` reports its
identity; a symbol property key carries that identity in its private
`PropertyKey` and ordinary-object tracing reports keys as well as values.
Saved native descriptor keys are traced too. Root tables, task frames (including
parked runs and generator activations), closure captures, collection edges and
native continuations use the same tracing interface. A host must retain a
symbol in a `Rooted` or scope local to preserve its weak entries; an unrooted
`JsSymbol` clone keeps the immutable primitive usable but is not a GC root.
`Symbol.for` bodies carry an immutable registered flag, so they remain invalid
weak keys even if passed between runtimes as host primitives. Well-known
symbols are non-registered and rooted by the realm's symbol properties.
Symbol identities do not count as heap objects in collection statistics.
Tracking adds O(reached symbol identities) temporary marking space.

WeakMaps classify as `TaskOwned`, including empty and frozen instances. Direct
publication, persistent wrappers and closure captures, atoms and dependency
outcomes reject them before traversing entries. Spawn does not publish-check
arguments, matching ordinary objects; explicit handoff still cannot bypass
owner checks on properties, prototypes, integrity operations or borrowed
WeakMap methods. Ordinary property freezing leaves entry mutation available
to the owning task. No publication cloning or cross-task WeakMap snapshot is
provided.

The constructor/prototype pair and methods are appended to the realm/global
and built-in registries. Construction without `new` throws TypeError; nullish
arguments create an empty map. Non-nullish iterable arguments throw the same
explicit unsupported-construction TypeError as Map. `set` validates
CanBeHeldWeakly and returns its receiver; invalid keys give undefined/false
for get/has/delete. These are leaf operations: no coercion, getter, callback
or iterator runs while the object-table lock is held. The ordinary prototype
exposes the fixed `Symbol.toStringTag` data value `WeakMap`; general
Object.prototype.toString consumption of inherited symbol tags remains the
existing explicit gap.

Root inventory: WeakMapEntries maps the WeakMap heap kind to persistent-root
coverage in `tests/gc.rs`: `weak_map_value_kept_while_key_reachable` retains a
value only through a live object or symbol key;
`weak_map_drops_entries_of_unreachable_keys` reclaims ten values after five
deletions and dropping the remaining object/symbol keys.
`weak_map_cycles_are_reclaimed` checks cyclic object keys and chained tables,
then resumes the owning task to get a fresh absent key between collections.
`weak_map_symbol_keys_follow_traced_reachability` covers property-key,
property-value, Map-key and closure retention plus symbol self cycles;
`weak_map_ephemeron_values_activate_symbol_keys` covers mixed-kind fixpoints.
`weak_map_properties_and_prototype_are_traced` covers independent ordinary
strong edges and their reclamation.

WeakMap symbol tracing also covers constants in all descendant function code,
including code whose closure has not been created yet. `Code::trace` walks the
immutable code DAG iteratively and deduplicates shared children. The
`weak_map_symbol_keys_in_descendant_code_are_traced` regression injects a
symbol through `ExecutableSpec`, collects during a dependency wait, then
retrieves the same symbol through its descendant closure and checks the value.
`weak_map_symbol_keys_survive_running_safepoints` in `tests/gc_roots.rs`
keeps the key only in a running worker's local, verifies that worker's
Safepoint participant state for three rendezvous collections, and reclaims
the entry/value after the local is released. Instance descriptor reflection,
accessor invocation and property deletion (including non-configurable
failures and foreign-task rejection) are covered by
`weak_map_property_descriptors_and_delete` and the ownership source test.

## Exponential and significant decimal formatting (clj-quickjs-151)

`number::to_exponential` and `number::to_precision` reuse `Decimal::from_number`
and its exact `halfExpand` rounding, with no new arithmetic representation or
dependency. Rounding to p significant digits keeps p coefficient digits;
carry updates the decimal point before layout chooses plain or exponential
form. The coefficient is then padded or truncated to p digits. Exponential
format uses one leading digit and an explicit signed decimal exponent.
Precision format uses exponential notation when the rounded exponent is below
-6 or at least p. This also covers carries across either notation boundary.
The existing binary64 generator bound applies: at most 767 coefficient digits,
with requested output bounded to 101 significant digits for toExponential and
100 for toPrecision. Omitted toExponential digits reuse
`value::string::shortest_digits`, including its ties-to-even rule for equally
close shortest round-trip candidates. Specified counts use ties away from zero.

The realm installs both length-1 methods with ordinary method descriptors;
the Builtin variants are appended. `native::ConvertTarget::NumberToPrecisionFormat`
saves the receiver f64 and operation while existing traced ToPrimitive operands
and frames handle count coercion. This preserves waits, GC, yields and migration
without replaying callbacks. An object-produced undefined count is distinct
from an originally undefined argument. Both methods share toFixed's bounded
digit-count conversion helper (4096 UTF-16 units before parsing/flattening).
Unlike toFixed, these methods handle nonfinite values after count conversion
but before range validation. Tests cover coercion exceptions, receiver ordering,
one-instruction budgets, collection, waits and thread migration.

## Object patterns (clj-quickjs-132)

The shared Pattern AST includes ordered property-name/pattern pairs and optional
rest. PropertyName adds a computed expression and opening-bracket span for
conversion/read diagnostics. Object assignment cover grammar
converts literals recursively through the same pattern conversion as arrays.
Cover-only shorthand defaults and rest retain deferred syntax
errors until consumed as patterns; unconsumed forms still fail parsing. Rest
uses a temporary Spread value inside the cover object with an unused empty
property name. Binding-name collection traverses object values and rest, and
the existing parameter slots, TDZ cells, catch scopes and var hoisting apply.

Lowering stores the source and each converted exclusion key in traced frame
locals. GetElem performs resumable Get; existing default/store lowering handles
nested patterns and property targets. Source computed keys convert before the
target reference is evaluated; target computed keys retain the array-pattern
PutElem timing. Four appended instructions supply callback-free steps:
RequireCoercible rejects null/undefined; OwnKeys snapshots all own keys into a
private array, including symbols; IsEnumerable checks the current own descriptor;
DefineElem creates an own data property on the new ordinary rest object. The
enumerability query reads virtual function flags without materializing a lazy
prototype or changing publication eligibility. The
copy loop uses GetElem between descriptor checks and definitions, so getters
can suspend, mutate later properties, throw, or trigger GC without retaining a
Rust stack. OwnKeys is an allocation poll point; the other new instructions
allocate no heap objects. There are no new heap kinds or native continuations.
The instruction loop routes these leaves before entering the main dispatch
frame. Ordinary VM calls also keep native forwarding temporaries in a separate
slow path, preserving execution on 64 KiB worker stacks. Computed object literal execution is described below; object spread remains deferred.

### Intl.DateTimeFormat date fields (clj-quickjs-215)

DateTimeFormat uses an ordinary task-owned object with one boxed service payload:
immutable owned options, references to the existing `data::dates` pools, a
value-only `TimeZone`, and a traced cached bound function. Frozen objects remain
task-owned; neither the slots nor the static pattern/name data adds heap edges.
The service uses shared locale-list/resolution, option conversion and
supported-locale helpers. Locale and option getters and `format` argument
ToNumber run in saved native stages, so waits, GC and worker migration retain
no Rust stack frame. Realm identities and builtin variants append to the registry.

Resolved `ca`, `hc` and `nu` extension keys are reconciled with explicit options;
only Gregorian dates are implemented. `ar` defaults to Arabic-Indic digits for
this service. Numeric fields share NumberFormat's snapshot digit mapper;
name fields use the original month, weekday and era pools, including the
Russian genitive month forms and all long/short/narrow widths. No importer or
emitted table representation changes are needed. Component layouts are stored
as literal/field pattern items; the existing locale date/time/combined pattern
pools supply styles in -217; time components extend these pattern items in -216.

The service TimeClips the argument before using Date's existing local calendar
conversion, including offsets across TimeClip endpoints. UTC, ±HH, ±HHMM and
±HH:MM are accepted; offsets resolve to ±HH:MM (zero resolves to +00:00).
Host fixed zones report `TimeZone::identifier()` (UTC at zero; second precision
when needed); named host zones report the canonical snapshot identifier. A
requested named zone must be the host-selected zone or its recorded alias,
matched without ASCII case;
other names fail explicitly, including inventory-only names. Default-zone
selection shares Date's runtime cache. First/last transition offsets extend
indefinitely without extrapolating recurrence rules. Clock/default-zone reads
are guarded, unrecorded host leaf effects; a missing host clock throws RangeError.

Compatibility follows the recorded QuickJS-output policy: manual date fields
use weekday, month, day, year, era ordering with QuickJS separators even where
CLDR style patterns differ. Unsupported calendars and explicit time zone name
components fail with RangeError. Non-Gregorian components,
formatToParts and range methods remain deferred.

### Intl.DateTimeFormat time fields (clj-quickjs-216)

The existing immutable payload adds time widths, fraction precision, day-period
width and a resolved hour cycle; the pattern appends literal/field items for
hour, minute, second, fraction and day period. There are no additional GC edges
or publication categories. Option access and conversion use the existing saved
native stages, including ToBoolean for hour12 and resumable ToNumber for
fractionalSecondDigits. Formatting shares Date's zoned calendar conversion,
NumberFormat's digit mapper and the generated `ampm_tab`, `dayperiod_tab` and
`dayperiod_narrow_tab` pools. Existing short time patterns select locale default
cycles, as required by this pellet; this avoids the reference component
formatter's hardcoded h12 fallback for locales such as en-GB/fr. No generated
representation or importer changes are needed: all pools were already repacked
and verified through `scripts/quickjs-data/`.

Explicit hour12 overrides hourCycle and removes the hc extension from the
resolved tag; matching explicit hourCycle retains hc. Manual component output
keeps the reference's colon layout, two-digit neighboring fields, Japanese
trailing period, Spanish U+202F inside am/pm, en-US U+202F before the period,
and five shared day-period hour buckets. Fraction precision truncates
milliseconds and also formats without a second field (the reference's manual
formatter omits that case). Explicit timeZoneName components remain a gap.

### Intl.DateTimeFormat styles (clj-quickjs-217)

The immutable service payload stores optional date/time styles. After the
saved native stages read and convert every option, construction checks style
versus component conflicts before selecting date defaults. Style formatting
reads `date_pattern_tab`, `time_pattern_tab` and `combined_pattern_tab` through
the existing checked pool readers. These tables and per-locale indices were
already emitted and verified by `scripts/quickjs-data/repack-intl.pl`; there is
no generated-data or representation change. A bounded interpreter handles
field runs, quoted literals, Unicode punctuation and combined {0}/{1}
substitution. Style widths are independent of the manual-component layout.
No new heap edges, publication rules, host effects or dependencies are added.

The interpreter uses resolved hour cycles, names and digit maps, and TimeClips
before local calendar conversion. Full/long time styles implement the pinned
reference's zone pattern semantics: English UTC names, raw fixed identifiers,
Vienna's English standard/summer names and GMT offset fallback for other named
host zones. Named-zone offsets use the formatted instant's transition; their
display truncates seconds to minutes as the reference does. Explicit
timeZoneName components keep their documented RangeError gap, but combining
one with a style throws TypeError. Resolved style options omit all components,
and time styles expose the resolved hour cycle and boolean hour12.

### Async activations and task completion (clj-quickjs-248)

AsyncStart establishes a promise capability and an exception-catching native
boundary below the function's ordinary VM frame, before parameter initialization.
Await runs intrinsic PromiseResolve through resumable native frames, registers an
internal PerformPromiseThen reaction, and moves the activation with the same
`VmState::detach_activation` / `attach_activation` mechanism as generators. Saved
operands, handlers (including finally completions) and native frames retain their
relative indices and precise tracing. An AsyncState heap object is task-owned and
never published; its traced edges are its saved activation and promise capability.
A promise reaction is the only root path into a detached state. Its registering
task runs the resume job; cross-task settlement only enqueues that opaque job.
Rejection is raised in the reattached frame at await, before executing its next
instruction. Completion resolves or rejects the function's promise through the
existing resolver protocol, including resumable thenable adoption.

The owning task drains its queue and completes even with suspended awaits. Its
`suspended_async_activations()` diagnostic lists function names, including states
already reclaimed as unreachable; bookkeeping never traces activation identities.
No async job runs during a whole-task `__rt.need` wait. Await itself never changes
the task to Waiting. Cancellation unwinds attached frames normally and drops queued
jobs and detached state with the task's roots. An externally reachable pending
promise can retain its reaction until settlement, as for ordinary reactions.

QuickJS references and provenance: semantic references are `research/quickjs/quickjs.c`
`async_func_init`, `js_async_function_call`, `js_async_function_resume`,
`js_async_function_resolve_mark` and `CASE(OP_await)`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. No C implementation was ported.
`tests/gc.rs::test_async_gc_suspended_activation` adapts `research/quickjs/tests/test_std.js`
`test_async_gc` at that revision and retains its license. Runtime::collect at a
one-instruction budget boundary inside done substitutes for std.gc. A temporary
reference to the awaited promise makes its activation reachable at that point;
the host supplies obj to permit a weak-handle lifetime assertion. Dropping that
reference and completing the task reclaims the activation, closures and promises.

Root inventory addition: the task-owned AsyncState's saved activation and promise
capability are strong edges, reached through its internal promise reaction or an
attached native boundary. `test_async_gc_suspended_activation` in `tests/gc.rs`
checks retention and reclamation, including a host-observed weak object handle;
`unreachable_async_cycle_can_be_collected_before_task_completion` verifies that
diagnostic bookkeeping adds no hidden roots.


## Computed literal properties (clj-quickjs-233)

Object literals reuse `PropertyName::Computed` from object patterns, including
its expression-height limit and opening-bracket diagnostic span. The parser
accepts computed data, concise method, generator method and accessor names;
computed shorthand remains a syntax error. Key expressions retain the outer
function's suspension context, while method parameters/bodies use their own.

Lowering evaluates each key and emits the existing resumable `ToPropertyKey`
before evaluating its value. Two traced scratch locals preserve the converted
key and value while `Method` sees the literal on the operand stack as its home
object. The locals are cleared after defining the property. `DefineElem`
creates own data properties without inherited setters; appended
`DefineGetterElem` (0x85) and `DefineSetterElem` (0x86) merge accessor halves
using ordinary descriptors. These instructions accept converted symbol keys
without interning them as strings, and computed `__proto__` is an own property.

`SetComputedName` (0x87, string-constant prefix operand) sets a fresh closure's
per-instance name from the converted key, including bracketed symbol
descriptions and accessor prefixes. Its immutable `JsString` metadata lives
outside the traced heap and adds no GC edges or mutable property record;
shared executable code remains unchanged. Initial function metadata and later
property-record materialization use this per-instance name. Only anonymous
function expressions/arrows and method definitions emit the naming instruction.
The VM routes these leaf instructions outside its large general dispatch
frame, retaining execution on 64 KiB worker stacks. Property operations and
native conversion steps also use separate dispatch functions. A conversion's
resume action returns its instruction to the loop after dropping the native
Rust frame, rather than recursively entering instruction dispatch. Key callbacks remain
ordinary detachable VM calls: budgets, dependency waits, collection and
migration retain the in-progress literal in existing operand/local roots.

QuickJS references and provenance: consulted `research/quickjs/quickjs.c`
`js_parse_property_name`, `js_parse_object_literal`, `OP_to_propkey`, and
`OP_define_method_computed` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` for semantic comparison only.
No upstream source or test text was copied; fixtures are synthetic.

## Unhandled Promise rejections (clj-quickjs-249)

Embedding policy: a successful task still returns `RunOutcome::Complete(value)`.
After that return, `Task::unhandled_rejections()` supplies an `UnhandledRejections`
snapshot containing `entries` (each an `UnhandledRejection { promise, reason }`)
and `dropped`. The runner enables the query only at quiescence, after the body,
jobs and any timers drain; it never checks after individual jobs, budget yields
or dependency waits. No callback or JavaScript runs as part of reporting.
Unsettled awaits retain their separate string-only activation diagnostic.
The CLI prints the remaining reasons to stderr and exits 1, including overflow.

Settlement's reject transition and PerformPromiseThen's handle transition run
under the existing Promise metadata lock. The rejecting task's inbox holds a
FIFO queue bounded to 30 entries. Each entry traces exactly the Promise identity
and rejection reason as TaskFrame roots; those roots persist after successful
completion while the task handle exists. This retains the Promise prototype
and reason graph, but a rejected Promise has already drained its registrations,
so its original reactions are not retained by this diagnostic. A query creates
independent persistent handles after releasing the inbox lock, under short
registered heap access, so snapshots remain safe across concurrent handling
and collection. Dropping the task releases its records; failure/cancellation
close admission and discard records before terminal cleanup.

When full, new entries retain neither Promise nor reason: only the saturating
`dropped` counter increments. Each rejected Promise carries a weak inbox
receipt and one stored/overflow bit; this adds no GC edge or task root. Any
later reaction attachment uses that receipt to remove the stored entry or
decrement overflow, including across tasks and after completion. Newly freed
queue slots admit subsequent rejections; overflow records are never promoted.
Snapshots already returned remain snapshots. The rejecting task owns the
record even for a Promise created elsewhere. Marking every `then` source
handled matches HostPromiseRejectionTracker: a missing rejection handler
causes the reaction job to reject its derived Promise, which is reported
instead. Internal await reactions follow the same handle transition.

Root inventory addition: `UnhandledRejection` is a TaskFrame holder covered by
`tests/promises.rs::unhandled_rejection_roots_are_retained_and_reclaimed`.
`rejection_queue_is_bounded_and_later_handling_updates_overflow` also proves
omitted reasons are collectible, and the acceptance test collects between
individual instructions. Waiting, cancellation, failure, foreign settlement,
and foreign handling have dedicated tests.
`src/task/job_tests.rs::unhandled_rejection_snapshot_survives_concurrent_handling_and_gc`
pauses a query between copying entries and rooting them, removes its record
on another task, and proves collection waits until the snapshot owns roots.

QuickJS references and provenance: consulted `research/quickjs/quickjs.c`
`fulfill_or_reject_promise`, `perform_promise_then`, and
`JS_SetHostPromiseRejectionTracker`; `research/quickjs/quickjs-libc.c`
`js_std_promise_rejection_tracker`, `js_std_promise_rejection_check`, and
`js_std_loop`; revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`.
The implementation is written fresh. The opening body of
`unhandled_rejections_reported_at_quiescence` adapts
`research/quickjs/tests/test_std.js::test_async_promise_rejection` at that
revision, using an optional simulated-host timer; `tests/promises.rs` retains
the upstream copyright/license notice. Timer and nested-timer cases also prove
that handlers can remove earlier rejections before the quiescence report,
and that unhandled rejections from timer-created jobs reach the final report.

### Runtime compilation for indirect eval (clj-quickjs-106)

The global eval builtin prepares source with a task-owned native continuation.
Rope flattening and UTF-16 decoding yield between bounded chunks. Input is
capped at 16,384 UTF-16 units because the existing parser/lowerer/validator is
synchronous; no heap read closure, heap mutation lock or waiter lock spans
compilation. The separate `frontend::compile_eval` entry point parses an
initially sloppy script on a temporary 2 MiB compiler thread, including AST
cleanup on failure. This is the frontend's tested stack bound; VM workers
can use 64 KiB stacks. The helper receives source text only and runs no
JavaScript or heap operations. Global lookup, calls and eval native steps
avoid the large general dispatch frames before using deeper helpers.
Directives select strictness. Compilation uses script
completion lowering while keeping strict eval declarations local. The result
is the usual shared immutable `Executable`; a trampoline action releases the
native borrow and enters a fresh ordinary VM frame. No Rust interpreter loop
or stack is retained across a yield or wait. The frame records eval origin so
sloppy declarations use configurable global properties. Existing task roots,
frame tracing and native cancellation/unwinding cover the whole path.

The task also owns a traced name-to-cell global declarative environment
(clj-quickjs-278), retained across job activation changes. Script lexical
declarations register their TDZ cells before any initializer or hoisted
function runs. Mutable script `let` bindings always share these cells with
ordinary closures; eval resolves global names through the environment before
consulting object properties. Immutable `const` bindings keep the existing
capture-by-value optimization when initialized before closure creation;
their environment cell is initialized alongside the frame value. Function
locals and eval-local lexicals keep their existing capture rules. Eval
closures resolve global lexicals dynamically and can yield, wait and migrate.
All global var/function declarations have a separate read-only validation
pass before any global property is created or reset. Every lexical conflict
check precedes every property-admissibility check. Lexical conflicts throw
SyntaxError; refused eval object declarations throw TypeError. Global lexical
bindings are never object properties and cannot be deleted through eval.

QuickJS references and provenance: consulted `research/quickjs/quickjs.c`
(`js_global_eval`, `JS_EvalObject`, `__JS_EvalInternal`),
`research/quickjs/tests/test_builtin.js` (`test_eval`) and
`research/quickjs/tests/test_language.js` (`test_global_var_opt`) at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. These were semantic references;
implementation and synthetic fixtures were written independently, with no
upstream code or test text copied.

### Exact iterable summation (clj-quickjs-158)

`Math.sumPrecise` drives the shared resumable GetIterator protocol, caching
`next` and reading `done` before `value`. Iterator calls/getters and IteratorClose
are ordinary VM actions. A native catch boundary closes on non-Number elements
or the 2^53-element limit; the original exception wins over return errors.
IteratorStepValue failures propagate directly, as in QuickJS.

The task-owned native state traces the acquisition, iterator, cached next,
current result and pending exception. Its numeric accumulator uses two 34-limb
unsigned integers in units of 2^-1074, one for each sign. A finite binary64
occupies at most 2098 bits; fewer than 2^53 elements fit in 2151 bits. Subtraction
and one ties-to-even rounding happen only after exhaustion, allowing overflowing
intermediate sums to cancel. Each element has its own budgeted addition step;
addition, subtraction and rounding each scan at most 34 limbs. Empty/all-negative-
zero inputs yield -0; other exact zero sums yield +0. No heap kind or dependency
was added. GC, waiting and worker migration preserve the accumulator in the
native continuation rather than on a Rust stack.

## Simulated collection points and timers (clj-quickjs-175)

`Task::install_sim_host_bindings()` installs task-owned `std` and `os`
namespace properties for synthetic fixtures and embedding tests. They are
absent on ordinary tasks. The namespaces contain `std.gc()`,
`os.setTimeout(callback, delay)` and `os.clearTimeout(id)`. Installation is
idempotent and preserves pending timers and virtual time; call it before
running the script. These are optional host facilities, rather than engine
globals or imports of QuickJS's libc modules.

The GC built-in completes its call with undefined and sets an explicit VM
collection request. At that instruction boundary the run driver retains its
exclusive execution claim but releases the task-body lock and run mutator
token. The task root source traces the unlocked saved body even though its
claim remains executing; an ordinary run stopped at a safepoint still reports
its frames through the existing poll. The driver calls the heap's existing
`request_collection` rendezvous, then reacquires heap access and the body.
No task or heap lock spans the wait, no other worker may claim the task,
and no Rust callback stack is retained across a VM suspension. The remaining
instruction budget is preserved. An enclosing heap scope, native operation
or extra run guard is rejected explicitly rather than deadlocking.

Overlapping task requests join a runtime-local collection batch before
releasing access, while that batch has not started its rendezvous. Later
requests wait for earlier notification delivery, then perform a fresh pause. One driver serves the rendezvous and publishes completion
only after synchronous post-pause collection observers return. Other batch
members wait outside heap access, so cleared-weak-slot observers have queued
their jobs before any requesting task's next statement. The batch metadata
has no heap edges, and its short locks never span collection or notification
processing. Successors take and drop predecessor links after waiting, so
completed batches do not retain a historical chain. Observer panic releases
batch waiters with failure. Reentry into
a collection point from the collecting thread's own observer is refused.
The public `Runtime::request_collection` contract remains unchanged:
independent explicit calls each collect. Every collection registers a delivery
completion record before releasing the pause, including explicit embedder
collections. Before publishing batch success, a collection point snapshots
and awaits still-active deliveries outside heap access. Thus it also waits
for an older explicit collection whose weak slots were already cleared.
Observers remain concurrent and may request reentrant explicit collections;
they must not wait on runtime task progress. Collection points from an active
observer are refused to avoid waiting on their own report. FinalizationRegistry's
consumer delivers owner-task jobs (clj-quickjs-177 below).

Collection-driver temporaries stay in a separate helper, and optional host
state is boxed, preserving ordinary execution on 64 KiB worker stacks.

`src/host/sim/facilities.rs` holds each task's virtual clock and pending
`(due time, sequence, callback)` records in traced VM state. Timer IDs increase
per task. Delay conversion uses the ordinary resumable ToNumber protocol,
including getters and `valueOf` calls that may wait, yield, collect or migrate.
Finite delays are truncated to integer milliseconds; negative delays become
zero. Missing delay is zero. Nonfinite delays and values/times beyond the
safe-integer range throw RangeError. Callback validation throws TypeError.
Clear removes a matching numeric timer ID and otherwise does nothing.

After the body or a callback returns, the job queue drains first. Only when
it is empty does the next timer advance virtual time and start through the
ordinary job trampoline; its return is followed by another job drain. Equal
deadlines use registration order, and timers added by a callback are relative
to the current virtual time. A pending timer keeps job admission open across
an empty checkpoint. Callback closures and captures survive GC, dependency
waits and worker migration. The final task outcome preserves the body's
value and waits for all jobs and timers. Failure/cancellation drops pending
callbacks without running them. There are no wall-clock waits or implicit GC
triggers; intervals, string callbacks, extra callback arguments and integration
with the simulated Date clock are deferred.

Tests in `src/task/sim_tests.rs`, `tests/sim_facilities.rs` and
`heap::gc::tests` cover statement/notification ordering, jobs before timers,
virtual ordering, callback lifetime and waits, merged requests and two-worker
completion with bounded deadlines. Semantic references are `js_std_gc`,
`js_os_setTimeout` and the jobs-first libc event loop in
`research/quickjs/quickjs-libc.c`, plus the weak-object cases in
`research/quickjs/tests/test_builtin.js`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. No upstream source/test text was
adapted.
## WeakRef targets and task kept objects (clj-quickjs-176)

The append-only `WeakRef` heap kind contains an ordinary task-owned property
record and one collector `WeakSlot`. Slots now identify either objects or
symbols via `TraceEdge`, using the same private identities and mark set as
WeakMap. A symbol slot retains its immutable body as untraced metadata so deref
can reconstruct its value; Arc ownership does not establish symbol liveness.
Clearing a dead symbol slot moves this metadata to a retirement list dropped
after the pause and all heap locks end, so a live cleared WeakRef cannot retain
its target's description storage. `weak_ref_releases_cleared_symbol_metadata`
checks both preservation by a strong symbol root and release on clearing.
Only the collector clears slots. The notifying-slot interface remains available
for FinalizationRegistry; no callback or payload destruction runs in the pause.
WeakRef ordinary properties and its prototype are strong edges independently
of the weak target.

Each task's `VmState.kept_objects` is a deduplicated identity-to-value set.
Construction and every successful deref insert a strong value before the next
safepoint. All dynamic builtin call paths, including forwarded calls and native
callbacks, use the state-aware leaf dispatcher. The existing task-frame trace
reports the set for both suspended tasks and running safepoints. A generator
activation does not own or move this set. A budget yield, migration, or waiting
on `__rt.need` stays inside the same job, retaining these targets. The owner
clears the set after its script body and after every completed queued job,
before another activation starts. Terminal failure/cancellation drops VM state
and releases the set through the existing task teardown.

WeakRef instances classify as TaskOwned, even with a shareable target or after
freezing; transitive publication and atom/dependency publication reject them.
Receiver and ordinary property/prototype/integrity operations reject foreign
tasks, matching WeakMap. Cross-task publication is explicitly deferred: there
is no foreign task allowed to add to another task's kept set.

Tests cover constructor keep-alives over waits, successful deref keep-alives,
clearing after script/job completion, strong-target variants, symbol targets,
forwarded/native calls, properties/prototypes and publication. Opt-in `std.gc()`
collection points trace kept objects without ending the job. Timer callbacks
start after the preceding job's kept set clears; jobs queued by a timer drain
before the next timer, each with a fresh kept set. Tests cover object and symbol
targets at these boundaries and collection during a timer's dependency wait,
followed by migration. WeakRef itself adds no substitute host globals.


## Symbol prototype and wrapper storage (clj-quickjs-129)

`PrimitiveData::Symbol` appends immutable `[[SymbolData]]` to the existing
Number/String wrapper representation in `OrdinaryObject` (`HeapObject::Plain`).
`Object(symbol)` and supported ToObject consumers allocate an ordinary object
owned by the current task with `Symbol.prototype`. No new heap kind or opcode
is needed. `value::share::classify` continues to reject ordinary wrappers as
task-owned even when frozen; `builtin::wrapper_data` enforces ownership before
reading internal data. Prototype changes do not change the internal brand.

The precise ordinary-object trace reports the stored symbol's `TraceEdge::Symbol`
identity in addition to ordinary property/prototype edges. This keeps ephemeron
values alive while a reachable wrapper retains their symbol key, and permits
collection once the wrapper dies, including wrapper/value cycles. The immutable
symbol body still uses the existing Arc storage boundary; Arc counts are not
collector roots.

The realm appends Symbol.prototype and four builtin functions: toString,
valueOf, the description getter and [Symbol.toPrimitive]. The prototype is an
ordinary immutable intrinsic without [[SymbolData]], so borrowing its branded
methods on itself throws TypeError. Its constructor points back to Symbol.
Description is a non-enumerable configurable accessor; [Symbol.toPrimitive]
and [Symbol.toStringTag] are non-writable, non-enumerable, configurable data
properties. All functions and the prototype enter the existing realm root
inventory. Primitive property access starts at this prototype and retains the
primitive receiver, allocating no temporary wrapper.

`native::Convert` saves the Symbol-wrapper exotic lookup/getter/call stage.
Only branded Symbol wrappers enter this protocol path; general shared-object
Symbol.toPrimitive dispatch stays deferred. Each getter/method runs via an
ordinary VM call action with the original receiver and hint, and can yield,
wait, survive collection and migrate. Nullish methods fall back to the ordinary
method pair; object results throw. String's direct primitive-symbol exception
continues to apply only before object conversion. Descriptive strings reuse
UTF-16 JsString concatenation rather than Rust display formatting.
If a wrapper's prototype no longer supplies @@toStringTag, Object's fallback
tag is Object, independent of [[SymbolData]], matching `js_object_toString`.

QuickJS semantic references (no C code copied): research/quickjs/quickjs.c
`js_thisSymbolValue`, `js_symbol_toString`, `js_symbol_valueOf`,
`js_symbol_get_description`, `js_symbol_proto_funcs`, `JS_ToObject`, revision
7d1c13c5d8f8c6b32893786ed35cdcebd88194d2. Adapted test rows and their retained
license notice are recorded in docs/attribution.md.

### Caller environments for direct eval (clj-quickjs-107)

The lowerer's analysis pass resolves every visible binding at each syntactic
`eval(...)` call, including through enclosing functions. Those facts force
binding cells and the ordinary resolver threads captures through intervening
closures. Emission records names, assignment semantics, receiver source, and
validated local/capture locations at the Call/CallSpread byte offset. Loop
descriptors read the current iteration's cells at the call. Arguments bindings
are materialized when eval might reference them. Functions without eval retain
their existing slot/value capture optimizations.

The ordinary call slow path checks descriptors only for a direct intrinsic
eval callee; forwarding, aliases and member calls keep global eval semantics.
Its task-owned native continuation roots the captured cells and receiver while
preparing source. The compiler helper receives only source and immutable scope
metadata, and compiles against imported captures using inherited strictness.
The trampoline enters the compiled executable as a normal closure/frame.
Scope descriptors contain no heap edges and are validated and retained by
ExecutableSpec round trips. Native continuation tracing covers values until
the new function object owns them. Cell ownership and publication checks are
unchanged: a closure holding an eval-visible cell is not publishable.

Receiver-only captures use `GetRawThis` (0x91) instead of evaluating
`GetThis`. The descriptor retains the originating ordinary function's mode;
an imported sloppy receiver is checked with `CheckSloppyThis` (0x92) only
when eval source actually reads it. Ordinary receiver captures outside eval
keep their existing path. Calls named `eval` in parameter expressions use the
same runtime intrinsic check as body calls (clj-quickjs-109).

Strict eval declarations use a private variable environment; lexical
declarations always remain private. Sloppy var/function declaration injection
is supported by clj-quickjs-108 below; parameter-expression eval by -109 below.
`new.target` (-99) and lexical `super` (-112) are transported with the receiver.

QuickJS references and provenance: consulted `research/quickjs/quickjs.c`
`OP_eval`, `OP_apply_eval`, and `add_eval_variables` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, as semantic references only.
`tests/js_source.rs::strict_direct_eval` adapts rows from
`research/quickjs/tests/test_builtin.js::test_eval` and
`research/quickjs/tests/test_closure.js::test_eval_closure/test_eval_const` at
that revision and retains their MIT copyright/license notice. The implementation
and the publication, serialization, and migration tests are written fresh.

### Sloppy eval variable environments (clj-quickjs-108)

Analysis marks sloppy functions containing direct eval. Only those functions
allocate a private null-prototype object held by a task-owned binding cell at
body entry. Its properties hold cells for dynamically introduced names. The
existing precise collector traces this graph, and the enclosing cell prevents
publication even when the variable object is empty. No heap kind or opcode is
added. Functions without direct eval retain their existing lookup bytecode.

Resolution inserts that environment between body bindings and outer scopes
(before parameters for bodies with parameter expressions). Closures capture the
environment itself, so a closure created before eval sees later additions and
deletions. Default expressions run before the body environment exists. References
selected for assignments retain the environment and name across the
right-hand side: a sloppy store recreates a deleted binding there, while a
strict store throws if that binding is still absent. Destructuring defaults and
numeric updates use the same reference selection.

Scope descriptors retain the ordered environments and the number nearer than
each static binding, together with the variable target, reusable static cells, and
all intervening lexical names. Imported descriptors preserve that ordering
through nested eval and nested functions.

Runtime compilation checks every declared name against intervening lexicals
before running declaration bytecode or source effects. Sloppy declaration
instantiation reuses caller var/parameter cells or creates configurable entries
in its variable object; script targets use the existing global preflight and
configurable global declarations. Strict eval and lexical declarations keep
private bindings. All execution remains in resumable task-owned frames.

QuickJS references and provenance: semantic references are
`research/quickjs/quickjs.c::add_eval_variables/JS_CheckDefineGlobalVar`,
`OP_with_make_ref/OP_put_ref_value`, and
`research/quickjs/tests/test_closure.js::test_with` at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The parameter-body eval and
immutable function-name rows in
`tests/js_source.rs::sloppy_eval_declares_vars_in_the_caller` adapt
`research/quickjs/tests/test_language.js::test_argument_scope/test_function_expr_name`
at that revision and retain the nearby MIT copyright/license notice. Other
fixtures are synthetic, not light-cljs output; the implementation is original.

### Eval parameter environments (clj-quickjs-109)

Analysis distinguishes direct eval in a function's parameter expressions from
eval in its body and nested functions. An affected sloppy activation creates
a parameter variable object and cell before initializing defaults, using the
same traced, task-owned representation as body eval environments. The slot
remains live after parameter scratch slots are reused. A separate body
environment is allocated only if that body contains direct eval. Functions
without eval retain their existing allocation and lookup bytecode.

Resolution consults body declarations, the body eval environment, parameter
bindings, the parameter eval environment, then the function name and outer
scopes. Closures compiled in defaults capture only environments already in
scope. Eval scope descriptors carry both environments, nearest first, using
the existing validated sources and static-binding depth counts. No new VM
opcode, heap kind, or retained native call frame is needed. Parameter names
and implicit non-arrow `arguments` are declaration conflicts for sloppy eval;
arrows introduce no implicit arguments binding. Existing receiver, function-name,
and arguments transport also applies before body entry.

QuickJS references and provenance: consulted
`research/quickjs/quickjs.c::js_parse_function_decl2/add_eval_variables/add_arguments_arg`
at revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` as semantic references.
`tests/js_source.rs::eval_in_parameter_expressions` adapts the eval rows of
`research/quickjs/tests/test_language.js::test_argument_scope` at that revision,
under the MIT copyright/license notice reproduced nearby in that test file.
Additional fixtures and the implementation are original.

## Base class construction (clj-quickjs-136)

`ExprKind::Class` stores the optional name, strict base constructor and ordered
static/instance elements. Declarations are represented as lexical `let`
declarations initialized by this expression, preserving ordinary scope conflict
checks and TDZ. Lowering opens an inner immutable binding cell keyed by the
class keyword's syntax occurrence, independently of the outer declaration name.
Constructor and method closures capture that cell; computed keys execute before
its initialization. Temporary locals keep constructor, prototype, key and method
roots alive across resumable key conversion. The locals are cleared on completion.

`ConstructorKind::BaseClass` extends ordinary/non-constructor metadata. The
ordinary frame entry rejects it with TypeError; the checked construction path
uses constructor frame entry. This covers direct calls and native forwarding
without putting JavaScript on the Rust stack. Constructor return processing and
receiver allocation use the existing construct continuation. Derived kinds and
super operations extend this path below (-137).

Class function creation eagerly materializes the existing function property
record and prototype, makes `prototype` non-writable, and installs the prototype
as the constructor's home object. Instance methods use that prototype as their
home; statics use the constructor. Appended bytecodes `DefineClassMethod`,
`DefineClassGetter` and `DefineClassSetter` consume `object key function` and
return `object`, defining non-enumerable configurable descriptors (writable for
methods), merging getter/setter pairs through the existing descriptor path.
These instructions can allocate function property records for static members and
are therefore allocation safepoints, with no retained borrows or JS callbacks.

There is no new heap kind. Existing function tracing visits captures, property
records and home objects. Constructors, prototypes and detached class methods
therefore fail publication on their task-owned reachable state, just like
object methods and functions with own properties. Tests exercise collection and
worker migration during constructors, methods, accessors and computed-key
conversion, and execution on 64 KiB worker stacks. Fields
continue to fail explicitly.

Computed keys execute inline in their enclosing activation, so `yield` and
`await` retain the enclosing generator/async continuation. In sloppy scripts,
lowering temporarily applies class strictness; appended `PutGlobalStrict`,
`PutFieldStrict`, `PutElemStrict` and `DeleteStrict` bytecodes enforce strict
runtime writes/deletion there without changing the surrounding function's mode.
Direct-eval descriptors also retain the call site's strictness, independently
of the originating receiver's mode, so eval in these keys inherits class
strictness while keeping the enclosing activation's `this` behavior.

For an anonymous class used as a computed object-property value, lowering passes
the converted key's local into class construction. It sets the constructor name
before defining class elements, preserving explicit class names and later static
`name` methods/accessors. Async class tests await both property names and method
bodies, forcing collection and migration while the class's saved construction
state, receiver and inner-name captures remain live.

## Derived class construction (clj-quickjs-137)

Class AST nodes retain an optional heritage expression. The strict inner class
name is in its temporal dead zone during heritage and computed-key evaluation.
Lowering evaluates heritage once, checks constructor capability, reads the
parent's prototype with ordinary resumable property bytecode, and installs both
inheritance links. A class function's existing property record stores its own
prototype link, so static lookup and Object.getPrototypeOf observe the parent;
the instance prototype separately inherits from the parent's prototype. Null
heritage keeps Function.prototype for the constructor and uses a null instance
prototype link. No new heap kind is introduced.

Appended ConstructorKind variants distinguish explicit and default derived
constructors. Each construct frame uses the traced new_target value from
clj-quickjs-99, shared with source `new.target`, lexical arrows and direct eval;
ordinary base allocation uses that constructor's prototype. Derived frames
allocate an existing task-owned BindingCell instead of an instance receiver.
GetRawThis transports the cell through lexical captures and eval descriptors;
ResolveThis checks and reads it only at a source `this` occurrence. Arrows can
therefore be created before super() and see the initialized receiver afterward.
The executing constructor is captured through arrows that call super(); the
target uses the existing immutable lexical capture path. Source `new.target`
is available before super() initializes the receiver and remains the derived
constructor through all parent frames.

Appended instructions CheckHeritage and InitHeritage validate/install the class
links; GetSuperConstructor loads the executing constructor's parent, while the
existing GetNewTarget instruction loads the shared construction target.
ConstructSuper/ConstructSuperSpread enter the parent through the normal checked
construct path with an alternate new target. BindThis runs after the parent
returns, rejects an already initialized receiver, and yields the returned object.
The parent may wait, yield, migrate or throw while the derived frame and its
uninitialized cell stay traced. Default constructors use ForwardArguments to
pack the original actual arguments directly, without invoking an iterator.
Constructor return checks run after finally blocks: objects override the
receiver, undefined requires an initialized receiver, and other primitives throw
TypeError. Native built-in subclass allocation is rejected explicitly except
for Object, whose derived receiver uses ordinary allocation. Runtime eval keeps
the existing scope/strictness model and reads the live receiver cell, but does
not inherit super-call syntax context.

Both live and detached frame tracing visit new_target, and existing cell,
function-capture and property-record tracing cover every other edge. Tests force
collection and migration while parked inside super() and execute explicit,
default and Object-derived construction on 64 KiB worker stacks.
Ordinary Construct dispatches through the small call leaf too, keeping the
general opcode dispatcher's temporaries off the receiver-allocation stack.

Function-property reads and writes walk the constructor's actual prototype
chain before rejecting still-missing Function.prototype names such as toString,
caller and arguments. Bound-function metadata retains the target's internal
prototype, and materializing an own-property record preserves that link. This
retains inherited statics through reflection, calls, writes and conversion.
For-in follows constructor prototype links, including bound constructors,
while retaining non-enumerable shadowing and the realm-intrinsic shortcut.
The existing metadata/record prototype edges are traced by GC. Tests suspend in
inherited static getters and setters, collect and migrate, and verify that their
receiver and resulting state belong to the derived constructor.

### Intl.ListFormat (clj-quickjs-220)

ListFormat uses an ordinary task-owned object with one immutable boxed record
(locale plus type/style), without heap edges or a new heap kind. Freezing its
public properties leaves it task-owned under the existing publication category.
The append-only built-in registry and realm install its prototype, constructor
and four methods. Constructor locale/option stages use the shared resumable
canonicalizer, ordered GetOption helpers and guarded host-default leaf effect.
The supported-locale method reads only localeMatcher and never the host default.

`vm::native::list_format` stores the GetIterator continuation, cached iterator
and next, current result, collected strings, output, and pending exception in
traced task state. Getters and calls use ordinary VM frames. A non-string value
is validated at a separate native catch boundary so IteratorClose can wait or
collect, and rethrow the original diagnostic even if closing fails. Failures in
IteratorStep/IteratorValue propagate directly, following the existing iterable
consumer convention. Input collection and result construction each advance by
one element per budgeted step. String concatenation retains rope subtrees;
parts retain original string bodies, including empty strings, NULs and lone
surrogates. Finite option values reject oversized ropes by length before the
leaf spelling comparison, avoiding an unbudgeted flatten.

The snapshot's locale records have no list fields. `repack-lists.pl` extends the
existing verified-data pipeline by extracting only JSListFormatData and the
pinned `js_listformat_get_separators` selector/helpers into an offline exporter.
It emits all four separator positions for each of 15 test locales × 3 types ×
3 styles as safe Rust in `data/lists.rs`, with input/generator hashes in
`data/list_provenance.txt`. `verify.sh` repeats generation, compares every byte
with an independent Rust reader, detects deliberate corruption, and rejects
source drift. Cargo never invokes C or the importer. The selector's start
separator is empty and unused, just as in QuickJS; middle/end/pair delimiters
produce the final order and parts.

Coverage reproduces the pinned selector's strings, including English fallback
for other locales, Spanish `y` for disjunction and no contextual `e` alternation.
It does not infer a CLDR release or general translated-list coverage. Further
patterns require a separately pinned extension of this same data pipeline.

## Upstream built-in fixture execution (clj-quickjs-163)

`tests/quickjs_builtins.rs::quickjs_builtin_cases_run_unmodified` executes the
six selected QuickJS Array, String, Math, Number, JSON and rope cases with the
unchanged upstream assertion helpers. Each case gets one compilation and a
fresh runtime/task; compile or runtime failures are accumulated so one missing
feature cannot hide the other cases. The fixture contains the exact selected
388 upstream lines, a license header and six calls (see `docs/attribution.md`).
Each task runs in slices of 50,000 instructions, with collection after every
yield and a limit of 2,000 slices. The rope case must yield at least once.
All six cases complete, including String's direct eval (clj-quickjs-107)
and Math.sumPrecise (clj-quickjs-158). The rope case completes across 204
yields and collections. No runtime behavior or representation changes are
introduced here.

## Upstream Date and RegExp fixture driver (clj-quickjs-195)

The source fixture keeps the pinned QuickJS helper prologue and complete
`test_date` / `test_regexp` bodies. The integration driver compiles each
selected case once for execution with the original helper prologue.
One compiled Date executable runs in
four fresh runtimes with `SimTimeHost`: UTC, fixed +01:00, fixed -05:00 and
Europe/Paris. Each runtime gets its own zone because local-zone selection
is cached per runtime. A fixed virtual clock is supplied; these upstream
rows do not read it. Every run has bounded instruction slices and rejects
unexpected waiting or failures.

The RegExp executable runs unchanged in a fresh UTC runtime, including
upstream line 744's direct eval of a NUL-containing pattern. Direct eval
uses the support from clj-quickjs-107; this strict expression needs no
sloppy declaration hoisting. The fixture bytes and expected values remain
unchanged. No engine behavior, host representation or GC edges change.

## FinalizationRegistry and owner-task cleanup (clj-quickjs-177)

The append-only `FinalizationRegistry` heap kind embeds an ordinary task-owned
property store, a strong callback and registration cells. Each cell has a
notifying weak target slot, a strong held value and an optional weak unregister
token slot. Cells live in a BTreeMap indexed by private monotonically increasing
IDs, so notification removal takes O(log n) without linear scans or shifts;
cleared/repeated/out-of-order notifications cannot consume another cell.
Objects and non-registered symbols are eligible targets/tokens; registered
symbols and SameValue(target, heldValue) are rejected with TypeError. Calling
unregister removes every matching cell; collecting the token itself
merely clears that slot and does not unregister the target.

Retention is an explicit implementation choice: the owner task's inbox roots
all registries with active cells, even after the last JavaScript reference
vanishes. A registry therefore outlives its last JavaScript reference while
it has cells. This preserves the two block-scoped upstream finalization cases.
Removing/cleaning the last cell releases that root; task completion, failure
and cancellation release all active-registry roots. A registry may still be
reachable through ordinary JavaScript/host roots after its owner finishes.

The heap's post-pause dispatcher consumes registry notifications for the heap's
entire lifetime, even after the owner task handle disappears. Each registry
stores a weak owner-inbox capability, with no strong task/runtime reference.
After external report observers finish, the dispatcher removes the cleared
cell and transfers callback/held value into an ordinary job on that inbox,
dropping the job when the inbox is absent or closed. A guard also consumes
cells if an external observer unwinds. It holds heap access throughout transfer;
notification roots
keep the registry/callback/held values alive until then. Cell mutation and
retention updates share the object-table lock, followed by the leaf inbox
lock. No path enters heap access while holding that inbox lock. JavaScript
never runs during delivery or under a collector/waiter/inbox lock. Finished or
cancelled owners drop cleanup through the closed inbox. No delivery wakes a
dependency wait. Cleanup can yield, wait and migrate like any other job; jobs
drain after the body and before virtual timers. No `cleanupSome` is exposed.

Instances classify as TaskOwned, including empty or frozen registries.
Publication checks reject them through direct values, persistent collections,
closures, dependency outcomes and atom publication paths, consistently with
WeakMap/WeakRef. Host raw handles confer no authority to use another task's
registry. Cross-task publication has no snapshot/promotion rule and is deferred.
Freezing ordinary properties does not disable register/unregister.

Root inventory additions: `FinalizationRegistryEdges` covers callback captures,
held values, properties and prototype; `ActiveFinalizationRegistries` covers
the owner-task retention set under TaskFrame. Tests
`finalization_registry_properties_and_callback_are_traced` and
`finalization_held_value_is_strong` retain/reclaim these edges and pending job
arguments. Targets and tokens are never marked by registry tracing.
`finalization_callbacks_run_as_jobs_after_collection` adapts both upstream
blocks, with an order log proving body/collection, cleanup jobs, then timers.
Other tests cover unregister, weak symbols/tokens, finished owners and cleanup
waiting followed by migration. Delayed-report tests overlap unregister and
replacement registration with out-of-order collections, and a 4096-cell batch
checks once-per-cell cleanup mixed with unregister. Dropped-owner tests verify
held-value reclamation while the host continues rooting the registry.
See docs/attribution.md for the upstream notice.

### Intl.RelativeTimeFormat (clj-quickjs-221)

RelativeTimeFormat appends six realm intrinsics. Each instance stores a boxed
immutable record on its task-owned ordinary object: decimal NumberFormat
configuration, style and numeric mode. The record contains owned Rust strings
and static data references, with no heap edges. The existing Plain category,
property/prototype tracing and ownership checks apply; freezing does not make
instances publishable across tasks. The native continuation traces its locale
list, options, receiver, value and unit across getters, ToNumber, ToString,
host-default lookup, waits, collection and migration. No callback retains a
Rust frame. `supportedLocalesOf` uses the common canonical list reader and
reads only localeMatcher, without observing a host default.

The existing importer pins `intl/relativetimeformat.c`, extracts its literal
unit/prefix/suffix tables, English narrow forms and auto-return switches, and
exports them through the same C/Rust value comparison. The Intl repacker emits
`data/relative_time.rs`; repeatability, input drift and literal edge checks run
in the existing verifier. No C engine is linked or run. The snapshot has no ±2
auto phrases. Its per-language auto branches and English fallback are retained,
including its numeric fallback for German/French/Spanish/Japanese zero.

Numeric patterns choose one versus other from shared PluralRules applied to
the rounded decimal operands. Other categories use the snapshot's other form.
NumberFormat supplies rounding, grouping, decimal symbols and mapped digits;
its unsigned decimal parts helper is reused by relative-time formatting.
Literal prefix and unit/suffix parts surround those number parts. Only numeric
parts receive the canonical singular unit. Joining the parts reproduces format,
including the snapshot's space before CJK unit strings. The agreed locale
subset and existing NumberFormat/PluralRules limitations still apply.

## Static module parsing (clj-quickjs-251)

`frontend::parse_module` selects the strict Module grammar; the script parser,
script compiler and CLI retain their script goal. The parser reuses ordinary
statement, pattern, function and expression parsing, records static declarations,
and validates module bindings and exports before returning. Function declarations
at module scope are lexical. Exported imports are partitioned into indirect
exports, except namespace imports, which remain local. Requests are deduplicated
by cooked UTF-16 specifier content while retaining first-occurrence order.

`frontend::ModuleRecord` (also `frontend::ast::Module`) wraps immutable Rust-owned
body syntax and static tables in one `Arc`. Clones share the complete record;
accessors expose only shared slices. It contains no GC handles, mutable bindings,
resolver, instances, world/version, or task state and is `Send + Sync`. This is
an explicit immutable code/syntax storage boundary, not a JavaScript heap object.
Default function and class declarations are distinguished from expressions in the body;
anonymous defaults use ECMA-262's inaccessible `*default*` binding name.

The pre-run loader below associates records with world/version. Cyclic and acyclic graphs
link and execute with separate instances per task (see "Module execution").
Static requests do not create dependency edges. This parser neither fetches
sources nor starts tasks; it imposes no filesystem resolution policy and does
not accept top-level await.

QuickJS references and provenance: `research/quickjs/quickjs.c` functions
`js_parse_import`, `js_parse_export`, `js_parse_from_clause`, `add_import`,
`add_export_entry`, `add_req_module_entry`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, were consulted as semantic references;
no C implementation is adapted. The five complete upstream source copies used by
`frontend::tests::module_records_of_quickjs_cases` are documented under
"QuickJS references and provenance" above.

## QuickJS collection fixture driver (clj-quickjs-178)

`tests/quickjs_collections.rs` selects the pinned assertion helper prologue
and each complete case from `quickjs_test_builtin_collections.js`, compiling
once per case in a fresh runtime/task. The Map range includes `test_map1`.
All cases run without source substitutions; only the fixture's WeakRef case
adapts the four reference-counting assertions to kept-object semantics and
adds one timer per iteration. Each timer captures that iteration's pair of
WeakRefs, collects in its new job, and checks both targets are gone.
Completion drains timer and finalization jobs; exceptions fail the test.

Map runs with 10,000 instructions per slice; other cases use 100, up to
10 million instructions per case. Every yield triggers a collection, and
Map/typed-array runs must yield. For the two WeakMap cases, harness code
wraps the task-local constructor before calling the unchanged body. It
retains the native map instances without retaining keys or values or
altering native methods. A returned persistent vector keeps the maps alive
for Rust's existing `weak_map_live_entry_count` diagnostic: one map in the
ordinary case and two in the cycle case must each have zero entries. No
runtime representations, collector edges or built-in semantics change.

## Upstream BigInt fixture driver (clj-quickjs-206)

`tests/quickjs_bigint.rs` compiles both complete pinned upstream files and
executes the BigInt test as a whole and its five cases individually in fresh
runtimes. Individual runs replace only the final five calls with one call.
The whole-file run leaves `__loadScript` undefined, so the upstream catch
handles the missing shell helper. The pi example receives only a one-line
`print` declaration using the existing leaf host-effect intrinsic; absent
`scriptArgs` and top-level `arguments` select its original 1000-digit default.
Its single output line is compared with the unchanged test_pi expectation.

Each run is bounded by 20 million instructions and 30 seconds, checked
between slices. Normal slices contain 50,000 instructions. The individual
2000-digit `test_pi` uses 1000-instruction slices, forces collection after
every yield and requires at least one yield. It completes across 56 yields
and collections, exercising BigInts live in explicit recursive frames.
The existing 1 Mbit BigInt cap and one-instruction arithmetic cost remain
unchanged; no new runtime behavior or representation is introduced.

## Modules (clj-quickjs-252)

1. **Timing:** the embedder calls `module::ModuleLoader::load(world, entry,
   host)` before spawning the entry task. It returns a complete static
   `ModuleGraph` or a `ModuleLoadError`; a failure is not a dependency-cell
   `Failed` outcome. Pre-run loading keeps source acquisition outside resumable
   VM frames. No JavaScript runs, no task is spawned, and no heap or waiter
   locks are held while the loader calls the host. Module loading is a separate
   `ModuleHost` protocol, not a leaf `Host::effect` operation.
2. **Resolution:** `ModuleHost::resolve(specifier, referrer)` maps exact cooked
   UTF-16 text and an optional opaque referrer key to a private numeric
   `ModuleKey`; `None` denotes the entry request. Aliases of one resolved
   specifier must return the same key. The engine only compares keys, never
   module path strings or JavaScript values. The recording in-memory host in
   `tests/js_modules.rs` resolves `./x` and `../x` relative to the referrer's
   directory and normalizes dot segments; bare names are looked up directly.
   An unresolvable specifier or fetch failure reports `ReferenceError: could
   not load module`, the requested specifier, referrer name and host reason.
   A syntax failure additionally retains the fetched source name and 1-based
   line:column. The first failure stops the walk before later host calls.
3. **Caching:** keep one embedder-owned `ModuleLoader` per runtime and host
   identity domain, because worlds and host keys are domain-local. Its cache
   keys are `(WorldId, ModuleKey)`, representing world/version plus resolved
   specifier. Sources and resolutions must remain immutable within a world.
   The embedder supplies a host snapshot matching the load's world; the loader
   does not select or mutate the host's active world.
   The cache stores `Arc<LoadedModule>` (original source plus the immutable
   parsed `ModuleRecord`), without heap handles, environments or task state.
   Graphs from repeated loads share these records; even unchanged sources are
   fetched and parsed anew in a new world, preventing stale-world reuse.
   Mutable module instances are allocated separately per task by linking;
   loading neither creates nor shares instances. Successfully parsed records
   remain cached after a graph failure; failed fetches/parses and partial
   graphs are not cached, so a retry resolves requests again.
4. **Dependencies:** imports are static graph requests, never recorded cell
   dependency edges or waits. `__rt.need` in a module body retains the ordinary
   task/world/result-kind semantics during module execution; the
   loader only parses it and never performs the need. Loading does not alter
   dependency identity or the script VM's dependency log.
5. **Source kinds:** host fetch returns `ModuleSource::JavaScript { name, text }`
   or `ModuleSource::Json { name, text }`. Hosts classify a case-sensitive
   `.json` suffix as JSON (`ModuleSource::from_name` implements this rule).
   Import attributes remain unsupported. JSON source text is carried verbatim
   with a synthetic default-export record, without JavaScript or JSON parsing
   during loading. `Runtime::spawn_module` parses JSON during task linking
   before any JavaScript frame runs (clj-quickjs-256).
6. **Binary:** `cargo run -- <file>` remains script-only. Embedders supply
   resolution and acquisition; the VM gains no filesystem access or CLI mode.

The graph walk is iterative depth-first in first-request source order. A node
is inserted before its requests are followed, so cycles and self-imports point
to the existing entry key rather than fetching a second record. Resolved keys
are retained in each node in the record's request order, including distinct
specifier spellings that alias one key. Each key is fetched/parsed once per
world in a loader, including across entry graphs. Host resolution may repeat
on cache hits. Graph iteration preserves discovery order. JavaScript linking
and evaluation, including cycles and namespace objects, are described below.

Verification: `tests/js_modules.rs` uses the existing licensed parse fixtures
for the three-module cyclic graph and the hello-module JavaScript imports,
plus synthetic JSON text and failure/cache/deep-graph cases. Loading tests
run no bodies; execution tests are described below. QuickJS references and
provenance:
`research/quickjs/quickjs.c` functions `js_default_module_normalize_name`,
`js_host_resolve_imported_module`, `js_new_module_def`, `JS_LoadModule`, and
`research/quickjs/quickjs-libc.c::js_module_loader`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, informed resolution/caching and
suffix classification; no C implementation was adapted. The existing fixture
copyright notices and provenance above remain applicable.

### Synthetic JSON defaults (clj-quickjs-256)

Policy (a) is retained: the host returns `ModuleSource::Json`, normally selected
by the resolved name's case-sensitive `.json` suffix. Thus the pinned
`examples/hello_module.js` JSON import works without adding attributes;
clj-quickjs-257 runs the complete example with a recording console binding.
Attributes, including `type: 'json5'`, remain explicit syntax rejections; JSON5 text is
rejected by the strict JSON parser. There is no filesystem access in the VM.

The world/version record cache retains immutable JSON source and a synthetic
`ModuleRecord` with one local `default` export, an inaccessible `*default*`
binding, no requests and no JavaScript body. Task preparation creates the
synthetic binding without lowering JSON as JavaScript. The saved linker first
allocates cells and resolves all exports/imports, then parses every JSON source
with the same bounded `vm::json::Parser` used by JSON.parse. Only after all JSON
values are initialized does JavaScript function instantiation and evaluation
start. Malformed JSON therefore fails the task during linking before any body
effect, even if an effectful sibling appears earlier in DFS order. SyntaxError
messages include the source name and the parser's 1-based UTF-16 line:column;
the existing depth (512 containers) and key (4096 UTF-16 units) limits apply.

The active parser lives in the traced linker continuation, so partial arrays
and objects remain rooted at budget and GC safepoints. A completed value moves
into the task-owned default binding cell, which is shared by import aliases
and the instance's namespace. Synthetic modules have no executable VM frames;
evaluation simply marks them complete through the existing DFS machinery.
The same cached record yields freshly parsed objects in each task, without
host refetches or dependency edges from imports. Existing object/cell tracing
and publication rules apply; no new heap kind or lock is introduced.

Verification: `tests/js_modules.rs` covers the exact upstream message text,
default import properties, shared importer identity and mutation, namespace
contents, star exclusion, fresh task instances from cached records, primitives,
strict JSON/JSON5 rejection, UTF-16 positions, limits and failures before any
body. One-instruction runs collect between every step, migrate on 64 KiB worker
stacks, wait in JavaScript and resume without refetching or reparsing an instance.

QuickJS references and provenance: semantic references are
`research/quickjs/quickjs-libc.c::{js_module_loader,create_json_module,json_module_init}`
and `js_module_check_attributes` / `js_module_test_json`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The exact data text from
`research/quickjs/examples/message.json` is included in `tests/js_modules.rs`
with the QuickJS MIT copyright/license notice. No C implementation was adapted.

### Intl.Segmenter (clj-quickjs-222)

`intl::segmenter::Data` stores one of three private slots on ordinary task-owned
objects: immutable locale/granularity; Segments (segmenter, input, flat text);
or cursor (segmenter, input, flat text, next UTF-16 position). Segments and
cursors trace their segmenter edge; immutable JsString bodies have no heap
edges. The collector also traces every native continuation's receiver,
segmenter, options and locale-reader state. No Rust borrow survives a step.
Freezing public properties never permits publication or foreign-task reads.
The append-only builtin and realm additions install service, Segments and
Segment Iterator prototypes; the last inherits the existing Iterator prototype.

The selected algorithm follows `research/quickjs/intl/segmenter.c` at
7d1c13c5d8f8, rather than claiming full UAX #29. Graphemes join the snapshot's
Hangul L/V/LV/LVT/T ranges, nonzero canonical combining classes, Thai
U+0E31..0E3A/U+0E47..0E4E, emoji modifiers U+1F3FB..1F3FF, variation selectors
U+FE00..FE0F, ZWJ and any character after ZWJ. Combining classes come from the
existing generated Unicode 17.0.0 `data::collation::CCC` table and its verified
quickjs-data pipeline; no new break-property input or importer is needed for
these rules. Consecutive regional indicators join in pairs, correcting the
snapshot's unbounded RI chain. Word segments group equal word-like status:
ASCII letters/digits, every non-ASCII scalar, and `. - + % $ _ '` are word-like;
other ASCII characters are not. This deliberately classifies emoji and
non-ASCII punctuation/whitespace as word-like and groups adjacent CJK letters.

Differences from UAX #29: no CR/LF or control special cases, Prepend,
SpacingMark, Indic conjunct, full Grapheme_Extend or extended Hangul property
rules; combining classes cover fewer marks than Grapheme_Extend; ZWJ joining
is unconditional rather than Extended_Pictographic-contextual. Word rules
have no UAX letter/number/Katakana/ExtendNumLet classes, ignored format/mark
handling, contextual mid-letter/number rules, newline or RI word rules, or
dictionary boundaries. Sentence requests throw RangeError instead of the
snapshot's single whole-input segment. Both iterator and containing use the
same scanner, including lone-surrogate word runs (the snapshot's containing
path separately splits these). All other documented heuristic outputs match
the snapshot; no complete UAX #29 conformance is implied.

`segment()` budget-flattens ropes in batches of 128 nodes/units, retaining the
original immutable input for record identity/content. Each boundary scan step
consumes at most one scalar. `containing` scans from zero with the same rules;
`next` starts at its saved cursor. Substring copies use StringBuild batches of
128 units, with no final linear copy. These continuations may yield, collect
or migrate between steps, including within a single enormous grapheme or word.
The cursor commits its next position only when its result is ready. Iterator
creation and resolved-options allocation are bounded leaf operations.

### Intl.DisplayNames code names (clj-quickjs-223)

DisplayNames appends five realm intrinsics (prototype, constructor and three
methods). Ordinary instances have a boxed immutable payload of Rust strings
and static option spellings, with no heap edges. Their properties remain
owned by the constructing task, including after Object.freeze. Receiver
checks enforce ownership before code coercion. Native saved stages trace
locale readers, options, the receiver and code across getters/ToString,
waits, collection and migration. Options are read in localeMatcher, style,
type, fallback order; languageDisplay is read only for language instances.
Calendar types longer than 32 UTF-16 units use UnitCursor and bounded lowercase
rope chunks; no generated calendar key or alias can match such a long code.

The existing verified `data/display_names.rs` already contains all snapshot
English/French code-name and language pools, emitted by `repack-intl.pl`.
The new safe reader checks alternating NUL-separated code/name pairs; a
consumer test checks all seven pools for both languages, including UTF-8 and
terminal sentinels. No generation changes or manual table copies are needed.
Language pools also feed the -224 reader below. Short/narrow region lookup tries the
short pool before the full pool; other types use the snapshot's single pool.
The shared service locale set preserves QuickJS's required `de` metadata and
supportedLocalesOf output, while name lookup explicitly falls back to English
when the resolved language has no data. This acceptance requirement refines
the earlier -209 policy row: translated-name coverage remains exactly en/fr.
Calendar aliases use canonical Unicode types with the snapshot's legacy data
keys. Lowercase currencies intentionally throw under the pinned-output policy.

DisplayNames and RelativeTimeFormat occupy variants of one boxed immutable
service payload, keeping the ordinary-object size at its previous baseline;
small-stack worker execution is checked without increasing test stack sizes.
Long-code validation moves input ownership into its cursor, skips normalized
output for fallback none, and drains input/partial-output cursors in bounded
steps before raising validation errors. Native-frame disposal after return or
validation failure has no pending rope tree. SupportedLocalesOf applies
ToObject-equivalent property reads to non-null primitive options, separately
from the strict constructor options rule. Callback failure tests preserve
thrown identities and dependency-failure causes after waits and GC.

### Intl.DisplayNames language names (clj-quickjs-224)

The immutable DisplayNames payload also handles language identifiers. The
consumer reuses `intl/tag.rs::LanguageTag` validation and canonical aliases
from Locale (-210), rejecting all singleton extensions including private use.
It does not maximize/minimize (-211): doing so would add or erase components
that the caller explicitly requested for display. The existing parser bounds
identifiers to 511 ASCII bytes; larger rope arguments use the native cleanup
cursor and throw RangeError without flattening or recursive rope disposal.

Dialect display tries an exact compound name, then language-script-region,
language-script and language-region keys, consuming only matched components.
Standard display uses the base-language name; unmatched components are named
through the sibling script/region readers and composed with variants in order.
An unknown base language returns the complete canonical code or undefined.
All styles use the snapshot's single language pool. Locale resolution and the
English fallback for test locales without translations remain shared with -223.

The verified exporter additionally pins `intl/displaynames.c` and imports only
the two literal `dn_fallback_language` pairs, not service code. Repacking adds
`dn_language_fallbacks` beside the existing language pools. Dialect display
prefers these complete Chinese spellings over partial dialect keys, preserving
the snapshot's English strings even in French. Standard display composes the
localized components. The snapshot omits localeDisplayPattern, separator and
variant-name data; `{0} ({1})`, `, ` and canonical unnamed-component codes are
the explicit en/fr composition policy. The pipeline checks C/Rust literal
equality, repeatability, input pinning and semantic edge cases. Source tests
cover aliases, dialect/standard names, fallback, validation, long codes, and
waiting languageDisplay/code callbacks with GC and worker migration.

### Intl.DurationFormat (clj-quickjs-225)

DurationFormat stores immutable NumberFormat configuration, ten unit styles,
ten display flags and optional fractional precision in ordinary task-owned
slots with no heap edges. Append-only builtin/realm additions install its
constructor and prototype. Publication remains task-owned even after freezing.
The native continuation traces locale-reader state, options and the source
record; option getters and numeric ToPrimitive calls run through VM frames.
Locale-reader, slot and scanner storage is boxed in the saved continuation.
Constructor entry and DurationFormat steps bypass unrelated large dispatch
frames; Intl namespace allocation is outlined from the realm builder. Tests
exercise cold initialization, duration getters and coercions on 64 KiB workers.
The source is read alphabetically once. Numeric strings use StringToNumber's
budgeted flatten/trim/grammar-validation stages and the bounded NumberParse
accumulator, so neither rope flattening nor long digit scans block a step.

After validation, at most ten bounded components use NumberFormat's decimal
parts, PluralRules and the shared unit ListFormat separators. Integral
binary64 subseconds convert to exact u128 magnitudes before combining;
range validation bounds total time below 2^53 seconds, and all intermediate
nanosecond sums fit u128. Fractional formatting passes an exact decimal string
to the number core with truncation, avoiding binary-float rounding and carries.
Parts preserve unit metadata; list/clock separators are separate literals.

The existing quickjs-data exporter now imports all 28 eight-field nullable
UnitPatternEntry rows from pinned intl/numberformat.c. The Intl repacker writes
units.rs and unit_provenance.txt; complete C/Rust value comparison, repeated
repacking, semantic edge cases, deliberate corruption and input hashes verify
these tables. Missing patterns retain the snapshot's unit identifier fallback,
including French and subsecond spellings. Data for NumberFormat unit style is
now present, but its constructor/options/compound-unit behavior remain a gap;
this pellet enables only the duration consumer. Locale coverage and the host
default-locale leaf remain the shared Intl policy.

### Module execution (clj-quickjs-253/255)

`Runtime::spawn_module(&graph)` lowers each immutable record independently,
then starts an entry task in the graph's world. A task-owned continuation
computes source-order DFS post-order and strongly connected components,
resolves named imports through
local/indirect export entries, and checks every link before any body runs.
Each graph edge, export-table entry, binding allocation or phase transition
consumes one budget step. Exported names propagate over reverse star edges
until stable, excluding default. ResolveExport then uses a saved pair set
and explicit frames; repeated (module, export name) pairs return no resolution
to the caller, allowing other star paths to find a binding. Only completed
dependency SCCs supply cached resolutions. Explicit exports override stars;
two star paths resolving to distinct origin bindings are ambiguous, while
two paths to one binding stay unambiguous. Explicit indirect exports are
validated even when unused. Missing exports fail the task with SyntaxError
naming the export and module. Default-export declarations fail explicitly for now.

The continuation allocates ordinary task-owned binding cells for all module
bindings before entering any instantiation frame. Each module has two
private strict executable entry points receiving those cells as internal
parameters: instantiation creates hoisted functions, then evaluation runs the
body. Imports receive the origin's cell, with immutable assignment semantics
preserved through closures and direct eval. Lexicals have dynamic TDZ checks;
var cells start as undefined. Declarations never create global properties and
top-level this is undefined. Function closures capture module cells, including
recursive references, so later writes to exported functions remain live.

The continuation selects ordinary VM entry frames without nesting Rust calls;
it retains traversal indices, tables and cells across runs. `VmState::trace`
traces these cells and prepared code, and the existing cell/function GC paths
trace their values and closure captures. Cells remain task-owned in
value::share. `Task::module_phase()` exposes linking, instantiating and evaluating
for scheduler diagnostics; it returns None for scripts, finished tasks and
snapshots unavailable during a concurrent run. There is no new heap kind, lock
or host fetch during a run. Budgets, need waits, GC and
migration preserve the exact continuation, including graph traversal, star
resolution, allocation and instantiation; no earlier module effect is replayed.
Bodies run once in source-request DFS post-order. Linking and evaluation
each use an explicit Tarjan walk (`src/module/dfs.rs`): discovery and ancestor
indices, call frames and an active SCC stack live in the continuation. A
back edge never re-enters a module; SCC members complete together, one member
per budget step. All module functions are instantiated before any body, so
mutually recursive functions work while cross-cycle let/const reads still
throw ReferenceError until initialized. Self-imports use the same live cells.
A throw stops the driver and records its original diagnostic for every
active SCC, including members whose bodies have already returned. Completed
dependency SCCs stay successful; selecting another member in the same
instance map returns the saved failure. Completion is
undefined. A finished task cannot be run again; a fresh spawn creates fresh
instances, including after failure. Imports create no dependency-log edges.
There is no public operation to select a second entry in a live instance map;
that cached-failure invariant is exercised by a module-driver unit test.
The namespace import variant of test_cyclic_import and named imports of its
re-exported function both compute 4. Export tables are fully resolved before
namespace creation and import argument construction. Namespace re-exports
terminate resolution at a canonical namespace origin, including within SCCs;
local exports of namespace imports use that same origin to avoid false star
ambiguity. All requested namespaces and initialized namespace cells are
created before population, so namespaces can contain one another while
lexical export cells retain their TDZ.

The task driver selects linking steps or the ordinary interpreter, returning
from the linking driver before entering a module frame. First-use global,
closure and object/arguments allocation use small opcode handlers, keeping
realm initialization within a 64 KiB worker stack. The module wait/migration
test also performs direct eval and forced GC on workers of that size.
Cycle tests additionally suspend on need, collect and migrate between single
instruction slices. A 10,000-module linear graph links and evaluates on a
64 KiB worker stack. Tests also cover circular indirect exports failing
before effects, star exports through cycles, shared leaves and once-only
evaluation order.

Semantic authority: ECMA-262
[InnerModuleLinking](https://tc39.es/ecma262/multipage/ecmascript-language-scripts-and-modules.html#sec-innermodulelinking),
[InnerModuleEvaluation](https://tc39.es/ecma262/multipage/ecmascript-language-scripts-and-modules.html#sec-innermoduleevaluation)
and [ResolveExport](https://tc39.es/ecma262/multipage/ecmascript-language-scripts-and-modules.html#sec-resolveexport).

QuickJS references and provenance: `research/quickjs/quickjs.c` functions
`js_create_module_function`, `js_resolve_export`, `js_inner_module_linking`,
`js_inner_module_evaluation`, `js_execute_sync_module`, and
`js_evaluate_module`, revision `7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`,
were consulted as semantic references, without adapting C code. The licensed
`tests/fixtures/modules/fib_module.js` copy of `examples/fib_module.js` is now
executed as well as parsed; its JavaScript body remains verbatim.

## SuppressedError and engine wrapping (clj-quickjs-234)

`NativeErrorKind::Suppressed` appends one constructor/prototype pair to the
realm's error inventory and appends the global operand. The fixed-size
constructor/prototype inventory is boxed so extending it does not grow
the realm initialization stack; `Intrinsics::all` roots all its objects.
It reuses
`Builtin::NativeError(kind)` and Error's native continuation, selecting the
third argument for resumable message conversion and retaining both input
values in traced continuation fields until their ordinary properties are
installed. It ignores options. Instances use the existing ordinary-object
Error marker, ownership, precise tracing and publication policy; no heap
kind or root category is added.

`VmState::suppressed_error(heap, error, suppressed)` is the crate-internal
engine entry point used by disposal lowering. It takes the
prototype directly from `realm::Intrinsics`, builds own `error`,
`suppressed` and `stack` properties, and allocates an ordinary Error without
consulting globals, properties, getters or JavaScript constructors. It uses
the current VM creation-site stack snapshot and leaves message inherited,
matching QuickJS's engine helper. Allocation/key exhaustion propagates the
existing terminal OutOfMemory fault. Like other VM allocations, it runs
under a mutator guard between safepoints: inputs stay in traced task/native
state, and the caller stores the result there before the next collection
poll. No collector can interleave inside this leaf operation.

User decision (Amend acceptance): the direct unit test attempts the intrinsic
`SuppressedError.prototype.constructor` overwrite and expects TypeError,
then replaces the global `SuppressedError` with a throwing function. This
keeps the shared immutable intrinsics design. The test verifies the original
prototype, exact input identity, descriptors, absence of own message, stack,
no callback effects, survival of
input objects after dropping their original roots, and nested wrapping
across collections. The existing shared-immutable-intrinsic decision
prevents the literal upstream intrinsic `prototype.constructor` write;
the test catches its TypeError and verifies the replacement global cannot
redirect engine wrapping. Supporting an actual write to the intrinsic would
require a separate change to the realm mutation policy;
this pellet does not introduce shared mutable prototypes or task overlays.

QuickJS references and provenance: `research/quickjs/quickjs.c`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, `js_error_constructor`,
`js_native_error_proto_funcs` and `js_new_suppressed_error` were read for
semantics. Rust implementation and synthetic tests were written fresh;
no upstream source or test text was copied.

### Complete Intl upstream fixture (clj-quickjs-226)

`tests/quickjs_intl.rs::quickjs_test_intl_cases` uses the existing compiler,
Runtime and SimTimeHost to execute each of the ten pinned upstream Intl
cases in UTC and UTC+05:30. The host's default locale is asserted to be
en-US. All 470 upstream definition/helper lines and their QuickJS expected
strings stay unchanged. The harness appends one selected case call, bounds
execution by instruction budgets, and collects between slices with live Intl
objects/native continuations. Each case/zone reports its unmodified status;
failures are accumulated so one case cannot hide another. No runtime
representation, locale tables or host interfaces change. Attribution and
the retained license are recorded in docs/attribution.md and the fixture.

### Super property references (clj-quickjs-138)

The parser admits a super property base only in method/accessor parameters and
bodies. Ordinary functions reset the context; arrows and runtime eval keep
lexical super deferred to -112. The base is never a standalone JavaScript value.
Lowering preserves a triple on the operand stack: current receiver, home-object
prototype, key. Assignment and deletion retain a raw key; reads and read/write
updates convert it first. `GetSuperBase` reads the executing function's existing
traced `home_object` edge and obtains its current prototype. `GetSuperElem`
searches that base but dispatches a getter with the separate receiver.
`PutSuperElem` performs the lookup-base descriptor decision before checking the
receiver's own descriptor and defining/updating its data property; inherited
setters run with the receiver. `PutSuperElemStrict` preserves class computed-key
strictness inside sloppy surrounding code. `ToSuperKey` reuses the string-hint
resumable conversion continuation without checking base coercibility. Assignment
converts through PutSuperElem after the right-hand side; typed-array receiver
descriptors validate before value coercion. Sharing faults are checked outside
sloppy ordinary-rejection suppression, including before rejecting a read-only
own descriptor on a built-in receiver. A numeric super write whose lookup base
is the typed-array receiver itself uses IntegerIndexed Set, preserving value
coercion even for invalid indices or detached buffers. `Dup3`
retains the triple for compound assignments and updates. Destructuring targets
use the same reference representation. Calls and tagged templates keep a second
receiver below the triple and use ordinary method-call instructions. Deletion
evaluates its key expression and uses `ThrowError(DeleteSuper)` without key
coercion or fetching a property.

All opcodes append to the registry (0xA3..0xA8); existing encodings are unchanged.
There are no new heap kinds or publication categories: the method's home object
already participates in precise tracing and transitive publication. Operand and
native frames root receivers, lookup bases, keys, setters and assigned values;
callbacks use ordinary resumable frames without retaining a Rust stack. Tests
cover one-instruction budgets, collections around waits and yields, worker
migration and 64 KiB worker stacks. Public class fields execute below (-139);
ordinary methods' lexical super in arrows/eval remains an explicit gap.
QuickJS is a semantic reference; this implementation uses
the existing Rust object model and VM continuations.

Generator `next` and Promise entry use small dedicated native handlers before
the general constructor dispatcher, so suspended methods can attach and resume
on 64 KiB workers. Tests retain compound super references across generator yield
and async await, collect while detached and migrate before resumption.

## Module namespace objects (clj-quickjs-254)

`HeapObject::ModuleNamespace` is a distinct object kind, not an ordinary object
or a Proxy. It holds the owning task ID and a BTreeMap from immutable JsString
names to resolved binding-cell IDs. JsString compares UTF-16 code units, so
own keys preserve module namespace order even for numeric-looking names.
The fixed Symbol.toStringTag property follows the exports. Property hooks
implement Get, HasProperty, GetOwnProperty, DefineOwnProperty, Set, Delete,
prototype reflection, extensibility and enumeration. Reads release the
namespace lock before reading a cell; presence checks never read its value.
There are no JavaScript getters, user-extensible internal comparisons or new
locks. A namespace on the prototype chain also rejects inherited assignment.

Namespace creation is lazy on static linkage requests and cached per task's
module instance, not on the world's immutable module record. The saved linker
first allocates requested namespace objects and their internal initialized
cells, then fills each namespace one export entry per budgeted step. All
binding cells already exist; namespace properties hold cells rather than
values, preserving TDZ and later writes. Namespace imports have immutable
local bindings. Both local exports of namespace imports and direct
`export * as ns from` resolve to the same canonical namespace origin, avoiding
false ambiguity in star barrels. IndirectExportEntry uses the unspellable
`*namespace*` import-name marker for namespace re-exports.

The linker continuation traces cached namespaces before any module body runs;
each namespace traces every export cell, which traces its current value.
`namespace_cells_retain_and_reclaim` tests retention and reclamation through
the namespace alone. `namespace_waits_collects_and_migrates` tests budgeted
linking and execution, forced GC, a dependency wait, fresh instances and
migration on 64 KiB workers without repeated host fetches or effects.
Namespaces are classified TaskOwned in value::share, even when empty and
frozen. Publication rejects them, containers reaching them and closures
capturing them; they never cross task boundaries as persistent values.

`tests/js_modules.rs::namespace_get_in_tdz_throws_reference_error` checks a
catchable ReferenceError through a namespace imported across a cycle.
Mutually referencing namespaces and circular barrels retain canonical identity
and live reads across single-step budgets and collection. The internal
`module::namespace` TDZ test additionally uses an uninitialized cell, proving that
Get, descriptors, Object.keys and DefineProperty throw catchable ReferenceErrors
while HasProperty and own-name enumeration succeed.

QuickJS references and provenance: `research/quickjs/quickjs.c` functions
`js_build_module_ns`, `JS_GetModuleNamespace`, `js_module_ns_has` and
`js_module_ns_autoinit`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`, were consulted as semantic
references. No C code or upstream test text was adapted for these additions.

## Public class fields (clj-quickjs-139)

`ClassElement::is_field` distinguishes a public initializer expression from a
method definition, including an omitted expression represented by `undefined`.
The parser establishes an initializer context for strictness, super properties,
new.target and the prohibition on arguments; ordinary nested functions/methods
reset that context while arrows inherit it. Private names and static blocks
retain their explicit rejection.

Lowering evaluates every computed key once in class-definition order. Each
field retains its converted key in an inaccessible class-scope binding cell.
After installing methods and initializing the inner class-name cell, lowering
creates an initializer method closure over the instance fields, with the instance
prototype as its immutable home object. An inaccessible initializer cell in the
class scope is captured by the constructor and its super-calling arrows. Base
constructors call it before parameter initialization; super-call lowering calls
it immediately after `BindThis`, preserving the super expression's receiver on
the operand stack. A separate method closure over static fields has the class
as home object and is called with the class receiver at definition time. This
also permits a static initializer to construct the class after its instance
initializer has been installed. All calls use existing `CallMethod` frames.

Initializer closures use the existing computed-property lowering for anonymous
value names and `DefineElem` to define own enumerable data properties in order.
`DefineElem` now accepts constructor-provided objects and function property
records as well as ordinary literal objects; define failures remain catchable.
It never performs a setter-based assignment. Existing exotic descriptor
conversion restrictions continue to fail explicitly.

No heap kind or opcode is added: initializer cells, method home objects,
converted keys and captured lexical bindings use existing precise tracing and
publication categories. Temporary class roots are cleared only after static
initialization. Field values/receivers remain in VM operands across suspension.
`EvalSite` carries the initializer early-error flag and lexical home-function
source; native eval captures that source alongside existing receiver/new.target
and binding/environment captures. Validation bounds and checks the extra source;
executable round trips retain it. Direct eval and initializer arrows use the
captured method's home object rather than their own current function. This
extension is confined to initializer contexts; ordinary methods' lexical super
arrow/eval gap remains explicit.

Tests cover definition order, computed-key reuse, descriptor attributes, setter
shadowing, base/default-parameter and derived/super timing, function-valued
receivers, direct/nested eval, lexical super, collection and migration during
waits, executable round trips and execution on 64 KiB worker stacks.

Integer-indexed fields on a constructor-returned typed array validate their data
descriptor before running object-valued numeric conversion through the existing
traced native write continuation; its result is the receiver, preserving
`DefineElem`'s stack contract. Typed-array constructor entry and non-coercing
metadata getters bypass the large general built-in dispatcher, keeping this
field path within the 64 KiB stack budget as well.

Field initializer expressions also charge four units in the existing weighted
frontend nesting budget: their lowering retains a class frame, an initializer
closure frame and a computed definition frame. The boundary-depth frontend test
now covers nested instance and static field classes on the same 2 MiB compiler
stack, with excessive input rejected as TooDeep.

A field initializer establishes a fresh eval context even when its class
expression is evaluated in an enclosing parameter default. Direct eval in
such initializers and their arrows retains the initializer receiver, home
object, and `arguments` early error, independently of the enclosing parameter
scope. Nested arrow parameter defaults also support direct eval (clj-quickjs-109).

The constructor records its resolved initializer access in `FnCtx` at closure
entry; lexical arrows capture that exact access. A nested class's computed keys
or heritage may call the enclosing constructor's super without resolving through
the nested class's still-uninitialized initializer binding. Derived super in a
parameter default initializes fields before any later default executes, including
when a field waits and resumes on a different worker.

Class-body lexing retries escaped keyword names in IdentifierName mode at a
newline where a field may end through ASI. Binding, reference and label parsing
still reject the reserved spelling when the token belongs to an expression.
This preserves both initialized/uninitialized fields before escaped names and
multiline accessors without treating get/set as line-restricted modifiers.

### Lexical super contexts (clj-quickjs-112)

The lowerer captures the enclosing method function through arrows, memoizing one
capture per arrow as for `new.target`. Super property bytecode uses that method's
home object and a separate lexical receiver, so existing resumable property/key
conversion paths remain unchanged. Direct-eval descriptors carry optional method
and derived-constructor field bindings, alongside the raw receiver and construct
target. Presence of these bindings controls eval's parser permissions. The
validated descriptor counts every hidden capture and checks its source index.
Nested eval and eval-created arrows forward the same bindings; ordinary functions
establish new contexts. In derived constructors the captured receiver is the
existing shared binding cell. After `BindThis`, eval runs the captured field
initializer on the ordinary VM call path. No native interpreter recursion or
new heap kinds are introduced; existing closure and eval tracing retains all
values across suspension and prevents publication of task-owned home objects.

QuickJS references and provenance: `tests/fixtures/js/quickjs_super_arrow.js`
adapts the super row in `research/quickjs/tests/test_closure.js`,
`test_arrow_function`, revision `7d1c13c5d8f8` (VERSION 2026-06-04), replacing
`assert` with a boolean script completion and retaining its MIT notice. Other
new tests are synthetic fixtures, not light-cljs output. QuickJS's `OP_get_super`
and `js_method_set_home_object` in `research/quickjs/quickjs.c` are semantic
references; the Rust capture and eval transport follows this runtime's design.

## DisposableStack (clj-quickjs-239)

The append-only `HeapObject::DisposableStack` holds an ordinary property store,
its owning task, a disposed flag and resource records (value, method, sync/adopt/
defer hint). Instances remain `TaskOwned` even when empty, disposed or frozen;
publication rejects them with kind `DisposableStack`, the owner and the path
through the enclosing value (for example `value[0]` in a persistent vector).
No callback runs under the object-table lock. GC traces the prototype, ordinary
properties and both value/method edges in each resource record. Root inventory:
`DisposableStackResources` is a PersistentHandle holder, mapped to the new kind;
`disposable_stack_resources_retain_and_reclaim` verifies all those edges and
reclamation when the holder is dropped. The native-frame holder also covers
resources and pending errors after ownership transfers into a running disposer.
`disposable_stack_continuation_roots_survive_collection` additionally collects
with a continuation-only use receiver and after every disposal instruction,
covering anonymous errors held only by native catch and suppression state.

The shared immutable realm adds the constructor, unbranded prototype, five
methods and disposed getter. `[Symbol.dispose]` aliases the dispose function
object, and the prototype's `[Symbol.toStringTag]` is `DisposableStack`.
Construction requires new; its native continuation reads new.target.prototype
resumably, falling back to the intrinsic prototype, including derived classes.
Move allocates directly with that intrinsic prototype and transfers the resource
vector, marking the source disposed. It never reads the global binding or any
constructor property. Instance ordinary properties stay with the source.

Use shares AddDisposableResource with using declarations: it snapshots and
validates Symbol.dispose through a traced getter continuation before appending.
Nullish values are returned without recording. Adopt and defer validate callable
callbacks without invoking them. Dispose marks the stack disposed and transfers
its records into `native::dispose::Run` before calling anything, making re-entrant
dispose a no-op. The same loop runs the `using_dispose` instruction for lexical
scopes. It pops one resource per step, calls methods with the resource receiver,
adopt callbacks with undefined receiver and one argument, and defer callbacks
with undefined receiver and no arguments. All calls are ordinary resumable VM
frames: budget yields, dependency waits, GC and migration preserve progress and
never replay effects. Native catch handlers retain each new error; the next step
combines it with the previous error via the intrinsic SuppressedError helper.
An Option distinguishes no error from thrown undefined. After all resources,
a changed error overrides the pending completion, except during cancellation
cleanup where errors are discarded so outer scopes can continue unwinding.
AsyncDisposableStack extends this representation and loop (clj-quickjs-241),
as described below.

### Dynamic Function compilation (clj-quickjs-111)

`native::eval::DynamicFunction` saves argument values in a traced native
continuation and requests ToString through the VM trampoline, one argument
at a time. It builds bounded rope source and delegates flattening, UTF-16
validation and the temporary 2 MiB compiler thread to eval's existing State.
The parser verifies that the generated parameter-closing token and final
body-closing token are consumed at their expected offsets, preserving the
separate parse goals and rejecting delimiter/comment injection. Lowering
creates ordinary function code without a lexical `anonymous` self binding
or caller captures. The completion allocates a normal FunctionObject;
calling it uses ordinary VM frames, GC tracing and publication rules.

QuickJS references and provenance: semantic reference only,
`research/quickjs/quickjs.c`, `js_function_constructor`, at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. Tests are original synthetic
fixtures; no upstream source or test text was copied.

## Await using (clj-quickjs-240)

The frontend records `LexicalKind::AwaitUsing` in async function, arrow and
method statement lists. It uses the existing const binding and finally scope
lowering. `using_check_async` returns the method and a private numeric hint
(1 async, 2 sync fallback); nullish resources return undefined with hint 1.
Hidden hint slots start at 0, so a failed or unreached declaration never
introduces an await. The getter continuation reads asyncDispose first, then
dispose only for a nullish result, with ordinary VM callback frames.

`using_dispose_async` transfers triples of value/method/hint into the same
`native::dispose::Run` used by synchronous lexical scopes and DisposableStack.
Sync entries in a mixed scope use hint 0. After calling an async entry, Run
requests the ordinary PromiseResolve/await native operation, retaining itself
and its catch handler inside the detachable activation. A fallback drops the
method's return value before awaiting undefined. A fallback throw retains its
error across an await, matching GetDisposeMethod's rejected promise wrapper;
that await also covers nullish records (clj-quickjs-285). Direct method throws
continue without awaiting. Nullish entries coalesce into a pending await unless
an async disposer supplies it (clj-quickjs-284). Callback throws and rejected
awaits merge into the existing traced error state, then disposal continues in
LIFO order.
Async-boundary lookup skips native frames inside the current JS activation;
detach/attach already traces and relocates those frames and NativeCatch indices.
No heap kind, dependency wait, JS wrapper function or second disposal loop is
introduced. Await during cancellation ends the cleanup and unwinds outer
finally scopes under the existing cancellation policy.
The native action returns through `Flow::Resume(Instr::Await)` before executing
the ordinary await, and promise stepping bypasses the unrelated general native
arms. This bounds dispatcher stack use on 64 KiB workers, including disposal
getters, thenable callbacks, GC and migration.

QuickJS references and provenance: `research/quickjs/quickjs.c`
`check_await_using`, `js_op_using_check`, `js_sync_dispose_wrapper`,
`js_emit_dispose_scope`, and the async dispose/merge opcode cases at revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2` are semantic references only.
The validation/fallback rows in `tests/js_source.rs::await_using_declarations`
adapt `research/quickjs/tests/test_language.js::test_await_using` at that revision;
the QuickJS MIT copyright/license notice is retained in that test file.
AsyncDisposableStack shares this disposal path (clj-quickjs-241); for-await-using
and module top-level await using are explicitly unsupported (top-level await is
absent).

## AsyncDisposableStack (clj-quickjs-241)

The existing `HeapObject::DisposableStack` carries a private Sync/Async brand.
Both classes retain the same task ownership, property operations, resource
tracing and `TaskOwned` publication classification; no new heap kind or opcode
is needed. Constructor, use, adopt, defer, move and disposed share the original
implementation with a class parameter. Move chooses the matching immutable
intrinsic prototype without consulting globals or user properties. Async use
shares await-using's traced method lookup and fallback hints, retaining nullish
records to perform the required await. The realm adds operand 44 for the
constructor, plus its prototype, methods, getter, asyncDispose alias and tag.

The public disposeAsync builtin enters a small, immutable internal executable:
AsyncStart, GetThis, GetCapture, CallMethod, Return. The captured private builtin
starts `native::dispose::Run` with await-all enabled. The usual async boundary is
established before receiver validation and before the first synchronous callback;
an invalid receiver therefore rejects a promise. Empty/disposed stacks finish
without awaiting. Each active call transfers the resource vector after marking
the stack disposed, so a second call returns its own fulfilled promise without
stealing the pending resources. The driver code is shared in a OnceLock; each
call's closure and movable activation are ordinary traced VM state owned by the
calling task. The private builtin is a rooted realm intrinsic, never a user
property lookup or a JavaScript-visible entry point.

The shared loop awaits async use/adopt/defer results, discards sync fallback
results, and coalesces nullish records with other pending awaits. A synchronous
throw from a direct async method or adopt/defer callback records the error and
continues immediately. A sync fallback throw awaits before the next resource,
matching its rejected promise wrapper (clj-quickjs-285). Await rejections enter
the same intrinsic SuppressedError path. Pending errors remain traced until final
rejection, including thrown undefined. The ordinary
async machinery detaches and resumes the disposal native frame and its catch
handler; all callbacks remain VM frames and `__rt.need` remains a dependency
wait. `async_disposable_stack_callback_waits_and_resumes` collects the task and
remaining callbacks while waiting, migrates on completion, and verifies LIFO
effects and exactly one promise settlement. Source tests check the five upstream
groups, receiver/disposed validation, intrinsic move, fallback, tags, reentry,
thenables and await-using composition.
Stack methods bypass the large general builtin-start dispatcher; the wait and
migration test runs both phases on 64 KiB worker stacks.
`async_disposable_stack_detached_disposal_survives_collection` additionally
collects between budget slices, suspends the disposal activation at an await,
then waits inside a resumed callback with an earlier anonymous error and a
remaining adopted resource held by the detached continuation. It migrates on
64 KiB workers and verifies the complete suppression chain and single settlement.

## Proxy enumeration (clj-quickjs-91)

The append-only Proxy heap kind traces its target and handler. Proxies belong
to their creating task, including frozen proxies, following ordinary-object
publication rules. `ProxyEdges` is a PersistentHandle holder covered by
`proxy_edges_retain_and_reclaim` (lib inventory): both edges survive collection
while only the proxy is rooted and are reclaimed after that root is dropped.

`vm::proxy` keeps an explicit stack of OwnPropertyKeys/GetOwnProperty operations
inside a traced native frame. Trap acquisition, calls, array-like list reads and
descriptor conversion progress without a retained Rust stack. Nested target
proxies use that same stack. Descriptor conversion reuses the resumable
ToPropertyDescriptor cursor; invariants use completed descriptors and
`validate_and_apply`, without changing the target. The target descriptor and
extensibility are observed before converting a reported descriptor. Own-key
invariants snapshot extensibility before reading target keys and descriptors.

For-in retains its heap enumerator and requests proxy key lists and descriptors
from this continuation. Its current object and all symbol keys are traced;
missing exotic keys do not hide inherited properties. Ordinary enumeration keeps
its existing order and prototype walk. Object reflection, rest, JSON object
key enumeration and freeze queries use the same operations. Unsupported traps
fail explicitly by name; absent traps forward. Callable and array brands follow
the target. No opcode was added.

QuickJS references and provenance: consulted `research/quickjs/quickjs.c`
`js_proxy_constructor`, `js_proxy_get_own_property_names`,
`js_proxy_get_own_property`, `js_for_in_next`, and adapted
`research/quickjs/tests/test_loop.js::test_for_in_proxy`, revision
`7d1c13c5d8f8c6b32893786ed35cdcebd88194d2`. The test body is unchanged except for
its assert helper and invocation; the MIT notice is retained in
`tests/js_source.rs`. Remaining cases are synthetic fixtures, not light-cljs
output. Rust code is original. Revocation and other traps are deferred.

Proxy forwarding uses a bounded explicit leaf cursor for handler/property walks;
a handler inheriting from its own proxy cannot recurse through Rust frames.
Property and prototype leaves bypass unrelated dispatcher temporaries on 64 KiB
executor stacks. Forwarded for-in keys always reach descriptor lookup, including
keys non-enumerable on the target. Sloppy writes preserve unsupported-trap
exceptions. Bind's own-length query uses the descriptor continuation; typed-array
writes and definitions select the forwarded target before resumable value
coercion, preserving the original assignment/definition result. Object-valued
ownKeys lengths are tested through waits, collection and migration. Prototype
ownership/cycle checks include transparent proxy targets.

## Semantic corpus audit and malformed-input rejection (clj-quickjs-30)

`tests/quickjs_language.rs` shares one driver between the operator fixture and
the new parse/literal fixture: each case is compiled once in sloppy mode with
the unmodified upstream helpers and definitions, runs in 100-instruction slices
with a collection after every yield, and must complete with undefined. The
pinned line counts guard the fixture text against silent edits.

`tests/malformed_input.rs` is the rejection half of the corpus. The truncation
sweep compiles every line-boundary prefix of five corpus fixtures and every
character-boundary prefix of a synthetic program; a rejected prefix must carry
a span inside the source at character boundaries with a real line/column, and
a compiled prefix runs for a bounded slice (any outcome, no panic). A table of
the `docs/grammar.md` "Rejected forms" rows, including every reserved-binding
misuse of `__rt`, is checked through `compile_script_with` so compile-time
rejections (async generators) are covered as well as parse-time ones. The
bytecode rows are early VM tests with hand-built code, not source support:
backward and end-of-code branch targets, catch targets inside operands,
truncated branch operands, loop back-edge and handler-entry stack mismatches,
oversized call/dup operands, empty capture/local/constant tables, and a
run-time handler mismatch (`PopCatch`, `PopFinally`, `EndFinally` without a
handler) that fails the task with `Internal` rather than panicking.
