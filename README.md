# clj-quickjs

A Rust-native JavaScript runtime for highly concurrent compile-time execution.
Requirements: [`docs/design.md`](docs/design.md). Backlog: Pellets (`pl list`).

This is an architectural slice in progress, not a finished JavaScript engine.
A small documented JavaScript subset ([`docs/grammar.md`](docs/grammar.md))
compiles to VM bytecode and runs: straight-line code, conditionals, loops,
locals, functions, calls, recursion and closures in task-owned frames, and
private collection and host-effect intrinsics through the compiler-reserved
binding `__rt`. Strings and property access are not executable yet.

## Build and test

The toolchain comes from `flake.nix` (`nix develop` if `cargo` is not on PATH).

```sh
cargo build --all-targets
cargo test --all-targets          # unit + integration tests
cargo test --doc                  # doc tests, including compile-fail handle tests
cargo run                         # compile and run examples/demo.js (JavaScript source)
cargo run -- path/to/script.js    # compile and run any script of the subset
cargo run --example resumable_counter   # budgeted yield/resume of examples/resumable_counter.js
cargo test --lib resume_tests     # yield/resume and host-effect unit tests
cargo test --test resumable       # resume on another embedder thread
cargo test --test gc              # collection through the embedding API
cargo test --lib heap::           # collector, native payloads, root categories
cargo test --test collections     # persistent vectors/maps, identity, provenance
cargo test --lib collections::    # trie internals, structural sharing, key contract
cargo test --test collection_intrinsics   # collection intrinsics and transient builders
cargo test --lib vm::intrinsics:: # cross-task builder rejection
cargo test --test publication     # publication check: paths, identity, deep/shared graphs
cargo test --lib value::share::   # categories, known-shareable cache, cycles
cargo test --test dependency      # dependency cells: need, completion, worlds, GC, races
cargo test --lib dep::            # cell root category, rejected completion
cargo test --test js_source       # JavaScript source compiled and executed
cargo test --test js_functions    # functions, calls, closures, reserved intrinsics from source
cargo test --lib frontend::       # lexer, parser, grammar doc examples, lowering
cargo test --test wakeup           # parking and wakeups: many waiters, duplicate wakes, one owner
cargo test --lib task::           # task state machine and hook-forced handoff windows
cargo test --test sim_host        # simulated host: worlds, result kinds, producers
cargo test --lib dep::graph::     # recorded uses vs active waits, wait-cycle detection
cargo test --test atoms           # atoms: identity CAS, single-writer publication, load-to-root
cargo test --lib atom::           # SameValue comparison, replaced-box reclamation
cargo test --test cancellation    # consumer and cell cancellation racing settlement and parking
cargo test --lib task::cancel_tests # hook-forced cancellation in every handoff window
cargo test --test safepoints      # safepoints: collection requests during runs and scopes
cargo test --lib heap::gc::       # rendezvous root snapshot with running tasks
cargo test --test executor        # two-worker executor: overlap, migration, cancellation, shutdown
cargo test --lib exec::           # builder ownership across workers, executor root snapshot
cargo test --test executor_gc     # collection under the executor: workers, native scopes, parked roots
cargo test --test gc_roots        # root inventory: retention and reclamation per holder, multi-worker
cargo test --lib heap::inventory:: # root inventory cross-check, crate-internal holders
cargo test --test need            # __rt.need: suspend at the need, resume without replay
cargo test --lib task::need_tests # hook-forced handoff windows of the VM need
cargo test --test exceptions      # throw, try/catch/finally, completion records, yields inside handlers
cargo run --example e2e           # end-to-end compiler-host scenario on two workers (prints its trace)
cargo run --example e2e -- failure    # ... the producer task fails; both consumers catch it
cargo run --example e2e -- cancel     # ... the claiming consumer is cancelled while parked
cargo run --example e2e -- companion  # ... a fold callback waits inside try/finally with a live closure
cargo test --test e2e             # every e2e scenario, asserted
cargo test --test native_fold     # resumable natives: a fold whose callback waits, needAll, GC
cargo test --lib vm::native::     # leaf versus resumable natives, step accounting
cargo test --test exception_waits # waits inside handlers, catchable dependency failures, cancellation cleanup
cargo test --test concurrency     # bounded adversarial concurrency and cross-feature suite
cargo test --test concurrency seeded   # only the seeded repeated schedules
cargo run --release --example bench   # cost measurements (docs/measurement.md); -- --quick for a short run
cargo test --test measurement     # measurement workloads at small sizes; shared-input object counts
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Crate layout

Library (`src/lib.rs`) plus a thin binary (`src/main.rs`). Modules:
`runtime` (embedding entry point, `collect`), `task` (task state, execution
guard, run outcomes), `vm` (instruction format, executable validation,
task-owned frames, dispatch, resumable native continuations, test
assembler), `heap` (object identity, roots,
scoped and weak handles, tracing contract, mark/sweep collector), `value`,
`ids`, `error`, `host` (leaf host effects; `host::sim`, the simulated
compiler host), `collections` (persistent
vectors, maps and occurrences), `dep` (dependency keys, single-assignment
cells, read-or-subscribe, per-task dependency log), `atom` (atomic
references: single-writer publication, identity compare-and-set), `exec`
(optional reference executor with worker threads). See
the `lib.rs` crate docs and [`docs/architecture.md`](docs/architecture.md).

## Supported and deferred

### VM core (clj-quickjs-1)

Supported:

- Embedding API: `Runtime`, immutable shared `Executable`, `Task`,
  persistent `Rooted` handles, scoped `Local` handles (`Runtime::scope`), and
  `RunOutcome::{Complete, Failed}`. The embedder drives tasks with
  `Task::run` on its own threads; the runtime owns no threads.
- Execution guard: a task being run cannot be run again concurrently
  (`RunError::AlreadyRunning`, never blocks); finished tasks report
  `RunError::Finished`.
- A new byte instruction format, validated at load time (unknown opcodes,
  truncated operands, bad indices and branch targets, stack underflow and
  depth mismatches, falling off the end): constants, `undefined`, local
  get/set, `pop`, `dup`, number `add`/`sub`/`mul`/`lt`, `===`, `!`, `jump`,
  `jump_if_false`, `return`, and allocation of property-less objects. The
  opcode table is in `src/vm/instr.rs`.
- Task-owned VM state (instruction position, operands, locals, diagnostic
  context); failures report executable, task, world, instruction and opcode.
- Stable object identity across tasks and runs; task results are persistent
  roots that outlive the task.

Deferred (explicitly not implemented):

- JavaScript source: programs are hand-assembled bytecode (`Assembler`), an
  early VM test tool, not JavaScript source support.
- `Waiting` is reserved but never produced (`Yielded` and budgets: see
  clj-quickjs-16 below).
- Garbage collection: superseded, see clj-quickjs-2 below.
- Arithmetic on non-number operands fails with `UnsupportedOperand` instead of
  applying JavaScript coercions. No strings, properties, calls, closures,
  exceptions, collections, atoms or dependency cells yet.

### Budgeted yield and leaf host effects (clj-quickjs-16)

Supported:

- `Task::run_with(Budget, &mut dyn Host)`: a run executes at most
  `Budget::instructions(n)` instructions and returns `RunOutcome::Yielded` when
  the budget runs out. Instruction position, operands and locals stay in the
  task; the next run, on any thread, continues at the saved instruction
  (including a branch target). The budget is checked only between
  instructions, so no instruction or effect is ever replayed.
- `host_effect op` (opcode `0x11`): a non-reentrant leaf host effect through the
  `Host` trait. It takes and returns primitives only, cannot reenter the VM
  (`RunError::InsideHostEffect`), open a heap scope, or suspend. Host errors
  fail the task (`VmErrorKind::HostEffect`); `Task::run` supplies no host.
- Example: `cargo run --example resumable_counter` round-robins two tasks with
  a budget of 5 instructions and prints the host counter once per effect.
  Since clj-quickjs-21 its program is JavaScript source,
  `examples/resumable_counter.js` (`__rt.effect` lowers to `host_effect`).

Deferred:

- Budgets count instructions (and resumable native steps, see
  clj-quickjs-12 below) only, not time or allocation.

### Garbage collection (clj-quickjs-2)

Supported:

- A precise, nonmoving, stop-the-world mark/sweep collector over the one
  shared heap (`src/heap/gc.rs`). `Runtime::collect()` is the deterministic
  "collect now" entry point; it returns `CollectStats` (roots, live,
  reclaimed). Unreachable objects are reclaimed, cycles included; reachable
  objects keep their identity and storage address.
- Roots: persistent `Rooted` handles (including task results), locals of
  open `Runtime::scope` scopes, and every live task's saved frames (operands
  and locals) plus its executable's constants. Native heap objects report
  their payload edges through `Trace`. Later root kinds (dependency results,
  atoms, closures, native continuations) register through the same
  `RootSource` boundary.
- A Rust variable is not a root: `WeakHandle` (`Rooted::downgrade`,
  `Scope::downgrade`) names an object without keeping it alive. After
  reclamation `upgrade` fails with `HandleError::Reclaimed`, even when the
  object's storage slot has been reused (slot generations are checked on
  every lookup).
- Finishing a task releases its frames but not its allocations: a returned
  value lives as long as its `Rooted` result.

Deferred:

- `collect` never waits: it refuses with `CollectError::MutatorActive`
  while another thread is in heap access and not at a safepoint. Waiting
  collection while tasks run is `Runtime::request_collection`
  (clj-quickjs-17, below), integrated with the executor in clj-quickjs-25;
  per-root-kind coverage is clj-quickjs-31.
- No automatic collection trigger (allocation never collects), no
  generational, concurrent or compacting collection, and no worker-local
  allocation buffers.
- Heap-object constants in executables: the constant-pool edge is traced,
  but loaded code still accepts only primitive constants.

### Persistent collections and occurrences (clj-quickjs-3)

Supported:

- Persistent vectors (32-way radix trie) and maps (hash array mapped trie)
  as heap objects, plus syntax occurrences (a value with a `Provenance`).
  Embedder API on `Scope`: `new_vector`, `new_map`, `new_occurrence`,
  `count`, `nth`, `get`, `assoc`, `dissoc`, `conj`, `pop`, `items`,
  `entries`, `occurrence_value`, `provenance`, `persistent_kind`,
  `value_equals`. The full contract (index and key rules, errors, equality)
  is in the `collections` module docs (`src/collections.rs`) and
  [`docs/architecture.md`](docs/architecture.md).
- Updates never change their input and copy only the root-to-change path;
  untouched trie nodes and every unreplaced element (including nested
  collections) are shared by identity, before and after collection. No-op
  updates return the original object.
- Map keys compare by SameValueZero (objects, including collections, by
  identity) with a private hash: no user code runs during lookups.
- Value equality (structural, provenance-blind), identity (`===`,
  `same_object`) and provenance are separate; equal scalars at different
  source locations are distinct occurrence objects.
- Collections pass to tasks as `Rooted` arguments and come back as results
  with the same identity (`cargo test --test collections`).

Deferred:

- VM intrinsics for collections and transient builders: superseded, see
  clj-quickjs-19 below.
- Publication rules for collections containing mutable objects: superseded,
  see clj-quickjs-18 below.
- Strings/keywords/symbols, structural map keys, ordered maps, sets, lists,
  and JavaScript property access on collections.

### Collection intrinsics and transient builders (clj-quickjs-19)

Supported:

- Private VM intrinsics (opcodes `0x12`–`0x20`, not user-visible globals):
  `new_vector n`, `new_map n`, `count`, `nth`, `get` (absent entry is an
  error), `get_or` (absent entry gives a default), `assoc`, `dissoc`,
  `conj`, `pop_last`. Contract and per-intrinsic errors: `src/vm/intrinsics.rs`
  module docs; summary in [`docs/architecture.md`](docs/architecture.md).
- Operand errors (wrong type, invalid or out-of-bounds index, missing key)
  fail the task with `VmErrorKind::Collection(CollectionError)` and
  diagnostics naming the intrinsic; nothing panics.
- Task-owned transient builders: `new_vector_builder`, `new_map_builder`,
  `builder_conj`, `builder_assoc` (in place) and `freeze` (a new persistent
  vector/map; the builder becomes frozen). Ownership is by task id, so a
  builder keeps working after a yield and on another OS thread. Use by
  another task, after freeze, or after the owner finished fails with
  `VmErrorKind::Builder(BuilderError::{NotOwner, Frozen, OwnerFinished})`.
  Open builders' contents are traced by the collector; a finishing task
  releases its open builders. Lifecycle: `src/collections/builder.rs`.
- Tests: `cargo test --test collection_intrinsics` (hand-assembled bytecode:
  early VM tests, not JavaScript source support) and
  `cargo test --lib vm::intrinsics::`.

Deferred:

- Embedder (`Scope`) access to builders; builders seeded from an existing
  persistent value (`transient`); in-place map trie editing (map builders
  path-copy internally); `count`/lookups on builders.
- Releasing builders of a task dropped without finishing (they remain
  unusable by other tasks). Rejecting builders at publication: see
  clj-quickjs-18 below.

### Publication rules (clj-quickjs-18)

Supported:

- Three value categories (`design.md` §4), declared by every heap object
  kind: persistent (vectors, maps, occurrences: shareable when their
  contents are), task-owned mutation (transient builders in any state,
  plain objects: never shareable), and an explicit-concurrent-reference
  category for atoms (clj-quickjs-10) and any future heap handle to a
  dependency cell (shareable without being immutable). Primitives are
  shareable.
- `Runtime::publish(&Rooted)`: the embedder's check before handing a value
  to another task. It walks the graph iteratively (deep, cyclic and heavily
  shared graphs are fine) and fails with `PublishError::NotShareable`,
  naming the path to the offending object (e.g. `value[1]{7}[1]`), its
  kind and its owning task. Published values are not copied: receiving
  tasks see the same identities.
- Values built from shareable parts are known shareable from allocation
  (a sticky per-object bit), and a successful check caches its result, so
  republishing or publishing a derived version does not re-walk the graph.
- Contract: `src/value/share.rs` module docs; summary in
  [`docs/architecture.md`](docs/architecture.md). Tests:
  `cargo test --test publication` and `cargo test --lib value::share::`.

Deferred:

- `Runtime::spawn` does not check its arguments; embedders call `publish`
  when a value moves between tasks.
- No VM intrinsic for the check; dependency results (clj-quickjs-6) and atom
  contents (clj-quickjs-10) call it at their handoffs. Closure captures
  (clj-quickjs-20) need no handoff of their own: they are a function
  object's edges, which the walk follows.

### Dependency cells (clj-quickjs-6)

Supported:

- Typed identities: `DependencyKey` = world (`Runtime::world`,
  `Runtime::new_world`; each task runs in one world, `Runtime::spawn_in`) +
  host-assigned `DependencyId` + host-assigned `ResultKind`. Different
  worlds or result kinds are different cells. `ProducerId` per claim; the
  consumer is the `TaskId` (wake handles: clj-quickjs-7 below).
- Single-assignment cells with immutable outcomes `Ready(value)`,
  `Failed(error)`, `Cancelled`. `Runtime::complete(key, producer, ..)`
  accepts only the claimed producer, only once (`NotProducer`,
  `AlreadyComplete`); values and errors must pass the publication check
  (`NotShareable`). It returns the drained wakes for the caller to
  enqueue; nothing runs inline.
- Read-or-subscribe: `RunGuard::need(definition, kind)` returns the shared
  value, error or cancellation, or registers the task and returns
  `Need::Waiting` carrying the key's single `ProducerRequest` for the first
  asker (`Runtime::claim` for hosts that produce eagerly). Check and
  registration share one cell critical section: no lost-wakeup window.
- Per-task `DependencyLog` (`Task::dependency_log`): recorded edges (ready
  reads included) kept apart from the unresolved wait.
- Published results and errors are collector roots while their cell
  exists. Contract: `src/dep.rs` module docs; summary in
  [`docs/architecture.md`](docs/architecture.md).

Deferred:

- Deadlock reporting; invalidation or removal of cells (they live as long
  as the runtime). Cancellation: see clj-quickjs-24 below.

### JavaScript source compilation (clj-quickjs-22)

Supported:

- `frontend::compile_script(name, source)` compiles a script of the
  documented subset to a validated `Executable`; a task running it completes
  with the script's completion value. `frontend::compile_function(source)`
  compiles one function declaration as a task entry point with parameters
  and `return`. Semantics and errors: "Execution" in
  [`docs/grammar.md`](docs/grammar.md); design: "Source compilation" in
  [`docs/architecture.md`](docs/architecture.md).
- Executed forms: number/boolean/`null`/`undefined`/`NaN`/`Infinity`
  literals, `let`/`const` with block scoping and shadowing, assignment,
  `+ - * / %`, unary `- + !`, `< > <= >=`, `== != === !==`, `&& || ?:`,
  `if`/`else`, `while`, blocks. Number semantics are IEEE doubles (NaN,
  Infinity, `-0`, rounding past 2^53); ToNumber, ToBoolean, strict and loose
  equality follow JavaScript for the supported primitive types.
- Binding resolution: redeclaration is a compile-time `SyntaxError`
  (`Invalid`); unknown names and use before initialization fail the task
  with `VmErrorKind::ReferenceError`, and assignment to a `const` with
  `VmErrorKind::TypeError`, only when that code runs.
- New opcodes `0x21`–`0x2A` (`div`, `rem`, `neg`, `to_number`,
  `to_number_pair`, `gt`, `le`, `ge`, `loose_eq`, `throw_error`), validated
  like every other opcode. Compiled code goes through the same validator as
  hand-built specs; `Executable::to_spec` re-encodes code, and corrupted
  compiled bytecode (bad branch target, operand out of range, stack
  underflow or depth mismatch) is rejected at load.
- Source positions: every compiled instruction has a `SourceLocation`;
  failures report it in `Diagnostic::source`, and `Task::source_location`
  gives the position a ready (for example yielded) task continues at.
- Fixtures: `tests/fixtures/js/` (synthetic, not light-cljs output, plus
  adapted QuickJS `test_op1`/`test_while` cases with their license notice).

Deferred:

- String values and property access: they parse but are rejected by the
  compiler as `Unsupported`. (Functions, calls, closures and the reserved
  intrinsic binding compile since clj-quickjs-21, below.)
- (`throw`/`try`/`finally`: clj-quickjs-11, below.) Operators on objects need ToPrimitive and fail with
  `UnsupportedOperand` (objects cannot come from source yet).
- labels, `switch`, `var`, update and compound
  assignment operators (rejected by the parser). (Unlabelled `break` and
  `continue`: clj-quickjs-78; `do`-`while`: clj-quickjs-80; `for`:
  clj-quickjs-81, below.)

### Parking and wakeups (clj-quickjs-7)

Supported:

- A per-task state machine on one atomic word (`Runnable`, `Executing`,
  `Parked`, `Terminal`, plus a `NOTIFIED` bit and an execution number).
  `Task::try_begin_run` is a non-blocking claim (CAS); a parked task is
  refused with `RunError::Waiting`; `Task::status` adds
  `TaskStatus::Waiting`. Transition table, linearization points and
  orderings: `src/task.rs` module docs.
- `RunGuard::park()` ends a run whose need found the cell pending:
  `RunOutcome::Waiting(key)` if it parked, or `RunOutcome::Yielded` (task
  runnable, requeue it) if the result was published after the subscription
  but before the park.
- `WakeHandle` (runtime + task id + subscribing run): stale and foreign
  handles are rejected, so duplicate or late wakes are harmless (`Task::wake` returns `Wake::Stale`).
- `Runtime::complete` returns `Wakeups`: it delivers every drained handle
  after releasing the cell lock (one CAS each, nothing inline) and
  `into_runnable()` gives the parked tasks to enqueue, each exactly once
  per suspension. Producer failure and cancellation wake like success.
- An atomic completed-result fast path (`OnceLock`) with the per-cell lock
  only for pending bookkeeping, rechecked before registration. Lock order
  and orderings: `src/dep.rs` module docs.
- Tests force each handoff window deterministically with test hooks
  (`cargo test --lib task::`) and race many waiters, duplicate wakers and
  competing claimers with barriers (`cargo test --test wakeup`). No sleeps.

Deferred:

- A reference executor with a ready queue (clj-quickjs-8). Cancellation:
  clj-quickjs-24 below. The VM `need` instruction (clj-quickjs-23, below)
  parks through the same path.

### Simulated compiler host (clj-quickjs-9)

Supported:

- `host::sim::SimHost`: immutable worlds (`WorldSpec` frozen by
  `add_world` under a fresh `WorldId`) of synthetic definitions, each with
  separate results `BINDING` (discovery), `CALLABLE`, `INITIALIZED` and
  `ANALYSIS`: one dependency cell per world, definition and kind. A world-2
  result never satisfies world 1; completed results of an old world stay
  stable when newer worlds are added.
- Producers: Rust work (run once, after its declared needs are ready), a
  VM callable compiled from JavaScript by this runtime, or an initializer
  that calls a callable. One producer per key, discovered through the
  cell's single producer request; completions wake parked jobs by task id.
  `need` and `invoke` drive a deterministic single-thread job queue.
- Recursion: mutually recursive callables (`tests/fixtures/js/sim_is_even.js`,
  `sim_is_odd.js`, synthetic) need only each other's binding, so they run
  without any initializer being needed and without a wait.
- Failures settle cells deterministically: `UnknownWorld`,
  `MissingDefinition`, `MissingResult`, `ProducerFailed` (error or
  unshareable result), `InputFailed`/`InputCancelled`, `WaitCycle`
  (clj-quickjs-26); `Stalled` when a request is left waiting on a cell no
  host job produces. Events (`Scheduled`,
  `Parked`, `Woken`, `Activation`, `Settled`) and per-producer dependency
  logs are recorded. Contract: `src/host/sim.rs` module docs; summary in
  [`docs/architecture.md`](docs/architecture.md).

Deferred:

- Real calls: a callable is a host token and calls between callables use a
  stand-in tail-call convention (a returned number calls the declared tail
  callee) until VM calls, function values and `__rt.need` exist
  (clj-quickjs-4, -20, -21, -23).
- Worker threads (clj-quickjs-8), cancelling a stalled request, and
  invalidation.

### Calls and recursion in task-owned frames (clj-quickjs-4)

Supported (VM level; early VM tests use hand-assembled bytecode,
`tests/calls.rs`):

- Function objects over shared immutable code: `ExecutableSpec::functions`
  holds child `Executable`s by `Arc` (never copied per task or call;
  `Executable::function(i)`), `function i` (`0x2B`) allocates a new function
  object for child `i`, and `current_function` (`0x2D`) pushes the running
  frame's own function object (a named function expression's self-binding).
- `call n` (`0x2C`) runs the callee in a new frame on the task's explicit
  frame stack; `return` pops it and hands the result to the caller. The
  interpreter never recurses, so nesting depth costs no Rust stack: nested
  calls, recursion and mutual recursion (through function values), budgeted
  yields and collections with any number of live frames, and resumption on
  another thread all work without replaying effects.
- Arguments: missing ones are `undefined`, extra ones are ignored, other
  locals start `undefined`. Calling anything but a function object fails the
  task with `VmErrorKind::TypeError` ("`undefined is not a function`", "`vector
  is not a function`", ...).
- Resource policy: `Runtime::with_limits(VmLimits { max_call_depth,
  max_stack_values })` (defaults 10,000 frames and 2^20 values). A call
  beyond either bound fails the task with `VmErrorKind::StackOverflow`.
- Diagnostics: a failure names the failing frame's function and instruction,
  plus `Diagnostic::call_chain`, the callers' call sites (innermost first).
  All frames run in their task's world.
- Function objects compare by identity, are traced through the frames and
  operands that hold them, and (having no properties) pass the publication
  check when they capture nothing task-owned, so a function returned by one
  task can be called by another.

Deferred:

- (Compiling functions and calls from source: clj-quickjs-21, below.)
- `this`, `arguments`, `new`, rest/default parameters, function properties
  (`length`, `name`), and native or host-function callees.
  (Catching `StackOverflow` and `TypeError`: clj-quickjs-11, below.)

### Lexical closures and captured bindings (clj-quickjs-20)

Supported (VM level; early VM tests use hand-assembled bytecode,
`cargo test --test closures`):

- Closures: `ExecutableSpec::captures` declares how many captured values a
  closure over that code carries; `function i` (`0x2B`) pops them and stores
  them in the new function object (its lexical environment), and
  `get_capture i` (`0x2E`, checked against the declaration at load time)
  reads one in the running closure. A capture is either an immutable value
  (captured by value) or a binding cell.
- Binding cells: `new_cell` (`0x2F`), `cell_init` (`0x30`), `cell_get`
  (`0x31`), `cell_set` (`0x32`). A mutable captured binding lives in one cell
  shared by the creating frame and every closure that captures it, so
  closures observe each other's writes and the binding outlives its frame;
  each executed `new_cell` is a fresh binding (per activation, per loop
  iteration). Reading or assigning an uninitialized cell is a
  `ReferenceError` (the run-time temporal-dead-zone check).
- Escaping closures, closures sharing one binding, closures created inside
  recursion, and closure/environment cycles survive yields and collections
  while reachable and are reclaimed once they are not.
- Publication: a function object is persistent and shareable exactly when
  its captured values are, so a closure over immutable values can be
  published and called by another task, while one capturing a binding cell
  is rejected (`value[0].capture(0) is a task-owned binding cell (owned by
  task#1)`). Cells are task-owned: using another task's cell fails with
  `VmErrorKind::ForeignBinding`, even if the closure was passed without the
  check.

Deferred:

- (Compiling closures from source: clj-quickjs-21, below.)
- A captured `const` that a closure may read before its initialization must
  live in a cell, which makes the closure task-owned; there is no
  initialize-once immutable cell.
- Spawning a task directly on closure code (no environment) fails at its
  first `get_capture` with `VmErrorKind::Internal`.

### Atoms (clj-quickjs-10)

Supported (Rust API and VM intrinsics; VM tests use hand-assembled bytecode,
`tests/atoms.rs`):

- Atoms are heap objects whose handle is an ordinary value (stable
  `ObjectId`, never reissued), held as `Rooted`/`Local`, in task frames, or
  inside persistent collections. They are explicit concurrent references:
  publishable without being immutable.
- Two writer contracts, kept apart from dependency completion:
  multi-writer compare-and-set (`Runtime::new_atom`,
  `Runtime::atom_compare_and_set`, VM `atom_new` `0x33` / `atom_cas` `0x35`)
  and single-writer publication (`Runtime::new_single_writer_atom` returns
  the atom's one `Publisher`; `Publisher::publish` stores any number of
  times; CAS on such an atom fails with `AtomError::SingleWriter`).
- CAS compares by identity for objects and SameValue for primitives (`NaN`
  matches `NaN`, `+0` does not match `-0`), never by structure or user
  code. A conflict changes nothing and reports the current value
  (`CasOutcome::Conflict { current }`; VM: `w ok` with `ok = false`); the
  runtime never retries with user code, so effects computed before the CAS
  happen once.
- Every installing operation (creation, CAS, publish, VM `atom_new`/
  `atom_cas`) runs the publication check: task-owned mutation is rejected
  with `AtomError::NotShareable` / `VmErrorKind::Atom(NotShareable)`.
- Load-to-root: `Runtime::atom_load` (a new `Rooted`), `Scope::atom_load`
  (a `Local`), VM `atom_load` `0x34` (an operand). The loaded value and its
  graph stay valid until released, whatever replaces the atom and however
  often the heap is collected; then it is reclaimable.
- Rust atomics only: an `AtomicPtr` to an immutable boxed value per atom,
  installed with `AcqRel` and read with `Acquire`. Replaced boxes are freed
  after the next collection (a grace period: atom operations are registered
  mutators). Contract, ordering, ABA and the `unsafe` invariants: `src/atom.rs`
  module docs; summary in [`docs/architecture.md`](docs/architecture.md).

Deferred:

- `swap`-with-function, `reset`, add-watch and validators; atoms from
  JavaScript source (the frontend does not lower to the atom intrinsics);
  publishing to a single-writer atom from the VM (the `Publisher` is a Rust
  capability).
- Memory held by replaced values until the next collection is bounded by one
  small box per write; there is no reclamation without a collection.

### Cancellation (clj-quickjs-24)

Supported:

- Consumer cancellation: `Task::cancel()` sets a sticky `CANCELLED` bit in
  the task's state word (one CAS, no lock, nothing inline) and returns
  `Cancel::Runnable(task)` for a parked task (enqueue it), else
  `Requested`, `AlreadyRequested` or `Finished`. It never cancels a cell or
  its producer: other consumers still get the result. From then on every
  wake is `Wake::Stale` and `RunGuard::need` refuses with
  `NeedError::Cancelled`, so a stale wake cannot resurrect the task.
- Observation by the task's own run, never inline: at run start, at
  `need`, at `park` (ahead of a remembered wake), and at the budget
  boundary; `RunGuard::cancel_requested()` polls. The run then performs
  terminal cleanup (removes its waiter entry, resolves its unresolved wait,
  releases its builders and VM state so their roots are reclaimable) and
  returns `RunOutcome::Cancelled`; `Task::status()` is
  `TaskStatus::Cancelled`. A run that returns or fails first keeps its
  outcome.
- Cell cancellation is a terminal outcome, separate from consumer
  cancellation: installed by the claimed producer
  (`Runtime::complete(.., Completion::Cancelled)`) or by the embedder
  without a producer id (`Runtime::cancel_cell(key)`); tasks cannot cancel
  cells. Waiters are woken exactly once like any completion and read
  `Need::Cancelled`; later completions, claims and cancellations are
  `AlreadyComplete`.
- Contract: "Consumer cancellation" in `src/task.rs` and "Cancellation" in
  `src/dep.rs` module docs. Tests force each handoff window with hooks
  (`cargo test --lib task::cancel_tests`) and race cancellation against
  parking and settlement with barriers (`cargo test --test cancellation`;
  consumers run the synthetic source fixture
  `tests/fixtures/js/cancel_consumer.js`).

Deferred:

- Polling between VM instructions (a run with an unlimited budget and no
  need observes a cancellation only if it would yield; `finally` cleanup
  on cancellation is clj-quickjs-27 and inside native continuations
  clj-quickjs-28, below); cancelling a producer
  automatically when its last consumer is cancelled (not done by design:
  the embedder decides, with `Runtime::cancel_cell`).

### Functions, closures and reserved intrinsics from source (clj-quickjs-21)

Supported (`cargo test --test js_functions`; fixtures in `tests/fixtures/js/`
and `examples/`):

- Function declarations (hoisted per statement list), function expressions
  (named and anonymous), calls (`call n`), parameters, `return`, nested
  calls, direct and mutual recursion, higher-order functions and escaping
  closures, all compiled from JavaScript source to the VM's call and closure
  instructions (`function`, `call`, `current_function`, `get_capture`,
  `new_cell`/`cell_init`/`cell_get`/`cell_set`). Semantics and errors:
  "Execution" in [`docs/grammar.md`](docs/grammar.md); design: "Functions,
  closures and intrinsics from source" in
  [`docs/architecture.md`](docs/architecture.md).
- Closures see later assignments, share bindings with each other, keep them
  alive after the frame returns, and get a fresh binding per loop iteration.
  The temporal dead zone is checked statically inside a function and at run
  time (binding cells) for closures that may run before a declaration.
  Never-assigned bindings initialized before capture are copied, so such
  closures stay publishable to other tasks; closures over cells are
  task-owned.
- A function's own name inside it is `current_function` (named function
  expressions, `compile_function` entries, never-reassigned declarations).
- The compiler-reserved intrinsic binding `__rt` with a specified contract
  (no shadowing, reassignment, computed access or aliasing; explicit
  `ReservedWord`/`Invalid` diagnostics): `__rt.vector`, `map`, `count`,
  `nth`, `get`, `getOr`, `assoc`, `dissoc`, `conj`, `popLast`,
  `vectorBuilder`, `mapBuilder`, `builderConj`, `builderAssoc`, `freeze`
  (collection intrinsics) and `__rt.effect(op, x)` (`host_effect`). See
  "Reserved intrinsic binding" in [`docs/grammar.md`](docs/grammar.md).
- Demo: `cargo run` compiles and runs `examples/demo.js` (recursion, nested
  calls, an escaping closure over a builder, intrinsics, one effect;
  completes with 89); `cargo run -- file.js` runs any script. No qjsc or C
  engine is used.
- Adapted QuickJS `test_closure.js` cases run from source
  (`tests/fixtures/js/quickjs_closure.js`, with its license notice).

Deferred:

- Strings, property access (`f.call`, methods, objects), `this`,
  `arguments`, `new`, default/rest parameters, generators. (Arrow
  functions: clj-quickjs-53 below.)
- A function declaration repeating a parameter or another declaration at
  the top level of a script or function body (var-scoped in JavaScript) is
  rejected as `Unsupported`.
- A hoisted declaration capturing a `const` of its own scope uses a cell
  (no initialize-once immutable cell), which makes it task-owned.
- `src/host/sim.rs` still uses its stand-in tail-call convention for the
  simulated producers (`tests/fixtures/js/sim_is_even.js`); switching it to
  compiled calls belongs to the end-to-end pellets.

### Safepoints and mutator registration (clj-quickjs-17)

Supported (`src/heap/gc.rs` module docs; tests in `tests/safepoints.rs`,
early VM tests with hand-assembled bytecode):

- Every heap access is a per-thread mutator registration: a `RunGuard`, an
  open `Runtime::scope` scope, or an atom operation. A thread with none
  never delays a collection.
- `Runtime::request_collection()` requests a collection and blocks until
  every other thread is outside heap access or stopped at a safepoint, then
  collects: the deterministic collection hook. `Runtime::collection_requested()`
  reports a pending request. Threads entering heap access wait for a
  pending request to be served; nested registrations never wait.
- VM poll points: run entry, backward branches, `Call`, `Return`, and every
  instruction that may allocate. A run stopped there reports its frames as
  roots (`TaskFrame`) and continues after the collection.
- A scope is the no-GC access scope for host code: it delays a requested
  collection until it closes or is released. `Scope::released(f)` is a
  safepoint (the closure must be `Send`, so it cannot use any scope);
  `Scope::safepoint()` releases only when a collection is requested. Scope
  locals stay roots and valid across a release.
- No global interpreter lock and no heap lock held during execution: the
  registry only decides when a collection may start.

Deferred:

- An automatic collection trigger (allocation never requests one),
  worker-local allocation buffers, and releasing heap access around a
  long-running leaf host effect (a request waits for the effect to return).
- A thread holding two run guards at once, or a guard it is not executing,
  delays collection until it drops them (only a thread's single run parks).

### Reference executor (clj-quickjs-8)

Supported:

- `exec::Executor` (optional example/host code, not part of the embedding
  contract): N worker OS threads (default 2) driving the public run API
  with one FIFO ready queue, per-run instruction budgets (default 10 000),
  one leaf-effect `Host` per worker, and a driver that receives the run
  guard (default: run the VM). The runtime never starts it; the same tasks
  can be driven by hand (`tests/executor.rs`,
  `manual_embedding_needs_no_executor`).
- Ownership per run: a worker acquires the task's execution claim before
  dispatch and the claim is returned on yield, park or a terminal outcome,
  so a task never runs on two workers. No lock is held while a task runs
  or suspends; two workers execute instructions at the same time (proved
  by a rendezvous inside host effects). The executor never dispatches a
  task again before the previous dispatch has recorded its release; a
  refused claim is requeued if the task is still runnable (`Retried`) or
  kept if parked. An executor serves the runtime it was started for
  (`ExecutorConfig::start(&runtime)`); tasks of another runtime are handed
  back (`ForeignTasks`) by `submit`, `wake` and `cancel`.
- Waiting tasks occupy no worker: a parked task is kept by the executor
  and re-enqueued by `Executor::wake(wakeups)` (from
  `Runtime::complete`), then runs on any eligible worker, continuing at its
  saved instruction without replaying effects. A placement hook
  (`ExecutorConfig::placement`) chooses the worker per enqueue (reason and
  last worker given), e.g. a different worker after a wait. Transient
  builders follow their task across workers; other tasks are rejected.
- Event record (`Executor::events`, `wait_for`): worker start (OS thread
  id) and stop, enqueue (reason, placement), dispatch (execution number),
  release (how the run ended; `Completed` = useful work), refused claims.
- Cancellation (`Executor::cancel`: a parked task is enqueued so its own
  run cleans up) and bounded shutdown (`begin_shutdown`, `shutdown(timeout)`
  returns the queued and parked tasks to the embedder, or
  `ShutdownTimeout` naming busy workers and handing the executor back).
- Tests: `cargo test --test executor`, `cargo test --lib exec::`.

Deferred:

- A blocking leaf host effect delays a collection request until it
  returns (see clj-quickjs-25 below).
- Producer requests claimed by VM `need` instructions (clj-quickjs-23) are
  kept in the task (`Task::take_producer_requests`); the executor does not
  collect them, so its drivers still issue needs through the run guard and
  park. Work stealing, priorities, fairness
  beyond FIFO, and bounding the event record are not implemented.

### Dependency waits in the VM (clj-quickjs-23)

Supported:

- `__rt.need(definition, kind)` compiles to the VM `need` instruction. It
  records the dependency (ready reads included) in the task's own world and
  evaluates to the shared result: a ready result continues in the same run;
  a pending one ends the run `Waiting(key)` through the same park path as
  `RunGuard::need`, with every frame, operand, local and closure kept in the
  task. The next run (on any thread) completes the suspended `need` and
  continues after it: no effect before it is replayed and no function is
  restarted.
- A producer request claimed by a VM need is kept for the embedder:
  `Task::take_producer_requests` after the run.
- A failed or cancelled cell raises a catchable error at the `need`
  (`VmErrorKind::DependencyFailed` / `DependencyCancelled`, with its source
  position; clj-quickjs-27, below); a cancelled consumer ends `Cancelled`.
- A producer task may publish a VM function compiled by this runtime as a
  cell result; consumers call it after the need completes
  (`tests/fixtures/js/need_producer.js`).
- `tests/need.rs` drives the protocol by hand (no executor, no promises, no
  inline dependent execution, resume on another thread).

Deferred:

- Definition ids and result kinds are numbers (no string values yet).
- `src/host/sim.rs` jobs still need through `RunGuard::need` in Rust and
  publish callables as host tokens.

### Dependency graph and wait cycles (clj-quickjs-26)

Supported (`src/dep/graph.rs` module docs; tests in `tests/sim_host.rs`,
`src/dep/graph.rs`):

- `dep::DependencyGraph` keeps recorded uses (every need, ready reads
  included; never removed) apart from active waits (at most one per task,
  removed when the key resolves, when the waiter is woken or cancelled, or
  when the wait is refused). Producer bindings (key → task running its
  claimed producer) connect them. Maps are keyed by private numeric IDs.
- A new wait that would close a chain of active waits back to its own task
  is a genuine wait cycle: it is refused and reported as a `WaitCycle`
  (each step's waiting task, `DemandSite`, awaited world/definition/kind,
  producer and producer task; readable `Display`).
- Policy: the newest demand fails. `SimHost` abandons the refused
  subscription (`RunGuard::abandon_wait`), records `SimEvent::WaitCycle`
  (`SimHost::cycles`) and fails the job with `SimError::WaitCycle`; the
  failure propagates around the cycle as `InputFailed`, so every job and
  request on it settles instead of stalling. Recursion through bindings
  (recorded uses, no wait) is never reported.

Deferred:

- Demand sites from VM source locations (the `__rt.need` operation,
  clj-quickjs-23) and cycle detection in `exec::Executor`, which has no
  producer bindings (the graph would sit under one leaf lock; ordering
  rules are in the module docs).

### Collection under the executor (clj-quickjs-25)

Supported (`src/exec.rs` module docs, "Collection"; tests in
`tests/executor_gc.rs` and `cargo test --lib exec::`, early VM tests with
hand-assembled bytecode):

- Every executor worker registers as a collection participant for its
  lifetime (`Runtime::register_thread`). Registration is not heap access:
  a worker waiting on the ready queue is idle and never delays a
  collection; it enters heap access only through a run guard, or a scope
  of a native job.
- Native jobs (`Executor::submit_native`) run host code on a worker,
  outside any run, in queue order with tasks. A scope held there is a no-GC
  access scope: it delays a collection until it closes or is released.
- `Executor::schedule_collection(placement)` has a worker request a
  collection; any thread may still call `Runtime::request_collection`.
  Running tasks stop at VM safepoints with their frames reported.
- Roots: queued, parked (held only by the executor) and in-flight tasks
  are traced through their saved frames, running tasks through the frames
  their run reported, completed cells through their results. The collector
  takes no executor lock; workers never hold the scheduling or record lock
  while they enter heap access or request a collection.
- Collection events: `Runtime::observe_collections` gives every
  `CollectionReport` (requester, each other registered or accessing thread
  with its `MutatorState`: `Safepoint`, `Released`, `Idle`, `Requesting`;
  stats) to observers; the executor records it as `EventKind::Collected`
  with workers named. `Event` and `EventKind` are now `Clone`, not `Copy`.

Deferred:

- An automatic collection trigger (no allocation threshold requests one),
  worker-local allocation buffers, and releasing a run's heap access
  around a long leaf host effect: a host effect that blocks still delays
  a request until it returns.

### Exceptions and finally blocks (clj-quickjs-11)

Supported:

- `throw expr;` and `try`/`catch (e)`/`catch`/`finally` from source, with
  JavaScript semantics: a thrown value is caught by the innermost handler
  in the same or any calling function; `return` runs the frame's finally
  blocks innermost first; an abrupt finally completion (`return`/`throw`)
  replaces the pending one, a normal one keeps it; catch blocks rethrow.
  Script completion values follow `try` statements.
- Task-owned handler state: the VM keeps catch and finally handlers and the
  *pending completion* of a running finally block (normal, saved return
  value, saved exception with its diagnostic, or a `break`/`continue` jump,
  clj-quickjs-79) in the task's `VmState`,
  next to its frames (`src/vm/exception.rs`). Raising unwinds frames in the
  interpreter loop, never on the Rust stack. A task yields (and is
  collected) inside try, catch and finally blocks and resumes with its
  handlers intact; a finally block never runs because a task yields.
  Saved values are traced as roots.
- Opcodes `0x37`–`0x3C`: `throw`, `push_catch`, `push_finally`,
  `pop_catch`, `pop_finally`, `end_finally`, validated like branches (a
  catch target starts one value deeper). `return` consults the handler
  stack.
- ReferenceError, TypeError and StackOverflow failures are catchable: the
  caught value is a fresh immutable error object; rethrowing it reports the
  original error. An uncaught `throw` of any other value fails the task with
  `VmErrorKind::Thrown` ("uncaught exception: 42") at the throw, with its
  source position and call chain, also after leaving through finally
  blocks.
- Adapted QuickJS `test_try_catch1`–`7` run from source (`5` since
  clj-quickjs-79)
  (`tests/fixtures/js/quickjs_try_catch.js`, with its license notice).

Deferred:

- Other failures (unsupported operands, collection, builder, atom and
  binding contract violations, host errors, out of memory) are not
  catchable and skip catch and finally blocks. Failed or cancelled
  dependencies became catchable, and cancellation runs finally blocks, in
  clj-quickjs-27 (below).
- Error objects have no properties (`message`, `name`), no `Error`
  constructors or `instanceof`; strings cannot be thrown (no string values).

### End-to-end compiler-host scenario (clj-quickjs-13)

`cargo run --example e2e` runs the success scenario of `docs/design.md` §13
and prints the host's trace and the executor's event record;
`cargo test --test e2e` runs the same code (`examples/e2e/scenario.rs`) and
asserts it. The JavaScript is the synthetic fixtures `examples/e2e/*.js`
(not light-cljs output), compiled and run by this crate; no external
compiler or C engine is used. Steps: Rust builds and roots one nested
persistent form; two consumer tasks receive that same root on two pinned
workers, run their pre-wait JavaScript and suspend at
`__rt.need(7, 2)` (a callable that does not exist yet), inside a
`try`/`catch`/`finally`; each worker then
runs an unrelated task; a worker forces a collection; only then a producer
task runs `producer.js`, whose result (a function compiled by this
runtime's frontend) the host publishes once; both consumers resume on the
other worker, run their finally block once (not before the wait), call
the helper and return subtrees of the input.

What the assertions prove (`tests/e2e.rs`), with §13's numbering:

1. Parallel execution on two OS workers: both consumers are inside a host
   effect at the same time (a bounded two-party rendezvous), on two
   distinct worker threads; their first runs overlap in the event record.
2. Shared persistent data: both consumers read the one Rust-built root;
   each returns a subtree that is the *same object* as the one Rust
   navigates to in that root, and not the same object as a structurally
   equal copy (identity, not equality).
3. Suspension without blocking a worker: after both runs end `Waiting`,
   each worker runs an unrelated task to completion before the helper
   exists; nothing of a consumer is dispatched between its wait and wake.
4. Multiple consumers, one producer: the first need claimed the single
   producer request, the cell's waiters are both consumers, and one
   publication wakes both.
5. No replay: each consumer's PRE and MEET effects ran once, before the
   wait; it was dispatched exactly twice (execution 1 and 2).
6. Migration: the placement hook sends each woken consumer to the other
   worker, where it resumes (other OS thread) and completes.
7. Collection while suspended: a worker-requested collection while both
   consumers are parked reclaims garbage, the other worker is `Idle`, and
   after resuming each consumer reads intact task-owned state kept only by
   its frames (a transient builder, a fresh vector, live locals).

Events carry world/definition/kind (`world#1/dep#7/kind#2`), task and
worker identities, OS threads, and the suspension's source location in
`consumer.js`; the tests check their order in the host trace and in the
executor record.

Not shown here (deferred):

- The glue is example code: the host holds the producer request and runs
  the producer when the scenario says so (it does not use `host::sim`'s
  worlds and producers, whose callables are still host tokens), and the
  executor still has no producer bindings or wait-cycle detection.

### End-to-end failure, cancellation and companion scenarios (clj-quickjs-29)

The same example, fixtures, host, driver and steps, with one host decision
changed per variant (`examples/e2e/scenario.rs`, `Variant`), plus a
companion scenario on the same stage. Each command is deterministic: the
asserted per-task effects, worker placements, endings and publication are
identical on every run (`every_variant_is_deterministic`); only which
consumer claims the producer request races, and the variants assert in
terms of the claimant.

| Command | What happens | What it proves (§13) |
|---------|--------------|----------------------|
| `cargo run --example e2e` (`success`) | as above; the finally block around the need runs once, after the wait (CLEANUP before POST) | items 1-7, item 8 completion; exception/finally state held across a wait, collection and migration |
| `cargo run --example e2e -- failure` | the producer task is given slot -1, `producer.js` throws and the task fails; the host publishes `Completion::Failed` with a value naming the throw's line and column; the one publication wakes both consumers | item 8 failure: both consumers are woken once, resume on the other worker, their `catch` observes the dependency error (CAUGHT), the finally block runs after it, and each returns the error object whose `Rooted::error_cause` *is* the published value; the cell holds that object; nothing before the need is replayed |
| `cargo run --example e2e -- cancel` | after both consumers park, the host cancels the one whose need claimed the producer request (`Executor::cancel`) | item 8 cancellation: its cleanup run (enqueued as `Cancelled`, on the other worker) runs the pending finally block once (no catch, nothing after the need) and ends `Cancelled`; the cell keeps its producer and loses only that waiter; the producer still completes and its publication wakes only the other consumer, which completes with the input's subtree by identity |
| `cargo run --example e2e -- companion` | one `companion.js` task reaches the helper from inside `__rt.reduce`'s JavaScript callback, inside two `try`/`finally` blocks, with two closures sharing a reassigned local | closing paragraph: a nested call (fold → callback → helper), a live closure-captured local, try/finally state and native-to-JavaScript reentry. The task suspends inside the fold's callback, is parked through a collection, resumes on the other worker; VISIT runs once per step (1, then 2: the counter is one live binding, no step repeated), the inner then outer finally run once each after the fold, and the returned vector holds the input's body and inner subtrees by identity (not equal copies) |

Tests: `cargo test --test e2e` (each row has its own test; the event
record and the host trace are checked for order, including cleanup:
cancellation request, finally effect, `Cancelled` run, then the rest of
the scenario). The fixtures are written for this crate (no QuickJS text is
adapted).

Deferred:

- An uncaught dependency failure, and a cancellation or failure while
  parked inside the native fold, are not part of the example (covered by
  `tests/exception_waits.rs` and `tests/native_unwind.rs`); error objects
  still have no JavaScript-readable properties, so the consumer returns the
  caught error for Rust to inspect.

### Resumable native operations (clj-quickjs-12)

Supported (`src/vm/native.rs` module docs; tests in `tests/native_fold.rs`,
`src/vm/native.rs`):

- Leaf natives (collection, builder and atom intrinsics, `__rt.effect`) stay
  ordinary Rust calls inside one instruction. Resumable natives push a
  task-owned native frame that the interpreter loop drives one step at a
  time (a trampoline): a step may call a JavaScript callback (entered as a
  VM frame), demand a dependency (the same protocol as `__rt.need`), or
  return the operation's value. No Rust frame survives a step.
- `__rt.reduce(f, init, vec)` (opcode `reduce`, `0x3D`): folds a persistent
  vector through `f(acc, x)`. A callback may suspend the whole task (at a
  `__rt.need`) or the budget may run out between steps; the next run, on
  any thread, continues where it stopped without repeating completed
  elements, callbacks or effects. Callbacks may start nested folds.
- `__rt.needAll(defs, kind)` (opcode `need_all`, `0x3E`): needs `kind` of
  each definition id in the vector `defs`, in order, suspending in the
  native for each pending one, and returns a vector of the shared results.
- A native frame's arguments, progress index, accumulator or results so far,
  and pending result destination are traced with the task's frames, while
  suspended, yielded, or running at a safepoint; they become garbage when the
  native returns or the task finishes or is cancelled.
- Each step counts as one budget unit and is a poll point. Failures in a
  step report the starting call's source position; a failing callback has
  that call in its call chain.
- Exceptions: a `try` inside a callback catches what the callback throws
  and the fold continues. A `throw` or catchable failure whose innermost
  handler is around the fold unwinds through it (clj-quickjs-28, below); a
  failure of the `reduce`/`needAll` call itself (bad operands) is caught
  normally.

Deferred:

- Other resumable natives (map/filter, map traversal) and natives callable
  as JavaScript function values; callbacks receive `(acc, x)` only.

### Exceptions across waits, dependency failures and cancellation cleanup (clj-quickjs-27)

Supported:

- A dependency wait is a suspension, not unwinding: a `__rt.need` that
  parks inside a try, catch or finally block, or in a function called from
  one, runs no handler. Handlers, pending completions (saved return values
  and exceptions), frames and locals stay in the task across the wait,
  collection and migration to another thread or worker; a finally block
  that waits then completes its pending `return` or rethrow.
- A failed or cancelled cell raises a catchable error object at the `need`
  (ready at the need or delivered on resume): `VmErrorKind::DependencyFailed`
  / `DependencyCancelled` with the key (world, definition, kind), the need's
  source position and call chain, and the producer's published error value
  as its cause. `Rooted::error()` and `Rooted::error_cause()` read a caught
  error object from Rust. Uncaught (also after finally blocks or a rethrow)
  the run fails with that error.
- Consumer cancellation (`Task::cancel`) is not catchable and runs finally
  blocks, like a generator's `return()`: the observing run leaves the cell,
  unwinds to each pending finally block in turn (skipping catch blocks),
  then ends `Cancelled`. Cleanup cannot be overridden or wait: a `return`
  or exception escaping a finally block, a `need`, or a failure ends it at
  once (still `Cancelled`); it runs under the budget and may yield. A task
  without finally blocks is cancelled without running code, as before.
- Contract: `src/vm/exception.rs` ("Dependency failures", "Waiting is not
  unwinding") and `src/task.rs` ("Cancellation and finally blocks").
  Tests: `cargo test --test exception_waits` (source snippets; manual host,
  thread migration, collections while parked, the two-worker executor) and
  `cargo test --lib task::need_tests` (cancellation inside the need and
  park windows).

Deferred:

- Error objects still have no properties (`message`, `cause` are not
  readable from JavaScript); a cancellation cleanup that ends early reports
  only `Cancelled`, not why.

### Concurrency regression suite (clj-quickjs-14)

`tests/concurrency/` (module docs list the guarantees; the harness is
`tests/concurrency/harness.rs`). Run `cargo test --test concurrency`.

Supported (tested, not new runtime behavior):

- A bounded harness over the production runtime and the two-worker
  executor: a recording driver and hosts, gates inside host effects and
  between a driver's `RunGuard::need` and `RunGuard::park`, migration and
  seeded placement hooks. Every wait is bounded (20 s per wait, 90 s per
  scenario); a hang, a gate never opened, a task left parked (lost wakeup)
  or a broken invariant fails with the harness notes and executor events.
  Self-tests prove each detection. Nothing sleeps; time only bounds.
- Invariants checked after every scenario: no task in two drivers at once,
  dispatch/release alternation on one worker, increasing execution
  numbers, one terminal release, effects only inside their task's runs, no
  task left queued or parked.
- Wake/park/cancel matrix: publish `Ready`/`Failed`, cancel the cell,
  cancel the consumer, duplicate and spurious wakes, and publication plus
  cancellation, each before registration, subscribed-but-not-parked, and
  parked (with a collection while parked); plus a completion racing a
  consumer cancellation (exactly one enqueues; the run is cancelled).
- Sixteen waiters with a racing host claim (one producer), world and
  result-kind isolation with cross-producer rejection, terminal stability
  of cells and tasks, ready reads recording edges, no effect replay, and
  24 seeded schedules mixing plain, cancelled-before-need and
  cancelled-while-parked consumers with a racing publication.
- Cross-feature: a fold callback suspended inside `try`/`finally`, and a
  fold inside a `finally` with a pending `return`, collected and migrated
  at every suspension (cleanup once, after the last result; identities
  kept; input reclaimed after); an atom replaced three times by a native
  job while a parked task holds an earlier read (old read survives,
  unheld intermediates are reclaimed, fresh load sees the newest); deep
  recursion and mutually recursive published callables waiting across
  migration with no cycle reported, versus a genuine two-producer wait
  cycle reported by `DependencyGraph` and settled by failing the newest
  demand's result; a producer failure racing the cancellation of half of
  eight waiters guarded by `catch`/`finally` (cancelled ones run finally
  once and no catch; the others are woken once, migrate and catch the
  failure at the need).

Deferred:

- Single-task exception, cancellation-cleanup and native-unwinding cases
  stay with clj-quickjs-27 (`tests/exception_waits.rs`) and -28; the GC
  root inventory is clj-quickjs-31. Windows inside one cell critical section
  (between the fast-path read and subscription) stay with the hook-forced
  unit tests (`src/task/handoff_tests.rs`); here they are only raced.

### GC root inventory (clj-quickjs-31)

- Every holder of heap values the collector must see (host handles and
  scope locals, published task results and dependency outcomes, running,
  suspended and need-suspended frames, executor ready queue and parked set,
  `finally` completions, resumable native frames, transient builders, atom
  current and reader-retained old values, closure environments and binding
  cells, persistent collection edges, executable object constants, native
  payload edges) has a retention and a reclamation test, the latter with
  stale-handle rejection after slot reuse; a multi-worker variant keeps
  every holder alive while executor workers allocate and collect. The
  inventory table is in `docs/architecture.md` ("Root inventory").
- Cross-check: `src/heap/inventory.rs` maps every `RootCategory` and object
  kind to its holders with exhaustive matches and checks that each
  holder's tests exist and are documented.
- Tests: `cargo test --test gc_roots`, `cargo test --lib heap::inventory::`.

Deferred:

- A published dependency outcome is reclaimable only with its runtime (cells
  are never removed); its reclamation is tested on a dropped cell table.
  Executable object constants exist only in crate tests (loaded code has
  primitive constants).

### Failures, cancellation and migration inside native continuations (clj-quickjs-28)

Supported (contract: `src/vm/native.rs`, "Errors" and "Safepoint contract
and debug borrow checks"; `src/task.rs`, "Cancellation and finally blocks"):

- An exception whose handler is around a pending `__rt.reduce` or
  `__rt.needAll` (in the frame that called it or a caller) unwinds through
  it: a callback's `throw` or catchable error, a dependency failure seen
  in a callback, and a failed or cancelled dependency `needAll` demanded
  (also when the failure arrives while the task is parked and it resumes
  on another thread). The native receives the failure (callback,
  dependency or own step), answers `Unwind`, and its frame is popped:
  what it held (collection, callback, accumulator, results so far) is no
  longer a root. Catch and finally blocks run as for any exception; nested
  natives unwind innermost first.
- Consumer cancellation of a task suspended inside a native follows the
  clj-quickjs-27 policy: the cell is left (the producer and its other
  consumers are unaffected), finally blocks inside the callback run, then
  the native frame is dropped without running native code and the finally
  blocks below it run; the run ends `Cancelled`.
- A task waiting inside a fold's callback resumes on the other executor
  worker and completes; a transient builder the task created before the
  fold is still usable from the callback after the migration.
- Debug builds enforce the safepoint contract: holding a scoped borrow
  (as a native step does) while entering the dispatch loop, starting a
  task run, entering a call frame, pushing a value or native frame,
  reaching a VM safepoint, leaving for a dependency or parking panics with
  "native safepoint contract violated". Release builds compile the checks
  out.
- Tests: `cargo test --test native_unwind`, `cargo test --lib vm::native::`,
  and the updated `native_fold` / `exception_waits` tests.

Deferred:

- No native recovers from a failure (every one answers `Unwind`); natives
  with their own cleanup or recovery actions, and error properties
  describing which native element failed.
- Heap object reads (`Heap::read` closures) are not counted as scoped
  borrows; only native steps are.

### Measurement harness (clj-quickjs-15)

`examples/bench/` (std only). `docs/measurement.md` has the method, the
exact commands, one recorded run with its environment, and what the
numbers do and do not show. Run `cargo run --release --example bench`
(or `-- --quick`, or name row groups).

Supported:

- Timed rows (median and min..max over measured rounds, setup outside the
  timed interval, results checked): task spawn and first run, already-ready
  dependency reads from JavaScript and from `RunGuard::need`, single-thread
  need/park/complete/wake/resume cycles, persistent vector and map lookups
  at three sizes and updates from empty, and executor throughput of
  unrelated work alone versus with parked consumers.
- Memory rows from a counting global allocator in the example: shared
  input and code bytes versus per-consumer bytes for 1 to 10,000 parked
  consumers, both parked in a plain frame and inside an `__rt.reduce`
  callback (a suspended native continuation), heap objects per consumer,
  storage kept after drop, host-owned handle storage (excluded from task
  figures), RSS, and instrumentation overhead rows.
- Read-only snapshots: `Runtime::heap_usage` (`HeapUsage`: live objects by
  kind, known-shareable count, slots, shallow table bytes) and
  `Task::stats` (`TaskStats`: saved frames, stack values, native frames,
  pending need, shallow byte estimate). `HeapStats` is unchanged.
- `tests/measurement.rs` runs every workload small and asserts results and
  the deterministic object counts (each parked consumer adds exactly its
  own builder; the input is never copied), never times or bytes.

Deferred:

- No comparison with light-cljs, QuickJS or V8 (none performed).
- Not measured: contention on one cell or the object lock with many
  workers, collection pause times, isolated cross-thread wake latency.
- Allocator bytes are requested sizes; `HeapUsage` and `TaskStats` bytes
  are shallow estimates (see their docs).

### Arrow functions (clj-quickjs-53)

Supported (`cargo test --lib frontend::tests::arrow_functions_and_asi`,
`cargo test --test js_functions arrow_function_values`):

- `x => e`, `() => e`, `(a, b,) => { ... }` from source, lowered as
  closures exactly like anonymous function expressions (captures, cells,
  temporal dead zone, publication rules). A `(...)` followed by `=>` is a
  parameter list; an arrow is a whole AssignmentExpression, so a line
  break after a block body ends the statement (`() => {}` newline `+1`
  completes with 1) and an unparenthesized arrow operand or callee is a
  syntax error, as is a line break before `=>`. Grammar and errors:
  [`docs/grammar.md`](docs/grammar.md).

Deferred:

- `async` arrows; rest, default and destructuring parameters; `this`,
  `arguments`, `new.target` and `super` (rejected inside arrows as
  everywhere else).
- QuickJS's `var h = x => x + 1` newline `yield 2` (needs `var` and
  generators) and `() => eval('0.a')` (needs `eval`).

### Comma operator (clj-quickjs-61)

Supported (`cargo test --lib frontend::tests::comma_operator`,
`cargo test --test js_source comma_operator_evaluates_left_to_right`):

- `a, b, c` wherever the grammar has an Expression (expression
  statements, parentheses, `if`/`while` tests, `return`/`throw` operands):
  left-associative, every operand runs left to right and the last one is
  the value (also the script's completion value). Arguments, `let`/`const`
  declarators, `? :` branches and concise arrow bodies stay
  AssignmentExpressions, so `f(a, b)` has two arguments, `f((a, b))` one,
  and `a ? b, c : d` is a syntax error; `(a, b) => ...` is still an arrow.

Deferred:

- Template substitutions (`` `${a, b}` ``) wait for template literals.

### Unicode identifiers (clj-quickjs-50)

Supported:

- Identifiers made of Unicode `ID_Start` / `ID_Continue` code points (plus
  `$`, `_`, ZWNJ, ZWJ), from Unicode 16.0 tables in
  `src/frontend/unicode_ident.rs` (regenerate with
  `scripts/gen-unicode-ident.sh`, which needs perl), and identifier escapes
  `\uHHHH` / `\u{H...}`. Names are code point sequences compared without
  normalization everywhere (binding resolution, the reserved `__rt`
  binding). A reserved word written with escapes is rejected except as a
  property name after `.`. Details: "Lexical grammar" in
  [`docs/grammar.md`](docs/grammar.md).

Deferred:

- Diagnostics quote a name as written in the source (an escaped name is
  quoted with its escapes), and the identifier tables follow Unicode 16.0
  until regenerated.

### Bitwise and shift operators (clj-quickjs-62)

Supported (`cargo test --lib frontend::tests::bitwise_and_shift_precedence`,
`cargo test --test js_source bitwise_operators_follow_to_int32`):

- `&`, `|`, `^`, `<<`, `>>`, `>>>` and unary `~` at ECMAScript precedence
  (`||` < `&&` < `|` < `^` < `&` < equality < relational < shift <
  additive). Both operands are evaluated, then converted with ToNumber
  (left first) and ToInt32 (ToUint32 for the left operand of `>>>`); shift
  counts are `ToUint32(right) & 31`; results are 32-bit integers as doubles
  (unsigned for `>>>`), never `-0`. Number-only opcodes `bit_and`,
  `bit_or`, `bit_xor`, `shl`, `sar`, `ushr`, `bit_not` (`0x3F`–`0x45`).
  Run-time cases adapted from QuickJS `test_op1` / `test_cvt` with the
  license notice.

Deferred:

- Compound assignment (`&=`, `|=`, `^=`, `<<=`, `>>=`, `>>>=`) is still
  rejected by the parser; string operands wait for StringToNumber
  (clj-quickjs-67) and object operands for ToPrimitive (they fail with
  `UnsupportedOperand`).

### Unlabelled `break` and `continue` (clj-quickjs-78)

Supported (`cargo test --lib frontend::tests::break_and_continue`,
`cargo test --test js_source while_break_and_continue`):

- `break;` and `continue;` inside a `while` loop body of the same function:
  `break` jumps past the innermost loop, `continue` to its test. A line
  break after the keyword ends the statement. Outside a loop (including in
  a function nested in one) they are early errors (`Invalid`).
- Leaving `try` blocks pops their catch handlers (`pop_catch`), so a later
  exception is not caught by a handler the jump left; leaving a catch body
  needs nothing. A task may yield, wait or migrate anywhere in the loop:
  the jumps are ordinary branches and the state stays in the task.
- Script completion values follow ECMAScript `UpdateEmpty`
  (`5; while (true) { 6; break; }` is 6, `5; while (true) { break; }` is
  `undefined`).

Deferred:

- Labels (`break l;` is `Unsupported`) and `switch`. (`do`-`while`, `for`:
  clj-quickjs-80, -81, below.)

### Exponentiation operator (clj-quickjs-63)

Supported (`cargo test --lib frontend::tests::exponent_operator_syntax`,
`cargo test --test js_source exponent_operator`):

- `**`, right-associative and binding tighter than `*`; its right operand
  may be a unary expression (`2 ** -1`), its left operand may not be an
  unparenthesized one (`-2 ** 2` is a SyntaxError of kind `Invalid` at the
  `**`; write `(-2) ** 2` or `-(2 ** 2)`). Both operands are evaluated,
  then converted with ToNumber (left first), then Number::exponentiate:
  IEEE `pow` except that a NaN or ±Infinity exponent with base `1` or `-1`
  gives NaN. Number-only opcode `pow` (`0x46`). `2 ** 8` is adapted from
  QuickJS `test_op1` with the license notice.

Deferred:

- `**=` is still rejected by the parser (compound assignment); string
  operands wait for StringToNumber (clj-quickjs-67) and object operands for
  ToPrimitive (they fail with `UnsupportedOperand`). BigInt is outside the
  subset.

### `break` and `continue` through finally blocks (clj-quickjs-79)

Supported (`cargo test --test exceptions break_and_continue`,
`cargo test --test exceptions a_jump_through_finally`,
`cargo test --test exception_waits jump`,
`cargo test --test exception_waits a_break_through_finally`):

- A `break` or `continue` that leaves a `try` block or catch body with a
  finally clause runs the finally blocks it leaves, innermost first, then
  completes the jump. A finally block that completes abruptly (`return`,
  `throw`, `break`, `continue`) replaces the pending jump; a jump out of a
  finally block discards its pending return, exception or jump.
- The pending jump is a fourth pending-completion kind of the task-owned
  handler stack (opcodes `call_finally` `0x47` and `drop_completion`
  `0x48`), so a task may yield, wait, migrate or be collected inside a
  finally block a jump runs and finish the jump afterwards.
- During cancellation cleanup a jump out of a finally block running for the
  cancellation ends the cleanup there, like a `return`.
- Completion values: `5; while (true) { try { 6; break; } finally { 7; } }`
  is 6. QuickJS `test_try_catch5` runs from source (adapted, with notice).

### `++`/`--` on bindings (clj-quickjs-64)

Supported (`cargo test --lib frontend::tests::update_expressions`,
`cargo test --test js_source update_expressions_on_bindings`):

- Prefix and postfix `++`/`--` on an identifier binding, possibly
  parenthesized: read (temporal-dead-zone and unresolvable-name checks as
  for any read), ToNumber, add or subtract 1, store (as `=`: a `const` is a
  `TypeError`, a closure-captured binding is updated through its binding
  cell). A prefix update yields the new value, a postfix one the old value
  converted to a number (`let t = true; t++` is 1). No new opcode. A line
  break before `++`/`--` starts a new statement (`a` newline `++b`).
  The identifier cases of QuickJS `test_inc_dec` are adapted with the
  license notice.

Deferred:

- `++`/`--` on properties (`a.b++` is `Unsupported`; clj-quickjs-76);
  object operands wait for ToPrimitive (they fail with
  `UnsupportedOperand`); compound assignment stays rejected.

### `do`-`while` loops (clj-quickjs-80)

Supported (`cargo test --lib frontend::tests::do_while_statements`,
`cargo test --test js_source do_while_loops`):

- `do Statement while (Expression);`: the body runs, then the test, and the
  loop repeats while the test is truthy, so the body always runs at least
  once. The `;` after `while (...)` is optional even on the same line
  (`do x; while (0) y;` is two statements). A declaration as the body is
  an early error (`Invalid`).
- Unlabelled `break` leaves the loop; `continue` jumps to the test. Both
  pop catch handlers and run finally blocks on the way, as in `while`
  loops. The loop is ordinary branches, so a task may yield, wait or
  migrate anywhere in it. QuickJS `test_do_while` runs from source
  (adapted, with notice).
- Completion values follow ECMAScript `UpdateEmpty`:
  `5; do { 6; } while (false)` is 6, `5; do { } while (false)` is
  `undefined`.

Deferred:

- Labelled `do`-`while` (labels: clj-quickjs-82).

### `for` loops (clj-quickjs-81)

Supported (`cargo test --lib frontend::tests::for_statements`,
`cargo test --test js_source for_loops`):

- `for (init; test; update) Statement` with an empty head part, an
  expression init, or a `let`/`const` declaration list (`for (;;)`,
  `for (i = 0; i < 3; i++)`, `for (let i = 0, j; ; )`). An absent test is
  true. A declaration as the body, and `const` without an initializer, are
  early errors (`Invalid`).
- Unlabelled `break` leaves the loop; `continue` runs the update, then the
  test. Both pop catch handlers and run finally blocks on the way. The
  loop is ordinary branches and locals, so a task may yield, wait or
  migrate anywhere in it. The first group of QuickJS `test_for` runs from
  source (adapted, with notice).
- A `let`/`const` head's bindings are scoped to the loop and in their
  temporal dead zone during their own initializers (`for (let i = i; ;)`
  is a ReferenceError). Each iteration has its own copy of the `let`
  bindings (ECMAScript CreatePerIterationEnvironment): closures created in
  the body keep their iteration's value, and an assignment a closure makes
  during an iteration carries into the next. A captured, assigned head
  binding lives in a binding cell, and the copy is a new cell before the
  first test and before each update; no new opcode. Updating a `const`
  head is a TypeError.
- Completion values follow ECMAScript `UpdateEmpty`:
  `5; for (;;) { 6; break; }` is 6, `5; for (let i = 0; i < 0; ) {}` is
  `undefined`.

Deferred:

- `var` heads (clj-quickjs-89), for-in (clj-quickjs-85 to -88) and for-of
  (`Unsupported`), labelled `for` (labels: clj-quickjs-82).
