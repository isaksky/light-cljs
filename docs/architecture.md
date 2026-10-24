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
| array elements | `TaskFrame` (`Array` → `Array` → argument) | `array_element_retains` | `array_element_reclaims` |
| function own-property record (clj-quickjs-125) and its values, the lazy `prototype` object among them | `PersistentHandle` (`Function` → `PlainObject` → `PlainObject` → argument) | `function_properties_retain` | `function_properties_reclaim` |
| bound function target, bound receiver, bound arguments and immutable metadata (clj-quickjs-103) | `PersistentHandle` (`BoundFunction` → target closure / receiver / arguments / metadata) | `bound_function_retains` | `bound_function_reclaims` |

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
  `src/vm/instr.rs`). Its operand is a reference when it is a name: with
  no global object, a name that resolves to no binding is known
  statically, so the compiler emits `undefined` (then `typeof`) instead of
  `throw_error`; once a global object exists that name must be looked up
  at run time. A binding in its temporal dead zone is read as usual, so it
  still throws.
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
  `get_this` for `this` in a non-arrow function; the parser still rejects
  it at the script top level and in arrows (clj-quickjs-104, -98).
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
  script bindings until the global object (clj-quickjs-104) exists. A
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
- **Catchability.** ReferenceError, TypeError and StackOverflow failures
  are raised as fresh immutable error objects (a native heap payload with
  no edges, shareable); other failures are uncatchable and end the run
  without running catch or finally blocks, as QuickJS's uncatchable
  errors do. Failed or cancelled dependencies became catchable, with the
  error object's cause as its one edge, in clj-quickjs-27 (below).
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
  takes: if some handler could see it, a fresh `ErrorObject` (error plus
  `cause` = `e`, its only heap edge, so it is shareable exactly when `e` is)
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
  Rust ownership of acyclic leaf buffers, not reference counting of heap
  objects: a string references nothing, so no cycle can pass through one.
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
  `length` will be `JsString::len` (stored count, O(1)); indexing reads
  `code_units()[i]` (clj-quickjs-142); relational order (`<` etc.,
  clj-quickjs-68) is `JsString`'s `Ord`, lexicographic over code units
  (QuickJS `js_string_compare`), not Unicode scalar order. Reading a
  string never panics on a lone surrogate: `code_units()` is exact,
  `Debug` is exact (a quoted literal that writes a lone surrogate as
  `\u{d800}`; `src/main.rs` prints a string result with it), and text for
  diagnostics and host effects (`to_string_lossy`, `Display`) maps a lone
  surrogate to U+FFFD.
- **Room for later representations.** Every string uses the 16-bit form
  and concatenation copies both operands into a new flat body (an empty
  operand returns the other's body). A Latin-1 body (QuickJS's 8-bit
  `JSString`) or a rope body for repeated concatenation (clj-quickjs-146,
  flattened on first code-unit access) fits inside the private body type
  without changing `JsString`'s interface, `Value`, or publication, since
  either is still immutable leaf storage. The length limit is QuickJS's
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
  `"0"` for `-0`). Radix 2..36 is clj-quickjs-149. The compiler emits
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
Caught error objects have JavaScript properties (`name`, `message`, ...)
whose access fails with `UnsupportedOperand` until clj-quickjs-230;
so did numbers and strings until clj-quickjs-73 ("Primitive bases"), and
so do booleans (clj-quickjs-39). `undefined` and
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
  `Object` captures nothing (the name compiles to `get_builtin_global 0`,
  resolved through the running task's runtime); one that captured
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
  shadows it. `typeof Object` is `"function"`. Assigning one is a
  compile-time `Unsupported` error until the global object makes these
  its properties (clj-quickjs-104).
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
  `Array` stub as its `constructor`) and the `Array` global and methods
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
ordinary object (not itself an array, which is unobservable while nobody
can write to it) inheriting from `Object.prototype`, with an own `length`
of 0, a `push` method (clj-quickjs-155), a `toString` that fails explicitly
until `Array.prototype.join` exists (so `[].toString()` never answers
`"[object Array]"`), and a
`constructor` that is the `Array` built-in, a stub that fails when called
and is also a global name (clj-quickjs-155; construction remains
clj-quickjs-39). Every array literal
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
  "Array length changes" below). The `Array` constructor, `Array.isArray`
  and the remaining `Array.prototype` methods belong to their own built-in pellets.
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
SameValue rewrites. It reports `Rejected(reason)` or `Accessor` (a define
that needs accessor properties, clj-quickjs-120) and changes nothing in
either case. `Object.defineProperty` and `Object.getOwnPropertyDescriptor`
are `Builtin` variants appended to the realm's `Object`; they run inside
their call instruction like every built-in so far.

- **Descriptor reads are `[[Get]]`s.** ToPropertyDescriptor reads the six
  fields through `property::find_field`, the prototype-chain read that
  tells an absent field from one holding `undefined`, so inherited fields
  count. No field read can run JavaScript until accessors exist; when
  they do (clj-quickjs-120), these built-ins must become resumable natives
  (`crate::vm::native`) so a getter runs as an ordinary VM frame across
  which the task may yield, wait or migrate. The decision point is the
  `Builtin` dispatch, as "Realm intrinsics" describes.
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
- clj-quickjs-120 turns the property value into a data/accessor enum and
  replaces the `Accessor` outcome with real accessor definitions;
  `getOwnPropertyDescriptor` then reports `get`/`set` instead of
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
  functions). Error objects would report `Error.prototype`, which does
  not exist (clj-quickjs-230): both directions fail with
  `UnsupportedOperand` rather than answer `null`. A function used as a prototype
  (`Object.create(function () {})`) lends its own properties, read and
  write-checked through its record (`function::read`, `own_store`),
  with their attributes; the names `Function.prototype` lacks fail
  explicitly through the chain as on the function itself, and
  `Function.prototype`'s own (`call`, `apply`) are inherited. The `__proto__`
  setter always receives the original base, not the record a function's
  write is redirected to.
- **`Object.prototype.__proto__` without stored accessors.** Accessor
  properties are clj-quickjs-120, so the accessor is not a property entry:
  the realm interns `__proto__` once and `Intrinsics::is_proto_accessor`
  recognizes it, and the property walk applies the getter (`find`) or the
  setter (`check_inherited` answering `Chain::ProtoSetter`) when an access
  of `__proto__` reaches `Object.prototype` without meeting an own
  `__proto__` on the way, which is exactly when the real accessor would
  run. A write of `__proto__` first walks the chain read-only
  (`reaches_proto_accessor`) and, when it meets the accessor, runs the
  setter before any data-property rule: an intrinsic receiver may make
  the no-op `Object.prototype.__proto__ = null`, and a function gets no
  property record for an ignored assignment. Both are Rust and call no
  JavaScript. Reads and writes are
  therefore exact; only reflection on `Object.prototype` itself differs,
  and it fails explicitly (`getOwnPropertyDescriptor`) instead of
  answering `undefined`.

**Alternatives considered.**

- *A native-accessor property kind now*: the property entry would become a
  data/accessor enum with a Rust getter/setter variant ahead of
  clj-quickjs-120, which designs that enum for JavaScript accessors and
  their resumable calls. Two designs of one entry would conflict; the walk
  hook is a few lines that clj-quickjs-261 deletes once the accessor can
  be stored with built-in getter and setter functions.
- *Lowering `__proto__: p` to a call of `Object.setPrototypeOf`*: needs
  the realm's function on the stack and has the wrong semantics (a
  non-object value must be ignored, not a `TypeError`). A one-byte opcode
  matches QuickJS and allocates nothing, so it is not a poll point.
- *Allowing only ordinary objects as prototypes*: ECMAScript accepts any
  object; the runtime's other values already behave as objects without
  properties, so they end a chain like `null` does, and functions and
  error objects fail explicitly until their properties exist.

**Consequences.**

- clj-quickjs-120 adds the accessor storage; clj-quickjs-261 then stores
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
  Error objects fail with `UnsupportedOperand` until their properties
  exist (clj-quickjs-230).
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
same way). A key with no name would be a symbol (clj-quickjs-127) and is
skipped, which is what both built-ins require. `for`-`in`
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
  constructor's lazy `prototype` (clj-quickjs-101). It fails with
  `UnsupportedOperand` for `Object.prototype` until `__proto__` is stored
  (clj-quickjs-261); `Object.keys` of either is exact, since none of the
  missing properties is enumerable. Error objects fail until their
  properties exist (clj-quickjs-230).
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
`@@hasInstance` lookup (no symbols yet), then OrdinaryHasInstance. Bound
functions delegate to their targets (clj-quickjs-103); otherwise the right operand must be a callable
object, a primitive left operand is `false`, `F.prototype` is read with
the ordinary property walk (a built-in method such as `Object.is`
inherits no `prototype` from `Function.prototype` or `Object.prototype`,
so it is `undefined`) and must be an object, and the chain is walked
with `[[GetPrototypeOf]]` (`prototype::get_prototype_of`), so its owner
rules apply, a function's chain runs through `Function.prototype`
(clj-quickjs-102: `f instanceof Function`, `f instanceof Object`), and an
error object fails with `UnsupportedOperand` until clj-quickjs-230 gives
it a prototype. The walk only reads; chains are acyclic,
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
`Unsupported` in sloppy code until clj-quickjs-105. The
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
property allocates nothing.

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
properties (`length`, indices) are answered before the walk, today by an
explicit `UnsupportedOperand` until clj-quickjs-142 implements them. A
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
rather than exotic Number/String objects; the two methods treat them as
holding `+0` and `""` (`String.prototype` has its own `length` 0), and clj-quickjs-72 adds `[[NumberData]]` /
`[[StringData]]` for wrapper objects. `Number` and `String` are built-in
globals now (operands 1 and 2 of `get_builtin_global`) whose calls fail
explicitly until clj-quickjs-72, so `Number.prototype` is reachable by name.

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
  but script-level ones are script bindings, not global object properties;
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
  identifier tables are generated from perl's Unicode database, not from
  `libunicode-table.h`. No code was copied.
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
  expression's value; `typeof Object` waits for a global `Object`. The
  notice is on `identifiers_are_not_normalized`, which also runs the
  `typeof õ` case of `test_unicode_ident`. `research/quickjs/quickjs.c`
  `js_operator_typeof`, `OP_typeof`, `js_parse_unary` and
  `OP_get_var_undef` were consulted only, for semantics: here an
  unresolvable operand is decided statically (there is no global object)
  and `type_of` in `src/vm/dispatch.rs` classifies this runtime's value
  kinds. No code was copied.
- `tests/js_source.rs` `strings_are_code_unit_sequences` (clj-quickjs-141):
  the literal rows of the unicode group of `test_string` (line 235) and its
  `"\0"` row (line 339) of `research/quickjs/tests/test_builtin.js`,
  revision 7d1c13c5d8f8. Modified: each `assert(a, b)` is a Rust assertion
  on `a === b`; `String.fromCharCode(0x20ac)` is the literal `"€"` and
  `eval('"\0"')` the literal itself; the `length`, `charCodeAt`,
  `codePointAt` and `String.fromCodePoint` rows and `test_rope` wait for
  string properties and methods. The notice is on
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
immutable `ErrorObject` allocation/unwinding path. No new heap kind or
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
name `push`. The existing `Array` constructor stub is exposed through an
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
Symbols, wrappers and resumable conversions inside leaf built-ins remain
with their own pellets.
