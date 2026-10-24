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
cargo test --test atoms           # atoms: identity CAS, single-writer publication, load-to-root
cargo test --lib atom::           # SameValue comparison, replaced-box reclamation
cargo test --test cancellation    # consumer and cell cancellation racing settlement and parking
cargo test --lib task::cancel_tests # hook-forced cancellation in every handoff window
cargo test --test safepoints      # safepoints: collection requests during runs and scopes
cargo test --lib heap::gc::       # rendezvous root snapshot with running tasks
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Crate layout

Library (`src/lib.rs`) plus a thin binary (`src/main.rs`). Modules:
`runtime` (embedding entry point, `collect`), `task` (task state, execution
guard, run outcomes), `vm` (instruction format, executable validation,
task-owned frames, dispatch, test assembler), `heap` (object identity, roots,
scoped and weak handles, tracing contract, mark/sweep collector), `value`,
`ids`, `error`, `host` (leaf host effects; `host::sim`, the simulated
compiler host), `collections` (persistent
vectors, maps and occurrences), `dep` (dependency keys, single-assignment
cells, read-or-subscribe, per-task dependency log), `atom` (atomic
references: single-writer publication, identity compare-and-set). See
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

- Dependency waits in the VM (pellet clj-quickjs-23; the cells: clj-quickjs-6,
  parking and `Waiting(key)`: clj-quickjs-7, below).
- Resumable natives that call JavaScript or suspend (clj-quickjs-12); a
  scheduler/executor with budgets (clj-quickjs-8). Budgets count instructions
  only, not time or allocation.

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
  (clj-quickjs-17, below). Executor integration is clj-quickjs-25;
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

- The `__rt.need` VM operation (clj-quickjs-23); needs are issued from Rust
  through a run guard for now.
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
- `throw`/`try` (clj-quickjs-11): ReferenceError and TypeError failures
  cannot be caught. Operators on objects need ToPrimitive and fail with
  `UnsupportedOperand` (objects cannot come from source yet).
- `for`, `do`-`while`, `break`, `continue`, `switch`, `var`, update and
  compound assignment operators (rejected by the parser).

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

- The VM instruction that needs and parks by itself (clj-quickjs-23); a
  reference executor with a ready queue (clj-quickjs-8). Cancellation:
  clj-quickjs-24 below.

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
  unshareable result), `InputFailed`/`InputCancelled`; `Stalled` when a
  request is left waiting with no runnable work. Events (`Scheduled`,
  `Parked`, `Woken`, `Activation`, `Settled`) and per-producer dependency
  logs are recorded. Contract: `src/host/sim.rs` module docs; summary in
  [`docs/architecture.md`](docs/architecture.md).

Deferred:

- Real calls: a callable is a host token and calls between callables use a
  stand-in tail-call convention (a returned number calls the declared tail
  callee) until VM calls, function values and `__rt.need` exist
  (clj-quickjs-4, -20, -21, -23).
- Worker threads (clj-quickjs-8), wait-cycle detection and reporting
  (clj-quickjs-26), cancelling a stalled request, and invalidation.

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
  (`length`, `name`), native or host-function callees, and catching a
  `StackOverflow` or `TypeError` (clj-quickjs-11).

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
  need observes a cancellation only if it would yield); cancellation as a
  language-level unwind with `finally` cleanup (clj-quickjs-27) and through
  native continuations (clj-quickjs-28); cancelling a producer
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

- `__rt.need` (reserved; `Unsupported` until clj-quickjs-23 lowers it to a
  resumable dependency wait).
- `throw`/`try`/`finally` (clj-quickjs-11): run-time errors still fail the
  task. Strings, property access (`f.call`, methods, objects), `this`,
  `arguments`, `new`, default/rest parameters, arrow functions, generators.
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
