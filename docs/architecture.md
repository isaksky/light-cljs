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
  this slice.
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
  task are gone; the reclamation test drops a cell table directly.

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
  | `TaskOwned` | builders in every state, plain objects, natives declaring it | never |
  | `ConcurrentReference` | atoms (clj-quickjs-10); reserved for any heap-object handle to a dependency cell (cells are runtime-owned, not heap objects) | always; not descended into |

  Primitives are shareable. Plain objects are the ordinary mutable
  JavaScript objects of §4 (property storage comes later), so they count as
  task-owned mutation now. A native payload declares its category through
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
- **Cache.** Each heap slot has a sticky known-shareable bit. A persistent
  object gets it at allocation when all its parts already have it
  (`Heap::alloc_persistent`: O(parts) for an update, never a walk), and
  every object a successful check expanded gets it afterwards. The walk
  stops at known objects, so values built bottom-up from shareable parts,
  republished values, and versions derived from them are accepted in O(1).
  This is valid because persistent edges never change and no object's
  category changes; the bit is rewritten when a slot is reused.
- **Entry point.** `Runtime::publish(&Rooted)` is what the embedder calls
  before handing a value from one task to another. Nothing is copied, so
  the receiving task sees the same identities. `Runtime::spawn` does not
  check its arguments: the embedder decides what each task receives, and
  builders and binding cells stay protected by their owner checks
  regardless. Later runtime handoffs (dependency results, atom contents)
  call `share::check` themselves; closure captures are function-object
  edges, which the walk follows (`.capture(i)` in a path).
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
  `throw_error` messages quote the binding name from the source.

## Calls and frames (clj-quickjs-4)

```
 VmState (task-owned)
   entry: Frame ─┐   calls: [Frame, Frame, …]      each Frame = { code: Arc<Code>,
                 │                                   callee, pc, base }
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
- **Tracing.** The task root source traces the whole value stack and each
  frame's function object; a function object's `Trace` reports its code's
  constants and captured values, and the entry frame's constants are the task's executable
  constants. A callee that is no longer on any operand stack stays alive
  through its frame.
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
  shareable values is known shareable at allocation and publishable; one
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
  zone check for closures that run early. Assignment to a cell goes through
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
  cell. Within the declaring function, the static TDZ decision still holds.
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
- **Deferred.** String values and property access (so no `f.call`, method
  calls or object literals); `this`, `arguments`, `new`, default/rest
  parameters; var-scoped redeclaration by function declarations (rejected as
  `Unsupported`); an initialize-once immutable cell, which would let a
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
  `let`/`const` for `var`; `log` appends two decimal digits per value to a
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
- `tests/fixtures/js/quickjs_try_catch.js` (clj-quickjs-11):
  `test_try_catch1`–`test_try_catch4`, `test_try_catch6` and
  `test_try_catch7` of `research/quickjs/tests/test_loop.js`, revision
  7d1c13c5d8f8, as JavaScript source compiled by this frontend. Modified:
  `let` for `var`; logged letters are digits appended to a number and the
  thrown strings are those digits (no string values); failure flags instead
  of `assert()` calls. `test_try_catch8` (`for`-`in`, object literals) is
  omitted. `test_try_catch5` was added with clj-quickjs-79 (`while (true)`
  for `for(;;)`). The file carries the full notice.
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
  frontend. Modified: `let` for `var`; `typeof õ` (not in the subset) is a
  reference to `õ` that must fail with a ReferenceError; the declared name
  is also read back. The test carries the full notice; the name `Ãµ` also
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
  value; the string operand cases of `test_cvt` are omitted (they need
  StringToNumber). The test carries the full notice.
  `research/quickjs/quickjs.c` `js_parse_expr_binary`,
  `js_binary_logic_slow`, `JS_ToInt32Free`, the `OP_shr` / `OP_sar` cases
  of `JS_CallInternal` and `js_unary_arith_slow` were consulted only. No
  code was copied.
- `src/frontend/tests.rs` `break_and_continue` and `tests/js_source.rs`
  `while_break_and_continue` (clj-quickjs-78): the first statement of
  `test_labels2` in `research/quickjs/tests/test_language.js` (`while (1)
  label: break` followed by a line break) and the body of `test_while_break`
  in `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8. Modified:
  the label is dropped (labelled statements are not supported) and the
  next line is `x`, checked as a parse tree; `let` for `var`, `x = x + 1`
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
- `src/frontend/tests.rs` `number_literal_member_access` (clj-quickjs-48):
  the six cases of `test_number_literals` (line 667) of
  `research/quickjs/tests/test_language.js`, revision 7d1c13c5d8f8
  (`0.1.a`, `0x1.a`, `0b1.a`, `01.a`, `0o1.a` and `eval('0.a')`).
  Modified: each case is checked as a parse tree or a lexer rejection
  instead of evaluated (`undefined`, or a SyntaxError for `0.a`), and
  `01.a` is rejected because legacy octal literals are not supported. The
  test carries the full notice. No code was copied.
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
  head) is left to the `var`-heads pellet. The test carries the full
  notice. The `TOK_FOR` case of `js_parse_statement_or_decl` in
  `research/quickjs/quickjs.c` (the head's scope, `continue` targeting the
  update, the loop variables closed before the first iteration and before
  each update) was consulted only. No code was copied.
