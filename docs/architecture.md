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
 │ constants, max stack;  │shared│ Mutex<TaskBody> = execution guard   │
 │ immutable, no heap refs│      │   Ready(VmState) | Finished         │
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
  task's `VmState`: instruction position, operand stack, locals and diagnostic
  context (one frame for now; explicit call frames are pellet clj-quickjs-4).
  A worker thread only borrows it for one run.
- **Handles** (`src/heap/roots.rs`): embedders never see raw values. `Rooted`
  is a persistent root (how results are returned); `Local<'s>` is valid only
  inside `Runtime::scope`; `WeakHandle` names a value without keeping it
  alive. A Rust local holding a raw value is not a root.
- **Host** (`src/host.rs`): the narrow embedder trait for leaf effects, passed
  as `&mut dyn Host` to one run.

## Run protocol

1. `Task::try_begin_run` takes the task's execution guard with `try_lock`. It
   never blocks: a second runner gets `RunError::AlreadyRunning`, so a task can
   never execute on two workers at once.
2. `RunGuard::run_with(budget, host)` runs the dispatch loop
   (`src/vm/dispatch.rs`) on the task's `VmState`.
3. The run ends with one `RunOutcome`:

| Outcome          | Task afterwards | Meaning |
|------------------|-----------------|---------|
| `Complete(root)` | `Finished`      | returned a value, delivered as a persistent root |
| `Failed(error)`  | `Finished`      | runtime failure with executable/task/world/instruction diagnostics |
| `Yielded`        | `Ready`         | the instruction budget ran out; state saved in the task |
| `Waiting(key)`   | (reserved)      | not produced yet; see the continuation shape below |

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
- **When it runs.** A `RunGuard` is a registered mutator from acquisition to
  drop; a scope is a mutator for threads other than its own. `collect`
  refuses with `CollectError::MutatorActive`, without waiting, while either
  exists. During the pause it holds every heap lock, so runs, scopes,
  allocations and root changes on other threads wait briefly. No lock is held
  across bytecode execution; the mutator count only stops the collector from
  starting. Lock order: `gc` → `roots` → `scopes` → `sources` → task bodies →
  `objects`.
- **Ownership.** A task's allocations are not owned by the task: completion
  drops its frames, and what remains reachable (the returned `Rooted` and
  anything it references) survives. `Executable` code is independently owned
  immutable `Arc` storage outside the heap; its constants are primitives in
  this slice.
- **No unsafe code.** Objects are reached only under the `objects` lock;
  reclaimed objects are dropped after all heap locks are released.

Cooperative safepoints (collection that waits for running mutators to reach
poll points) are pellet clj-quickjs-17.

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
  | `ConcurrentReference` | reserved for atoms (clj-quickjs-10) and any heap-object handle to a dependency cell (cells are runtime-owned, not heap objects) | always; not descended into |

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
  builders stay protected by their owner check regardless. Later runtime
  handoffs (dependency results, atom contents, closure captures) call
  `share::check` themselves.
- **What later kinds declare.** A category. If `Persistent`: edges that
  never change after allocation (closures qualify only with an immutable
  captured environment) and allocation through `alloc_persistent`. If
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
  `VmErrorKind::ReferenceError` or `TypeError` only if that code runs. With no
  closures, `break` or `continue` in the subset, initialization is decided
  statically by source position; closures (clj-quickjs-20/21) will need a
  run-time check.
- **Operators.** New number-only opcodes (`div`, `rem`, `neg`, `gt`, `le`,
  `ge`) follow the existing `add`/`sub`/`mul`/`lt` rule. The compiler applies
  JavaScript ToNumber explicitly (`to_number`, `to_number_pair`) to operands
  that are not statically numbers, after both operands are evaluated; with no
  string values that is JavaScript's semantics. `loose_eq` is `==` for the
  supported value types; `!=`/`!==` are the equality opcode plus `not`;
  `&&`/`||`/`?:` are branches.
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

## Task/dependency continuation shape (planned)

Dependency waits reuse the same mechanism as `Yielded`: the task returns from
the run, and all state needed to continue stays in the task.

- A demanding instruction reads a dependency cell keyed by
  world/version + definition/query id + result kind. If the result is ready,
  the read is recorded as a dependency edge and execution continues in the
  same run.
- If it is unavailable, the instruction leaves the saved position on itself
  (it has no effect to replay), registers the task's identity as the wake
  handle, and the run returns `Waiting(key)`. No JavaScript callback, promise,
  or Rust stack represents the continuation.
- When the result is published, the host requeues the task (outside any cell
  or waiter lock) and calls `run_with` again, on any worker; the instruction
  re-reads the now-ready cell and continues.

Dependency cells and read-or-subscribe exist (clj-quickjs-6, below); the
suspending VM operation and race-safe parking are pellets clj-quickjs-23 and
-7; the reference executor is clj-quickjs-8.

## Dependency cells (clj-quickjs-6)

`src/dep.rs` (protocol, lock and ordering contract in its module docs),
`src/ids.rs` (identities). Design: `design.md` §§7–9.

- **Identities.** `DependencyKey` = `WorldId` (runtime-allocated:
  `Runtime::world`, `Runtime::new_world`; a task's world is fixed at
  `spawn_in`) + `DependencyId` (host-assigned definition/query) +
  `ResultKind` (host-assigned: discovery, callable, initializer, analysis,
  ...). `ProducerId` is runtime-allocated per claim. The consumer identity
  and wake handle is the `TaskId`.
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
  drained wake handles for the caller to enqueue and runs nothing inline.
  `Runtime::claim` lets a host produce before anyone needs;
  `Runtime::cell_status` is a snapshot.
- **No check-then-register window.** Check and registration are one
  critical section of the cell lock, the same lock completion takes, so a
  completion is ordered entirely before or after a need.
- **Locks.** Dependency table → heap `sources` (registering a new cell);
  cell lock and task log lock are leaves; none is held while rooting,
  checking publication, or returning. The collector takes cell locks while
  tracing root sources. Ordering is the mutexes' release/acquire; producer
  ids use a `Relaxed` counter.
- **Collection.** Each cell is a `RootSource` (category
  `DependencyOutcome`): a published value or error survives collection
  while the cell exists, after the producer drops its handle. Readers get a
  fresh `Rooted` of the same identity.
- **Deferred.** The VM instruction and `Waiting(key)` (clj-quickjs-23);
  the task state machine, parking and an atomic ready fast path
  (clj-quickjs-7); consumer/cell cancellation beyond a producer completing
  with `Cancelled`; deadlock reporting over unresolved waits; cell
  invalidation and removal.

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

No QuickJS source or encodings were copied. Adapted material keeps the notice
from `research/quickjs/LICENSE` (MIT, Copyright (c) 2017-2021 Fabrice Bellard
and Charlie Gordon) and is listed here with its source path and revision:

- `tests/fixtures/js/quickjs_op1_while.js` (clj-quickjs-22): the supported
  cases of `test_op1` in `research/quickjs/tests/test_language.js` and
  `test_while` in `research/quickjs/tests/test_loop.js`, revision 7d1c13c5d8f8.
  Modified: rewritten for the subset (`let` for `var`, `x = x + 1` for `++`,
  failure flags instead of `assert()` calls); the bitwise, `**` and string
  cases are omitted. The file carries the full notice.
