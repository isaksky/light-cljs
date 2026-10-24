# clj-quickjs

A Rust-native JavaScript runtime for highly concurrent compile-time execution.
Requirements: [`docs/design.md`](docs/design.md). Backlog: Pellets (`pl list`).

This is an architectural slice in progress, not a finished JavaScript engine.
A small documented JavaScript subset ([`docs/grammar.md`](docs/grammar.md))
compiles to VM bytecode and runs: straight-line code, conditionals, loops,
locals, functions, calls, recursion and closures in task-owned frames, and
private collection and host-effect intrinsics through the compiler-reserved
binding `__rt`, and string values (clj-quickjs-65). Property access is not
executable yet.

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
  Operators and calls are located at their token (the operator, the
  call's `(`, also for caller frames), references at the identifier
  (clj-quickjs-52).
- Fixtures: `tests/fixtures/js/` (synthetic, not light-cljs output, plus
  adapted QuickJS `test_op1`/`test_while` cases with their license notice).

Deferred:

- Property access: it parses but is rejected by the compiler as
  `Unsupported`. (Functions, calls, closures and the reserved intrinsic
  binding compile since clj-quickjs-21, string values since clj-quickjs-65,
  below.)
- Object operand conversion is supported since clj-quickjs-70 (below).
- The logical assignment operators (rejected by the parser). (`var`:
  clj-quickjs-94; update: clj-quickjs-64; compound assignment: clj-quickjs-84; unlabelled `break` and
  `continue`: clj-quickjs-78; `do`-`while`: clj-quickjs-80; `for`:
  clj-quickjs-81; labels: clj-quickjs-82; `switch`: clj-quickjs-83, below.)

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
- `arguments`, rest parameters (defaults: clj-quickjs-100, below), and native
  or host-function callees. (Function metadata: clj-quickjs-101, below; `this`:
  clj-quickjs-116, below.)
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

- Property access (`f.call`, methods, objects), `this` (clj-quickjs-116
  below), (string values: clj-quickjs-65 below)
  `arguments`, rest parameters (defaults: clj-quickjs-100, below), generators (`new`:
  clj-quickjs-71, below). (Arrow
  functions: clj-quickjs-53 below.)
- (A function declaration repeating a parameter or another declaration at
  the top level of a script or function body runs since clj-quickjs-94,
  below.)
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

- Definition ids and result kinds are numbers (string values exist since
  clj-quickjs-65, but `need` takes numbers only).
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
- Adapted QuickJS `test_try_catch1`–`8` run from source with their
  upstream strings, `for(;;)` and `assert()` calls (clj-quickjs-227; `5`
  since clj-quickjs-79, `8` with a counted `for` for `for`-`in`)
  (`tests/fixtures/js/quickjs_try_catch.js`, with its license notice and
  its remaining adaptations: `let` for `var` and an `assert` that throws
  its message string).

Deferred:

- Other failures (unsupported operands, collection, builder, atom and
  binding contract violations, host errors, out of memory) are not
  catchable and skip catch and finally blocks. Failed or cancelled
  dependencies became catchable, and cancellation runs finally blocks, in
  clj-quickjs-27 (below).
- Error objects have no properties (`message`, `name`), no `Error`
  constructors or `instanceof`. (Strings can be thrown since clj-quickjs-65.)

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

- `async` arrows; rest and destructuring parameters (defaults: clj-quickjs-100,
  below), `arguments`, `new.target` and `super` (rejected inside arrows).
  Lexical `this` is supported by clj-quickjs-98, below.
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

Template substitutions (`` `${a, b}` ``) are Expressions too since
clj-quickjs-57.

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

- (Compound assignment `&=`, `|=`, `^=`, `<<=`, `>>=`, `>>>=`:
  clj-quickjs-84, below.) Object operands convert since clj-quickjs-70; strings since clj-quickjs-67.

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

- Nothing specific (`do`-`while`, `for`, labels, `switch`:
  clj-quickjs-80, -81, -82, -83, below).

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

- (`**=`: clj-quickjs-84, below.) Object
  operands convert since clj-quickjs-70; strings since clj-quickjs-67. BigInt is outside the
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

- (`++`/`--` on properties: clj-quickjs-76, below.) Object operands convert since clj-quickjs-70. (Compound assignment: clj-quickjs-84, below.)

### `++`/`--` on properties (clj-quickjs-76)

Supported (`cargo test --lib frontend::tests::update_expressions`,
`cargo test --test js_source update_expressions_on_properties`,
`cargo test --test need property_update_resumes_after_wait`,
`cargo test --test gc_roots property_update_base_survives`):

- Prefix and postfix `++`/`--` on `o.p` and `o[k]`, possibly
  parenthesized: the base and the key are evaluated once, the base is
  checked (`undefined`/`null` is a catchable `TypeError`) and the key
  converted with ToPropertyKey once, the property is read (absent:
  `undefined`, so `let o = {}; o.x++` is NaN), converted with ToNumber,
  incremented or decremented and written back as `o.p = v` writes. A
  prefix update yields the new number, a postfix one the old number. New
  opcodes `Dup2` (`0x5C`) and `ToPropertyKey` (`0x5D`). The member cases
  of QuickJS `test_inc_dec` are adapted with the license notice.

Deferred:

- Object property values and keys convert since clj-quickjs-70. (Compound assignment on properties:
  clj-quickjs-260, below.)

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

- Nothing specific (labelled `do`-`while`: clj-quickjs-82, below).

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

- For-of (`Unsupported`). (Labelled `for`: clj-quickjs-82; for-in:
  clj-quickjs-85; `var` heads: clj-quickjs-89, below.)

### String values (clj-quickjs-65)

Supported (`cargo test --test js_source string_values`,
`cargo test --test publication strings_publish`,
`cargo test --test gc strings_in_frames`, `cargo test --lib value::`):

- String literals compile to constants and run; a script may complete with a
  string (`cargo run -- file.js` prints it; embedders read it with
  `Rooted::string` / `Scope::string`, `ValueView::String`, `JsString`).
- `===`/`!==`/`==`/`!=` between strings compare contents; a string never
  strictly equals a non-string; `""` is the only falsy string; `+` of two
  strings concatenates (new opcode `plus` `0x49`, emitted for every `+`
  whose operands are not both statically numbers); calling a string is a
  `TypeError` "string is not a function"; an uncaught thrown string is
  reported with its text.
- Strings are immutable primitives over `Arc` storage outside the heap
  (`docs/architecture.md`, "String values"): publishable to other tasks,
  usable as persistent map keys by content (`__rt.get(__rt.map("k", 1), "k")`
  is 1), stored as UTF-16 code units.
- Strings are UTF-16 code-unit sequences, lone surrogates included
  (clj-quickjs-141; `cargo test --test js_source strings_are_code_unit_sequences`,
  `cargo test --lib frontend::tests::unicode_string_escapes_decode_code_units`):
  `\u` escapes decode to code units, so `"\u{dbff}\u{dfff}"` and
  `"\u{dbff}" + "\u{dfff}"` equal `"\u{10ffff}"`, a lone surrogate escape
  (`"\u{d800}"`) is a string that equals only itself, and such a string
  publishes to other tasks and works as a map key. `cargo run` prints a
  string result exactly (a lone surrogate as `\u{d800}`).
- `+` with a string operand concatenates the ToString of both operands
  (clj-quickjs-66; `cargo test --test js_source addition_concatenates_strings`,
  `cargo test --lib value::string::`): `"a" + 1` is `"a1"`, `1 + 2 + "3"`
  is `"33"`, `"" + null` is `"null"`. Numbers convert by Number::toString
  in radix 10 (shortest round-trip digits; `"" + 1e21` is `"1e+21"`,
  `"" + 0.000001` is `"0.000001"`, `"" + 1.5e-7` is `"1.5e-7"`,
  `"" + -0` is `"0"`). Without a string operand `+` stays numeric
  (`true + 1` is 2).

Deferred:

- Object operands convert with ToPrimitive since clj-quickjs-70. (Strings convert to
  numbers since clj-quickjs-67 and compare relationally by code units since
  clj-quickjs-68, below; `typeof` is clj-quickjs-69; untagged templates
  are clj-quickjs-57.) String `length` and indexing are clj-quickjs-142 below;
  additional string methods and radixes other than 10 remain deferred.

### StringToNumber and loose equality with strings (clj-quickjs-67)

Supported (`cargo test --test js_source strings_`,
`cargo test --lib value::string::`):

- ToNumber of a string is StringToNumber (`string_to_number` in
  `src/value/string.rs`) wherever ToNumber applies: `- * / %`, unary `-`
  and `+`, `**`, the bitwise and shift operators and `~`, `++`/`--`, and
  relational comparison when not both operands are strings. Leading and
  trailing whitespace and line terminators (NBSP, BOM and Unicode `Zs`
  included) are ignored; `""` is 0; unsigned `0x`/`0o`/`0b` integers of
  any length round correctly; decimals take a sign, fraction and exponent
  (`".5"`, `"5."`, `"-0"` is -0, `"010"` is 10); `Infinity` takes a sign;
  everything else is NaN (`"1_000"`, `"1n"`, `"-0x1"`, `"12px"`,
  `"infinity"`). `"6" * "7"` is 42, `"0x12345" >>> 0` is 74565.
- `==`/`!=` between a string and a number compare the number with the
  string's StringToNumber; a boolean against a string converts to 0 or 1
  first (`"" == 0`, `" 1 " == 1`, `"1" == true` are true, `"2" == true` and
  `"abc" == NaN` are false); a string never loosely equals `null` or
  `undefined`. No new opcode. The string cases of QuickJS `test_eq` and
  `test_cvt` are adapted with the license notice.

Deferred:

- Object operands convert since clj-quickjs-70.

### Relational comparison of strings (clj-quickjs-68)

Supported (`cargo test --test js_source strings_compare_by_code_units`):

- `<`, `>`, `<=`, `>=` with two string operands compare them
  lexicographically by UTF-16 code units, a proper prefix being smaller
  (ECMAScript IsLessThan, QuickJS `js_string_compare`): `"b" > "a"`,
  `"a" < "aa"`, `"" < "a"`, `"B" < "a"` and `"10" < "9"` are true, and
  `"\u{10000}" < "\uFFFF"` is true (U+10000 is D800 DC00). A string against
  a non-string still compares numerically (`"10" < 9` and `"abc" >= 1` are
  false). New opcodes `compare_lt`/`compare_gt`/`compare_le`/`compare_ge`
  (`0x4A`–`0x4D`), emitted when neither operand is statically a number;
  with a static number on either side the compiler keeps ToNumber and the
  number-only `lt`/`gt`/`le`/`ge`.

Deferred:

- Object operands convert with hint number since clj-quickjs-70.

### Labelled statements (clj-quickjs-82)

Supported (`cargo test --lib frontend::tests::labelled_statements`,
`cargo test --test js_source labelled_break_and_continue`,
`cargo test --test exception_waits labelled_jumps_wait_and_resume_after_migration`):

- `L: Statement` on any statement but a declaration, including blocks,
  `if`, `try` and loops, and as the body of `if`/`else` and loops; label
  sets (`L1: L2: while ...`). `break L;` leaves the statement labelled `L`;
  `continue L;` continues the loop labelled `L`, leaving every loop in
  between. The label must be on the keyword's line (otherwise ASI ends the
  statement). Labels are scoped to their statement and function, separate
  from bindings, and may be reused by sibling statements.
- Early errors (`Invalid`): an undefined label (labels do not cross
  function boundaries), `continue` to a label of a statement that is not a
  loop, a label repeated inside its own statement, a labelled declaration.
- Lowering reuses the `break`/`continue` exit stack: a labelled jump pops
  catch handlers, runs finally blocks and drops pending finally completions
  on the way, like an unlabelled one, and is an ordinary branch, so a task
  may yield, wait or migrate anywhere inside a labelled statement. No new
  opcode. QuickJS `test_for_break`, `test_labels` (without `with`),
  `test_labels2` and `test_cyclic_labels` run from source (adapted, with
  notice).
- Completion values follow ECMAScript `LabelledEvaluation` and
  `UpdateEmpty`: `5; L: { break L; }` is 5, `5; L: { 6; break L; }` is 6,
  `5; L: if (true) break L;` is `undefined`.

Deferred:

- `with` (clj-quickjs-110). (Labelled `switch`: clj-quickjs-83, below.)

### `switch` statements (clj-quickjs-83)

Supported (`cargo test --lib frontend::tests::switch_statements`,
`cargo test --test js_source switch_statements`,
`cargo test --test exception_waits switch_waits_survive_migration_and_collection`):

- `switch (e) { case a: ... default: ... }` with `default` in any
  position: the discriminant is evaluated once, the `case` tests in source
  order with `===` (`-0` matches `0`, `NaN` matches nothing, `"1"` does not
  match `1`) until one matches, and none after it; with no match, control
  goes to `default` if present. Clauses fall through. An unlabelled `break`
  leaves the switch, `continue` continues the enclosing loop, a labelled
  `break` leaves a labelled switch; jumps out run finally blocks.
- The case block is one scope: a name declared twice across clauses is an
  early error (`Invalid`, from `compile_script`), function declarations
  are hoisted to its start, and a `let`/`const` of a clause before the
  last non-empty one lives in a binding cell, so entering a later clause
  past it gives a run-time ReferenceError at the reference
  (`switch (1) { case 0: let y = 1; case 1: y; }`).
- Early errors (`Invalid`): a second `default`; `continue` with no
  enclosing loop. Completion values: `undefined` at entry, then
  `UpdateEmpty` over the clauses that ran
  (`switch (1) { case 1: 7; case 2: 8; }` is 8).
- Lowering: the discriminant lives in a scratch local, so clause bodies
  start at an empty operand stack and the switch is ordinary branches: a
  task may yield, wait or migrate inside any clause. No new opcode.
  QuickJS `test_switch1` and `test_switch2` run from source (adapted, with
  notice), as digit versions and as string versions (the string versions
  with upstream's `s += a` since clj-quickjs-84).

### Compound assignment on bindings (clj-quickjs-84)

Supported (`cargo test --lib frontend::tests::compound_assignment`,
`cargo test --test js_source compound_assignment_on_bindings`):

- `x op= v` for `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `&=`, `|=`, `^=`,
  `<<=`, `>>=` and `>>>=` on an identifier binding, possibly
  parenthesized; right-associative (`a -= b *= 2`), with the targets of
  `=` (`1 += 2` is `Invalid`, `undefined += 1` and `__rt += 1` are
  `ReservedWord`).
- Evaluation: read the binding (temporal-dead-zone and unresolvable-name
  checks as for any read, before the right operand), evaluate `v`, apply
  the binary operator's own instructions (`+=` concatenates when either
  value is a string, the others apply ToNumber to both values), store (as
  `=`: a `const` is a `TypeError` after `v`, a closure-captured binding is
  updated through its binding cell). The value is the stored value; the
  old value is the one read before `v` runs. No new opcode; failures of
  the operator are reported at the `op=` token.

Deferred:

- Logical assignment (`&&=`, `||=`, `??=`) stays `Unsupported`. (Compound
  assignment to a property: clj-quickjs-260, below.)

### Compound assignment on properties (clj-quickjs-260)

Supported (`cargo test --lib frontend::tests::compound_assignment`,
`cargo test --test js_source compound_assignment_on_properties`,
`cargo test --test js_source deep_property_compound_assignments`,
`cargo test --test need property_compound_assignment_resumes_after_wait`,
`cargo test --test gc_roots property_compound_assignment_base_survives`):

- `o.p op= v` and `o[k] op= v` for the operators of compound assignment
  on bindings, possibly parenthesized and right-associative. The base and
  the key are evaluated once and the key converted with ToPropertyKey
  once (`a[i = i + 1] += 4` and `get().x += 1` run their key and base
  once); then the property is read (a `null`/`undefined` base is a
  catchable `TypeError` before `v` runs), `v` is evaluated, the operator
  applied as on a binding, and the result written as `o.p = v` writes (a
  read-only property is a catchable `TypeError` in strict code and left
  unchanged in sloppy code). The value is the operator's result. No new
  opcode: the reference instructions of property `++`/`--`
  (clj-quickjs-76) around the operator's own. The read and the write are
  reported at the `.`/`[`, the operator at the `op=`.

Deferred:

- Logical assignment on properties (`o.p &&= v`) stays `Unsupported`; a
  property value, operand or key that is an object converts since clj-quickjs-70. Assignment chains deeper than about 190
  levels can overflow a 2 MiB debug stack before the nesting limit
  (clj-quickjs-262).

### `typeof` (clj-quickjs-69)

Supported (`cargo test --test js_source typeof_operator`,
`cargo test --lib vm::dispatch::tests::typeof_classifies_every_object_kind`,
`cargo test --lib frontend::tests::unary_binds_tighter_than_binary_and_looser_than_calls`):

- `typeof x`, a prefix operator with the precedence of `!` (so
  `typeof a ** 2` is `Invalid` at the `**`), evaluates to the type name of
  its operand's value as a string: `"undefined"`, `"object"` for `null`,
  `"boolean"`, `"number"`, `"string"`, `"function"` for function objects
  (declarations, expressions, arrows, closures: the only callable values),
  and `"object"` for every other object (persistent vectors, maps and
  occurrences, builders, atoms, caught error objects, plain objects). New
  opcode `typeof` (`0x4E`).
- A name with no binding as the operand, possibly parenthesized
  (`typeof nope`, `typeof (nope)`), is `"undefined"` rather than a
  ReferenceError, including the upstream `typeof õ` case of
  `test_unicode_ident`; a binding in its temporal dead zone still throws.
  Other operands (`typeof (0, nope)`, `typeof nope()`) are evaluated as
  usual.

Deferred:

- `typeof Object` is `"function"` since clj-quickjs-117. Once a global
  object exists (clj-quickjs-104), an unbound name must be looked up at run
  time rather than decided by the compiler.

### `void` (clj-quickjs-133)

Supported (`cargo test --test js_source void_operator`,
`cargo test --lib frontend::tests::unary_binds_tighter_than_binary_and_looser_than_calls`):

- `void x`, a prefix operator with the precedence of `!`, evaluates its
  operand once (its effects and exceptions happen; `void nope` is a
  ReferenceError), discards the value and gives `undefined`. No new
  opcode: the operand, `pop`, `undefined`.

### `var` declarations and var-scoped function declarations (clj-quickjs-94)

Supported (`cargo test --lib frontend::tests::var_statements`,
`cargo test --test js_source var_declarations_hoist_to_the_function`,
`cargo test --test js_functions top_level_function_declarations_are_var_scoped`):

- `var a, b = 1;` in every statement position (blocks, `if`/loop bodies,
  labelled statements, `try`/`catch`/`finally`, `switch` clauses). One
  binding per name per function body or script, hoisted and `undefined`
  from entry (no temporal dead zone); `var x;` does not reset a value; a
  `var` named like a parameter is the parameter. Closures capture a `var`
  by value or through one cell per call, under the existing rule, so
  closures from different loop iterations share it.
- Function declarations directly in a script or function body are
  var-scoped: they may repeat a parameter, a `var` or each other, and the
  last one wins. Block-level declarations stay lexical.
- Early errors (`Invalid`, at the `var`'s name): a `var` whose name a
  `let`/`const`/block function declares in the same body or an enclosing
  block of the same function (`let a; var a;`, `{ let a; { var a; } }`).
  A `var` repeating a catch parameter is allowed (Annex B), as in QuickJS.
- `tests/fixtures/js/quickjs_closure.js` keeps upstream's `var`s.

Deferred:

- Script-level `var`s and function declarations are bindings of the
  script, not properties of a global object (clj-quickjs-104).
  `var` in a `for` head runs since clj-quickjs-89 (below).

### Ordinary objects and named properties (clj-quickjs-114)

Supported (`cargo test --lib frontend::tests::object_literals`,
`cargo test --test js_source object_literals_and_named_properties`,
`cargo test --test publication publishing_an_object_literal_is_rejected_with_property_path`,
`cargo test --test gc_roots object_property`):

- Object literals `{a: 1, "b": 2, 3: 4, if: 5, x}` (identifier, reserved
  word, string and numeric keys, shorthands, trailing comma, duplicate keys,
  left-to-right evaluation), `o.name` reads and `o.name = v` assignments.
- Representation (decision record in `docs/architecture.md`, "Ordinary
  objects"): task-owned mutable heap objects with own data properties in
  creation order, keyed by runtime-interned property keys, a prototype slot
  and an extensible flag; traced by the collector; rejected by publication
  with a `.property(name)` path; another task's access fails with
  `VmErrorKind::ForeignObject`.
- Reading or writing a property of `undefined`/`null`, and writing one of a
  collection, builder or atom, throw catchable `TypeError`s; reading one of
  a collection gives `undefined`.

Deferred:

- (`Object.prototype` and method calls: clj-quickjs-117, below.)
  Getters/setters, methods, computed keys in literals (clj-quickjs-233;
  `o[k]` is clj-quickjs-115, below; `delete` is clj-quickjs-124, below),
  `__proto__` (property `++`/`--` and compound assignment: clj-quickjs-76,
  -260, above); properties of error objects and
  primitives fail with `UnsupportedOperand` (function properties:
  clj-quickjs-125, below).

### Computed member access (clj-quickjs-115)

Supported (`cargo test --lib frontend::tests::computed_member_access`,
`cargo test --test js_source computed_member_access`,
`cargo test --lib vm::property::tests::computed_keys_convert_by_to_property_key`):

- `o[k]` reads, `o[k] = v` writes and `o[k](...)` method calls in member
  and call chains (`a[b][c]`, `a.b[c].d`, `a[b](c)`), on ordinary objects
  and arrays, with ToPropertyKey of primitive keys: `o[1]` is `o["1"]`,
  `o[-0]` is `o["0"]`, `o[1.5]` is `o["1.5"]`, `o[true]` is `o.true`.
- Evaluation order base, key, right-hand side; an `undefined`/`null` base
  throws a catchable `TypeError`.

Deferred:

- Symbol keys (clj-quickjs-127) fail explicitly; boolean bases fail with
  `UnsupportedOperand` (clj-quickjs-39; numbers and strings read through their prototypes since
  clj-quickjs-73); optional chaining.
  (Compound assignment and `++`/`--` of a property: clj-quickjs-260, -76.)
- Interned property names are never reclaimed, so writing many distinct
  computed keys grows the runtime's key table (clj-quickjs-259;
  `docs/grammar.md`, "Explicit gaps").

### Directive prologues and sloppy compilation (clj-quickjs-95)

Supported (`cargo test --lib frontend::tests::directive_prologues_set_strictness`,
`cargo test --test js_source use_strict_directives`):

- `ScriptOptions { strict }` and the entry points `parse_script_with`,
  `compile_script_with` and `compile_function_with`. `ScriptOptions::SLOPPY`
  compiles non-strict code; `parse_script`, `compile_script` and
  `compile_function` stay strict (`ScriptOptions::STRICT`).
- A `"use strict"` / `'use strict'` directive (exact source text, no escapes)
  in the directive prologue of a script, function or arrow body makes it and
  everything nested in it strict, including a retroactive `ReservedWord`
  check of the function's own name and parameters. Arrows inherit the
  enclosing mode. Directives still evaluate as string statements.
- Sloppy code may bind, read and assign `eval` and `arguments` (parameters, `let`/`const`/`var`,
  function names, catch parameters).
- Each function's mode is recorded in its code: `ExecutableSpec::strict`,
  `Executable::is_strict()` (hand-assembled code is strict).

Deferred:

- Every other sloppy-mode difference (listed in `docs/grammar.md`,
  "Explicit gaps"): references that would denote the global `eval`
  (clj-quickjs-106), sloppy function-name
  stores (clj-quickjs-96), implicit globals (clj-quickjs-105), `with`
  (clj-quickjs-110); strict reserved words and duplicate parameters stay
  rejected in sloppy code.

### Default parameter values (clj-quickjs-100)

Supported (`cargo test --lib frontend::tests::default_parameters`,
`cargo test --test js_functions default_parameters`):

- `function f(a, b = expr) {}`, function expressions and arrows
  (`(a, b = 1, c) => a + c`). A default runs on entry, left to right, only
  when its argument is `undefined` (missing or passed).
- Parameters have a temporal dead zone: a default sees earlier parameters,
  the enclosing scopes and the function's own name; itself or a later
  parameter is a ReferenceError. Closures in defaults capture parameters
  by value or cell under the existing rule.
- With a default, the parameters have their own scope: the body's
  `let`/`const`/`var`/function declarations are invisible to defaults; a
  body `var` repeating a parameter is a separate binding starting with its
  value. A body `let`/`const` repeating a parameter, duplicate parameter
  names and a `"use strict"` directive in the body are `Invalid`.

Deferred:

- `eval` in defaults (clj-quickjs-106/-109; rejected as everywhere).
  An arrow's default sees its lexical
  `this` (clj-quickjs-98); a non-arrow function's default sees its own
  receiver (clj-quickjs-116). Function metadata is supported by clj-quickjs-101, below.

### Legacy octal number literals in sloppy code (clj-quickjs-51)

Supported (`cargo test --lib frontend::tests::legacy_octal_literals_in_sloppy_mode`,
`cargo test --test js_source legacy_octal_literals_in_sloppy_scripts`):

- In sloppy code (`ScriptOptions::SLOPPY`), legacy octal integers (`017` is 15; `01.a` is a member
  access) and leading-zero decimals (`08`, `09.5`, `08e1`). They stay `InvalidToken` in strict code,
  including the literal right after a `"use strict"` directive (`"use strict"; 010`).

### Legacy octal string escapes in sloppy code (clj-quickjs-258)

Supported (`cargo test --lib frontend::tests::legacy_octal_escapes_in_sloppy_mode`,
`cargo test --test js_source legacy_octal_escapes_in_sloppy_scripts`):

- In sloppy code, legacy octal escapes (`"\07"` is U+0007, `"\101"` is `A`, `"\400"` is U+0020 then
  `0`) and `\8` `\9` (the characters `8`, `9`). They stay `InvalidToken` in strict code, including
  the string right after a `"use strict"` directive (`"use strict"; "\07"`) and the directives
  before it in the same prologue (`"\07"; "use strict";`).

### `Object`, `Object.prototype`, `Object.is` and method calls (clj-quickjs-117)

Supported (`cargo test --test js_source object_intrinsics`,
`cargo test --test publication closures_using_builtin_globals_publish`,
`cargo test --test executor two_workers_share_the_realm_intrinsics`,
`cargo test --test gc_roots realm_intrinsics_retain`,
`cargo test --lib realm::`):

- One realm per runtime (decision record in `docs/architecture.md`, "Realm
  intrinsics"): `Object.prototype` and the built-in functions `Object`,
  `Object.is`, `Object.prototype.toString` and `Object.prototype.valueOf`
  are shared, immutable intrinsics, built on first use, the same objects in
  every task and on every worker, publishable, and collector roots
  (`RootCategory::Intrinsic`) while the runtime lives.
- Object literals and `Object()` results inherit from `Object.prototype`;
  reads walk the prototype chain, and a write of an inherited (writable)
  property adds an own one (`o.toString = f`).
- The free name `Object` resolves after lexical scopes (shadowable);
  `typeof Object` is `"function"`.
- `Object(v)` (`undefined`/`null`: a new object; an object: itself),
  `Object.is` (SameValue), `toString` (`"[object Object]"`, and
  `"[object Undefined]"`/`"[object Null]"` for a plain call), `valueOf`.
- Method calls `o.m(...)` evaluate `o` once and pass it as the receiver of
  a built-in function (`call_method`); JavaScript methods are called with
  their arguments.
- New opcodes `get_builtin_global` (`0x52`), `call_method` (`0x53`) and
  `new_object_literal` (`0x54`); new heap kind `Builtin`
  (`ObjectCounts::builtin`).

Deferred:

- Writing any property of a built-in object throws a catchable
  `TypeError` (built-ins are shared and immutable; listed in
  `docs/grammar.md`, "Explicit gaps"); assigning `Object` is a compile-time
  `Unsupported` error until the global object (clj-quickjs-104).
- (`this` inside JavaScript functions: clj-quickjs-116, below.) `new Object()`
  (built-in constructors; `new` on ordinary functions is clj-quickjs-71), wrapper
  objects for `Object(1)` and primitive receivers (clj-quickjs-72),
  `Object.getPrototypeOf` and the other reflection functions.

### Untagged template literals (clj-quickjs-57)

Supported (`cargo test --lib frontend::tests::template_literals`,
`cargo test --test js_source template`):

- `` `text ${expr} text` ``: substitutions are full Expressions (the comma operator included) and
  may hold further templates to any depth within the nesting budget; raw line breaks (CRLF read as
  LF) and the escapes of string literals, except that octal and `\8` `\9` escapes are `InvalidToken`
  in either mode. Each part keeps its cooked and raw text.
- Evaluation is left to right; each substitution is converted with ToString (`"undefined"`, `"null"`,
  `"true"`, Number::toString) by the `to_string` opcode right after it runs, then concatenated.

Deferred:

- Tagged templates and `String.raw` (clj-quickjs-58; `` f`x` `` is `Unsupported`). Object
  substitutions convert with hint string since clj-quickjs-70.

### `this` in method calls (clj-quickjs-116)

Supported (`cargo test --test js_source method_calls_bind_this`,
`cargo test --lib frontend::tests::this_expressions`,
`cargo test --test need a_method_resumes_with_the_same_this_after_a_wait_and_migration`):

- `this` inside non-arrow functions (bodies and default parameters): a
  method call `o.m()`, `(o.m)()` or `a.b.m()` evaluates its base once and
  passes it as `this`; a plain call `f()`, `(0, o.m)()`, `(f = o.m)()`, a
  task's entry function and a resumable native's callback get `undefined`
  (strict mode), so `this.x` there throws a catchable `TypeError`.
- Each frame keeps a task-owned, traced `this` slot that survives yields,
  `__rt.need` waits, collections and migration between workers.
- New opcode `get_this` (`0x56`).

Deferred:

- `this` at the script top level, including arrows without an enclosing
  non-arrow function (the global object, clj-quickjs-104): `Unsupported`
  compile errors. Arrows nested in functions capture `this` (clj-quickjs-98).
- Sloppy-mode `this` substitution: in sloppy code `this` with an
  `undefined`, `null` or primitive receiver fails with
  `UnsupportedOperand` (the global object is clj-quickjs-104, wrappers
  clj-quickjs-72); an object receiver works as in strict code.

### Array literals and array objects (clj-quickjs-118)

Supported (`cargo test --lib frontend::tests::array_literals`,
`cargo test --lib frontend::tests::computed_member_access`,
`cargo test --lib object::array`,
`cargo test --test js_source array_literals`,
`cargo test --test publication publishing_an_array_is_rejected_with_index_path`,
`cargo test --test gc_roots array_element`):

- Array literals `[]`, `[1, 2,]`, `[ , ]`, `[1, , 3]` (holes, one trailing
  comma, left-to-right evaluation); `a.length`; `a[k]` reads and
  `a[k] = v` writes, which extend `length` past the greatest index.
- Representation (decision record in `docs/architecture.md`, "Array
  objects"): a task-owned `Array` heap kind with dense element storage
  (holes as absent entries), a sparse fallback for far indices, a `u32`
  `length` and an embedded ordinary object for named properties; traced
  by the collector; rejected by publication with an `[index]` path step;
  another task's access fails with `VmErrorKind::ForeignObject`.
- Computed member access with primitive keys (ToPropertyKey: `a[1]` is
  `a["1"]`, `a[-0]` is `a["0"]`) on objects and arrays (clj-quickjs-115, above).
- `Array.prototype` (a realm intrinsic inheriting from `Object.prototype`)
  as every array's prototype; `a[k](...)` as a method call.

Deferred:

- Array methods other than `push` (clj-quickjs-155), calling the `Array`
  constructor, and `Array.isArray` (clj-quickjs-39, -153..-156).
  The `Array` global exposes `Array.prototype`; its `toString` and
  constructor still fail when called. Assigning `length` supports
  ArraySetLength (clj-quickjs-154). Spread elements and array destructuring
  (`Unsupported`); object keys convert since clj-quickjs-70.
  `__rt.vector` values stay persistent vectors, not arrays.

### `Object.defineProperty`, `Object.getOwnPropertyDescriptor` and data property attributes (clj-quickjs-119)

Supported (`cargo test --test js_source define_property`,
`cargo test --lib vm::property::tests::inherited_read_only_properties_reject_writes`,
`cargo test --lib vm::descriptor`; decision record
in `docs/architecture.md`, "Property attributes"):

- `Object.defineProperty(o, k, d)` creates or updates an own data property of an ordinary object
  with ECMAScript validation (absent fields default to `undefined`/false on creation and are kept on
  update; a non-configurable property allows only `writable` true to false and SameValue rewrites)
  and returns `o`; ToPropertyDescriptor reads `d`'s fields with `[[Get]]`, inherited ones included.
- `Object.getOwnPropertyDescriptor(o, k)` gives `undefined` or a fresh
  `{value, writable, enumerable, configurable}` object, also for built-in objects' properties and
  for array elements and `length`, whose attributes `defineProperty` also sets (a read-only `length`
  rejects new elements at or beyond it).
- `o.k = v` of an own or inherited read-only property, or on a non-extensible object, throws a
  catchable `TypeError` in strict code and does nothing in sloppy code.

Deferred:

- Accessor descriptors (`get`/`set`) fail with `UnsupportedOperand` (clj-quickjs-120); so do
  object keys
  (ToPrimitive, clj-quickjs-70). Function descriptors work for `length`/`name`
  (clj-quickjs-101) and other own properties (clj-quickjs-125).
  An inherited read-only property can be set up from source with
  `Object.create` since clj-quickjs-121.
  `length` changes on arrays fail explicitly (ArraySetLength, clj-quickjs-39). Built-in objects stay immutable: `defineProperty` on one
  is a `TypeError`.

### Function own properties and lazy `prototype` (clj-quickjs-125)

Supported (`cargo test --test js_source function_own_properties`,
`cargo test --test publication function_own_properties_follow_publication_rule`,
`cargo test --lib value::share::tests::a_function_gaining_properties_during_a_check_is_not_cached`,
`cargo test --test gc_roots function_properties`):

- `f.x = v` / `f.x` on function objects; `F.prototype` of a `function`
  declaration or expression, created on first use (an ordinary object
  with `constructor` = `F`, the same object on every read, writable);
  arrows have no `prototype`. A name a function lacks continues at
  `Function.prototype` (`f.call`, `f.apply`, clj-quickjs-102) and then
  `Object.prototype` (`f.y` is `undefined`).
- Representation (decision record in `docs/architecture.md`, "Function
  own properties"): a task-owned property record attached to the
  persistent function on its first property write or `prototype` read. An
  untouched function publishes; one with a record is rejected at
  `value.properties`; a published function is immutable (a write throws a
  catchable `TypeError`).

Deferred:

- `Function.prototype`'s `toString` fails with `UnsupportedOperand`.
  Function metadata and descriptors are supported (clj-quickjs-101, below).
  `Object.setPrototypeOf(f, p)` with a `p` other than
  `Function.prototype` fails with `UnsupportedOperand`. Creating the `prototype` of a
  published function fails with `UnsupportedOperand`.

### `new` on ordinary function constructors (clj-quickjs-71)

Supported (`cargo test --test js_source new_operator_constructs_objects`,
`cargo test --test js_source a_constructor_suspends_at_a_need_and_completes_after_publication`,
`cargo test --test need a_constructor_resumes_with_its_new_object_after_a_wait_and_migration`,
`cargo test --lib frontend::tests::new_expressions`):

- `new F(args)` and `new F` (`new MemberExpression Arguments?`: `new a.b(c)`,
  `new F()()` calls the result, `new new F()()`) on a `function`
  declaration or expression: a new task-owned ordinary object inheriting
  from `F.prototype` (or `Object.prototype` when that is not an object)
  is `F`'s `this`; an object returned by `F` replaces it, any other return
  value is ignored.
- Anything else (`new 1`, an arrow function, a built-in such as `Object`,
  a collection, a plain object) is a catchable `TypeError` "… is not a
  constructor" at the `new` token, after the callee and the arguments are
  evaluated.
- The constructor runs in an ordinary task-owned frame marked as a
  construct frame, so it may yield, wait on `__rt.need`, be collected
  around and migrate between workers.
- New opcode `construct` (`0x5E`, u16 argument count).

Deferred:

- `new.target` (clj-quickjs-99, `Unsupported`), classes and `super`
  (clj-quickjs-135..-139), built-in constructors such as `new Object()`,
  `new Number(1)`, `new String("abc")` (clj-quickjs-72), `instanceof`
  (clj-quickjs-75), spread arguments.
- `new` on a published function whose `prototype` was never created fails
  with `UnsupportedOperand`, as reading that `prototype` does.

### `Object.getPrototypeOf`, `Object.setPrototypeOf`, `Object.create` and `__proto__` (clj-quickjs-121)

Supported (`cargo test --test js_source prototype_reflection`, `cargo test --lib vm::prototype`,
`cargo test --lib frontend::tests::object_literals`; decision record in
`docs/architecture.md`, "Prototype reflection"):

- `Object.getPrototypeOf(v)` of objects and arrays (`Object.getPrototypeOf([])` is
  `Array.prototype`, whose prototype is `Object.prototype`, whose prototype is `null`); values
  without JavaScript properties (`__rt.vector(...)`, ...) give `null`.
- `Object.setPrototypeOf(o, proto)` on the running task's objects and arrays, returning `o`
  (a primitive `o` is returned unchanged); a cycle, a `proto` that is neither an object nor
  `null`, an `undefined`/`null` `o`, a built-in object or a non-extensible `o` throw a catchable
  `TypeError`. Setting the prototype an object already has always succeeds.
- `Object.create(proto)` with an object or `null`.
- `o.__proto__` reads and `o.__proto__ = p` writes reach `Object.prototype`'s `__proto__`
  accessor (a non-object, non-null `p` is ignored); an object without `Object.prototype` on its
  chain has an ordinary `__proto__` property.
- `{__proto__: p}` and `{"__proto__": p}` in a literal set the prototype (non-object, non-null
  values are ignored, a second one is an early `SyntaxError`, kind `Invalid`); the shorthand
  `{__proto__}` defines an own property. Lowered to the appended `set_proto` opcode (`0x5F`).
- A prototype link never points at another task's object (`ForeignObject`).

Deferred:

- `Object.prototype.__proto__` is applied by the property walk, not stored:
  `Object.getOwnPropertyDescriptor(Object.prototype, "__proto__")` fails with
  `UnsupportedOperand` until it becomes a stored accessor (clj-quickjs-120, then clj-quickjs-261);
  so do the prototypes of primitives (wrappers, clj-quickjs-72) and error objects
  (clj-quickjs-230), changing a function's prototype, and
  `Object.create`'s second argument (`Object.defineProperties`). Extensibility (clj-quickjs-122) and
  `Object.getOwnPropertyNames` (clj-quickjs-123) have their own sections below.

### `Object.preventExtensions`, `isExtensible`, `freeze` and `isFrozen` (clj-quickjs-122)

Supported (`cargo test --test js_source object_extensibility`, `cargo test --test publication
freezing_keeps_ownership_and_published_functions_are_frozen`, `cargo test --lib object::`;
decision record in `docs/architecture.md`, "Extensibility and freezing"):

- `Object.preventExtensions(o)` and `Object.isExtensible(o)` on the running task's objects,
  arrays and functions; adding a property to a non-extensible object is a catchable `TypeError`
  in strict code and ignored in sloppy code, `Object.defineProperty` of a new key and
  `Object.setPrototypeOf` to another prototype are `TypeError`s; existing properties stay
  writable and configurable.
- `Object.freeze(o)` and `Object.isFrozen(o)`: every own property (array elements and `length`
  included) becomes non-writable and non-configurable; freezing an array allocates nothing per
  element.
- Primitives are not extensible and frozen, and are returned unchanged. Vectors, maps and other
  values without JavaScript properties, and published functions, report non-extensible and
  frozen. Built-in objects report their attributes (`isExtensible(Object.prototype)` is `true`)
  and changing them is a `TypeError`.

Deferred:

- Freezing does not change ownership: a frozen object stays task-owned and cannot be published,
  and freezing a function makes it unpublishable (it gets a property record).
- `Object.seal`,
  `Object.isSealed` and `Reflect`'s extensibility functions are not provided; error objects fail with `UnsupportedOperand`
  (clj-quickjs-230); accessor properties are clj-quickjs-120.

### `Object.keys` and `Object.getOwnPropertyNames` (clj-quickjs-123)

Supported (`cargo test --test js_source own_property_key_order`, `cargo test --lib vm::keys`;
decision record in `docs/architecture.md`, "Own-property keys"):

- `Object.keys(o)` (enumerable keys) and `Object.getOwnPropertyNames(o)` (all string keys) return
  a fresh array in ECMAScript order: array indices (`0 ..= 2^32 - 2`) ascending, then the other
  names in creation order (an array's `length` first among them); holes and inherited properties
  are not listed. Works on the running task's objects and arrays, embedder objects, built-in
  objects and functions, including ordinary function objects; values without JavaScript
  properties list nothing. `undefined`/`null` throw a catchable `TypeError`.
- The order is one helper (`vm::keys::own_property_keys`) for `for`-`in` (clj-quickjs-85) and
  spread/rest copies to reuse.

Deferred:

- Primitive arguments (wrapper objects, clj-quickjs-72), error objects (clj-quickjs-230), and
  `getOwnPropertyNames` of `Object.prototype` (unstored `__proto__`, clj-quickjs-261)
  fail with `UnsupportedOperand`.
  Symbol keys (clj-quickjs-127), `Object.getOwnPropertySymbols`, `Reflect.ownKeys`,
  `Object.entries`/`values`.

### `instanceof` (clj-quickjs-75)

Supported (`cargo test --test js_source instanceof_operator`,
`cargo test --lib frontend::tests::binary_precedence_and_left_associativity`):

- `v instanceof F` at relational precedence, lowered to the appended `instanceof` opcode
  (`0x60`): a non-object or non-callable `F` is a catchable `TypeError`; a primitive `v` is
  `false`; otherwise `F.prototype` must be an object (`TypeError`) and the result is whether it
  is on `v`'s prototype chain (ordinary functions, `Object`, `[].constructor`, chains built with
  `new`, `Object.create`, `Object.setPrototypeOf` or `G.prototype = new F()`).

Deferred:

- An error object as `v` fails with `UnsupportedOperand` until `Error.prototype`
  (clj-quickjs-230) exists (functions inherit from `Function.prototype` since
  clj-quickjs-102: `f instanceof Function`); `String` and the Error constructors are not globals yet
  (clj-quickjs-72, the upstream corpus pellets). `Symbol.hasInstance` (symbols) and bound
  functions (clj-quickjs-103) are not consulted.

### The `delete` operator (clj-quickjs-124)

Supported (`cargo test --test js_source delete_property_references`,
`cargo test --lib frontend::tests::delete_of_identifiers_is_rejected`; decision record in
`docs/architecture.md`, "The `delete` operator"):

- `delete o.p` and `delete o[k]`, lowered to the appended `delete` opcode (`0x61`): an
  `undefined`/`null` base is a catchable `TypeError`; an own configurable property is removed
  (the rest keep their creation order; an array element becomes a hole and `length` stays) and
  the result is `true`, as for an absent property; a non-configurable one (frozen objects, an
  array's `length`, a constructor's `prototype`, `defineProperty` without `configurable`) stays
  and is a `TypeError` in strict code, `false` in sloppy code. String bases act as their wrapper
  (`delete "abc"[0]` is a `TypeError`, `delete "abc"[100]` is `true`). `delete` of any other
  non-reference expression evaluates it and gives `true`.
- `delete x` of an identifier is an early `SyntaxError` (`Invalid`) in strict code.

Deferred:

- `delete x` in sloppy code (implicit globals, clj-quickjs-105) is `Unsupported`;
  `delete super.x` (clj-quickjs-138) and `delete a?.b` (clj-quickjs-134) wait for those forms;
  properties of error objects
  (clj-quickjs-230) fail with `UnsupportedOperand`. Built-in objects are immutable: deleting a
  property they have is a `TypeError` in both modes.

### `in` (clj-quickjs-74)

Supported (`cargo test --test js_source in_operator`,
`cargo test --lib frontend::tests::binary_precedence_and_left_associativity`,
`cargo test --lib frontend::tests::for_statements`):

- `k in o` at relational precedence, lowered to the appended `in` opcode (`0x62`): an `o` that
  is not an object is a catchable `TypeError`, checked before `k` is converted; then
  ToPropertyKey(`k`) as for `o[k]` and HasProperty through the prototype chain (own and inherited
  properties of objects, arrays, functions and `Object.prototype`; `'__proto__' in {}` is true).
  Persistent collections, builders and other `__rt` values have no JavaScript properties.
- The init of a `for` head takes no top-level `in` (`[~In]`): `for (a.b in o)` and
  `for (let x = a in b; ;)` are for-in heads (clj-quickjs-85: a property target is `Unsupported`, an initializer `Invalid`), while
  `for (x = (a in b); ;)` and an `in` in the test or update parse as the operator.

Deferred:

- Names `Function.prototype` does not have yet
  (`toString`; `call`/`apply`/`bind` are supported) and error objects' properties (clj-quickjs-230) fail
  with `UnsupportedOperand`; symbol keys (clj-quickjs-127) do not exist.

### `for`-`in` over own properties (clj-quickjs-85)

Supported (`cargo test --lib frontend::tests::for_in_heads`,
`cargo test --test js_source for_in_over_own_properties`,
`cargo test --test gc_roots for_in`; decision record in
`docs/architecture.md`, "`for`-`in`"):

- `for (k in o)`, `for (let k in o)`, `for (const k in o)`: the body runs
  once per own enumerable string key of `o`, snapshotted at the start in
  `Object.keys` order (array indices ascending, then creation order); a key
  deleted or made non-enumerable before it is reached is skipped, keys added
  during the loop are not visited. `null`, `undefined`, numbers and booleans
  run zero iterations; a string visits its indices.
- A `let`/`const` head is in its temporal dead zone in the object expression
  (`for (let k in k)` is a ReferenceError) and a fresh binding per iteration
  (closures keep their key). `break`, `continue`, labels, `return`, exceptions
  and finally blocks work as in other loops; completion values follow
  `UpdateEmpty`. The upstream `test_for_in` first group and `test_for_in2` run
  from source (adapted, with notice).
- The enumerator (appended opcodes `for_in_start` `0x63`, `for_in_next` `0x64`)
  is task-owned frame state: a yield, wait, collection or migration inside the
  loop resumes it exactly.
- The head is parsed with `[~In]`: `for (let k = 1 in o)`, `for (let a, b in o)`
  and `for (1 in o)` are early errors (`Invalid`).

Deferred:

- Property targets `for (a.b in o)` (clj-quickjs-87, `Unsupported`), the
  non-strict `for (var k = 1 in o)` (clj-quickjs-90), Proxy traps
  (clj-quickjs-91). (`var` heads: clj-quickjs-89; inherited properties:
  clj-quickjs-86; arrays: clj-quickjs-88, all below.)

### `for`-`in` over the prototype chain (clj-quickjs-86)

Supported (`cargo test --test js_source for_in_walks_the_prototype_chain`,
`cargo test --lib vm::keys::`, `cargo test --test gc_roots for_in`; details
in `src/vm/keys.rs`, "Prototype chain"):

- After `o`'s own keys, `for (k in o)` visits each prototype's enumerable
  string keys, nearest first, each prototype snapshotted when the walk
  reaches it. A name is visited at most once: an own or nearer property of
  that name, enumerable or not, hides an inherited one (QuickJS
  `test_for_in`'s second and third groups run from source, adapted, with
  notice). A prototype key deleted or made non-enumerable before it is
  reached is skipped; when the chain changes, the walk continues on the
  prototype it is on.
- The walk ends at a `null` prototype, at `Object.prototype` and the other
  realm intrinsics (shared and immutable, with no enumerable properties:
  decided from the realm's views without taking a lock), and after a
  function. The enumerator traces the prototype it is on, so a prototype
  unlinked during the loop survives waits, collections and migration until
  the walk leaves it.

Deferred: an error object on the chain fails with `UnsupportedOperand` (its
own keys are clj-quickjs-230); Proxy traps (clj-quickjs-91) as above.

### `for`-`in` over arrays (clj-quickjs-88)

Supported (`cargo test --test js_source for_in_over_arrays`; regression
tests only, no code change: arrays use the generic enumerator):

- `for (k in a)` on an array visits its element indices as strings
  (`typeof k` is `"string"`) in ascending numeric order, holes skipped
  (elisions, deleted elements, sparse gaps up to index 2^32 - 2), then its
  named properties in creation order (`"01"`, `"-1"`, `"1.5"` and
  `"4294967295"` are names, not indices), never `length`. QuickJS
  `test_for_in`'s fourth group ("array optimization") runs from source,
  adapted, with notice.
- An element removed before it is reached is not visited; elements added
  during the loop are not. Shrinking with `a.length = n` during iteration
  skips removed indices in dense and sparse arrays; named properties
  remain enumerable even after shrinking to zero.

Deferred: shrinking with `a.pop()` (not implemented), and the upstream
group's original `push` / `toString` form (clj-quickjs-264, after
clj-quickjs-155 and -156). The test appends with `a[a.length] = i` and
collects keys with string `+` meanwhile.

### `var` heads of `for` and `for`-`in` statements (clj-quickjs-89)

Supported (`cargo test --lib frontend::tests::for_var_heads`,
`cargo test --test js_source var_heads_in_for_loops`):

- `for (var i = 0, j; test; update)` and `for (var k in o)`: the names are
  `var` bindings of the enclosing function or script (hoisted, `undefined`
  before the loop, the last value after it), with no per-iteration copy, so
  closures created in the body share one binding. A for-in `var` head is
  assigned each key, as `for (k in o)` is. Upstream `test_for`'s second
  group and `test_for_in`'s seventh group run from source (the latter
  adapted, with notice).
- A declarator's initializer in a `for` head is parsed with `[~In]`, so
  `for (var k = 1 in o)` and `for (var a, b in o)` are strict-mode early
  errors (`Invalid`), as for `let`/`const` heads. A `var` head repeating a
  `let`/`const` name of the script, the function or an enclosing block is
  `Invalid`, as for a `var` statement.

Deferred:

- The non-strict `for (var k = 1 in o)` (Annex B) is clj-quickjs-90;
  `for (var k of o)` stays `Unsupported` with for-of.

### Properties and methods of number and string primitives (clj-quickjs-73)

Supported (`cargo test --test js_source methods_on_primitive_bases`,
`cargo test --test js_source methods_on_primitive_bases_allocate_no_wrappers`):

- `n.x`, `s[k]`, `(1).toString()`, `"abc".valueOf()`: a read on a number or string looks the
  key up from `Number.prototype` / `String.prototype` through `Object.prototype`, without
  allocating a wrapper; a method call passes the primitive itself as `this`.
- `Number.prototype.toString()` (radix 10), `Number.prototype.valueOf()`,
  `String.prototype.toString()`, `String.prototype.valueOf()`; any other receiver is a
  catchable `TypeError`. `Number` and `String` are built-in globals carrying the prototypes
  (`(1).constructor === Number`).
- Writes to a primitive base are a catchable `TypeError` in strict code and ignored in sloppy
  code (`__proto__` runs its setter, which ignores a primitive); `undefined`/`null` bases
  throw catchable `TypeError`s for reads, writes and method calls.

Deferred:

- Boolean bases (`Boolean.prototype`), radixes other
  than 10 and the other `Number.prototype` / `String.prototype` methods (clj-quickjs-39) fail
  with `UnsupportedOperand` (an absent method reads `undefined`).

### `Function.prototype`, `call`, `apply` and `Reflect.apply` (clj-quickjs-102)

Supported (`cargo test --test js_functions call_apply_and_reflect_apply`,
`cargo test --test js_functions forwarded_calls_wait_and_resume_on_another_thread`; decision
record in `docs/architecture.md`, "Forwarding calls"):

- `Function.prototype`, a realm intrinsic: callable (returns `undefined`), the prototype of every
  function, arrow and built-in function (`Object.getPrototypeOf(f) === Function.prototype`,
  `f instanceof Function`), inheriting from `Object.prototype`; its `constructor` is the new
  built-in global `Function`.
- `f.call(t, ...args)`, `f.apply(t, list)` and `Reflect.apply(f, t, list)` (the new built-in
  global `Reflect`): call `f` with `this` `t`; `apply` takes no arguments for an
  `undefined`/`null` list and otherwise reads an array or array-like object
  (CreateListFromArrayLike). They are tail calls: the target runs in a task-owned frame that
  may yield, wait, be collected around and migrate. A non-callable target and a primitive list
  (any non-object list for `Reflect.apply`) are catchable `TypeError`s.

Deferred:

- `Function(...)`/`new Function` (clj-quickjs-111: `UnsupportedOperand`/`TypeError`),
  `toString`, `arguments`/`caller` (their names fail with
  `UnsupportedOperand`), the other `Reflect` functions, changing a function's prototype, and the
  sloppy-mode `this` substitution of a callee (an explicit `UnsupportedOperand`, as for `this`).

### Bound functions (clj-quickjs-103)

Supported (`cargo test --test js_functions`, `cargo test --test gc_roots`):
`Function.prototype.bind` creates a traced callable with a fixed receiver and
prepended arguments. Repeated binding composes; `call`, `apply`, native
callbacks, construction and `instanceof` follow the bound target. Callees may
wait, yield, be collected around and resume on another worker. Bound functions
have `length` and `name`, ordinary own-property operations, and no initial own
`prototype`. They publish across tasks when their target, receiver and arguments
are shareable and no mutable property record has been added. Bound arguments
and forwarding hops obey the VM's stack and depth limits.

Binding snapshots the target's current own numeric length, subtracting bound
arguments and clamping at zero, and prefixes its current string name with
`bound ` (clj-quickjs-101). `new.target` syntax remains clj-quickjs-99.

### Function `length` and `name` (clj-quickjs-101)

Supported (`cargo test --test js_functions function_length_and_name`,
`cargo test --test js_functions function_metadata_property_operations`):

- Every function owns non-writable, non-enumerable, configurable `length`
  and `name`. Length counts parameters before the first default. Names are
  declared names, inferred names for anonymous functions/arrows directly in
  binding initializers, identifier assignments, parameter defaults and object
  literal property values, or the empty string otherwise.
- Reads, descriptors, `in` and own-key enumeration preserve a record-free
  function's publication eligibility. Constructors initially list `length`,
  `name`, `prototype`; arrows and bound functions list `length`, `name`.
- The first property change copies metadata and materializes a constructor's
  prototype into its task-owned record. Deletion, redefinition and freezing
  use ordinary descriptors. Deleted metadata exposes `Function.prototype`'s
  0/empty-string fallback. Published functions are immutable and report
  non-configurable metadata descriptors without attaching a record.

Deferred: rest and destructuring parameters, method shorthand, and accessors;
their syntax remains rejected. See `docs/architecture.md`, "Function own properties".

### `Array.prototype.push` (clj-quickjs-155)

Supported (`cargo test --test js_source array_push`): `a.push(...items)`
appends, sets and returns length, including zero items; generic array-like
receivers through `Array.prototype.push.call(o, ...)`, primitive ToLength,
the `2^53 - 1` guard, and throwing Sets even in sloppy code. Dense appends
are amortized O(1). Frozen/non-extensible arrays and read-only lengths
reject invalid writes; another task's array fails with `ForeignObject`.
The `Array` global exposes the existing constructor stub and its prototype;
`Array.prototype.push.length` is 1.

Deferred: primitive receiver wrappers and object-valued length conversion
fail explicitly; array construction and other array methods retain their
existing gaps. The for-in adaptation clj-quickjs-264 also needs array
string conversion and stays deferred.

### Object operands and OrdinaryToPrimitive (clj-quickjs-70)

Supported (`cargo test --test js_source objects_convert_with_to_primitive`,
`cargo test --test js_source object_conversions_cover_hints_operators_and_property_keys`,
`cargo test --test need to_primitive`, `cargo test --test exception_waits to_primitive`):

- Object operands in arithmetic, relational, bitwise, unary, update and
  loose equality operators call `valueOf` then `toString`; templates and
  computed property keys reverse that order. Each callable method receives
  the original object as `this` and no arguments. The first primitive wins;
  otherwise a catchable TypeError is thrown. Left conversion finishes first.
- Ordinary objects inherit defaults: `({}) == '[object Object]'` is true,
  `'' + {}` is `'[object Object]'`, and a numeric conversion gives NaN.
  Two objects compare by identity; equality with null/undefined never converts.
- Task-owned native continuations preserve conversion state across budget
  yields, `__rt.need` waits, GC and migration. Exceptions propagate and
  cancellation runs finally blocks without restarting a conversion.

Deferred: Symbol.toPrimitive; Function.prototype.toString and
Array.prototype.join. Functions use the available Object.prototype default (`"[object Function]"`);
runtime collections with no conversion methods throw TypeError. Leaf built-ins
still reject object conversion arguments (for example descriptor keys,
array lengths and apply's argument-list length); these require their own
resumable built-in paths.

### Number and String wrapper constructors (clj-quickjs-72)

Supported (`cargo test --test js_source primitive_wrapper`):

- `Number(v)` / `String(v)` convert arguments using ToNumber /
  ToString; no argument gives 0 / `""`.
- `new Number(v)` / `new String(v)` allocate distinct task-owned objects
  holding `[[NumberData]]` / `[[StringData]]`, inheriting from the matching
  prototype. Their `valueOf()` / `toString()` return the stored primitive;
  an incompatible receiver throws a catchable TypeError. Ordinary own
  properties, prototype changes, freezing, identity and `instanceof` work.
- Wrapper data and own properties survive waits, collection and migration.
  Publication rejects wrappers; another task cannot read or mutate them.

Object arguments use resumable OrdinaryToPrimitive with number / string hint.
Wrapper equality and addition call observable valueOf / toString methods;
conversion callbacks can yield, wait, be collected around and migrate.

Deferred: Boolean wrappers, other radixes and Number constants
remain deferred. ToObject integration for `Object(primitive)`, prototype reflection
on primitives and sloppy-mode `this` remains unsupported.

### Lexical `this` in arrows (clj-quickjs-98)

Supported (`cargo test --test js_functions arrows_capture_this`,
`cargo test --test need an_arrow_retains_lexical_this_after_a_wait_collection_and_migration`):

- An arrow's body and default parameters capture the nearest enclosing
  ordinary function's `this` by value, through any intervening arrows.
  The capture survives its enclosing call, collection, waits and migration.
- Plain and method calls, `call`, `apply`, `Reflect.apply` and `bind`
  preserve that receiver. Ordinary functions nested inside arrows have
  their own `this`; `new` on an arrow remains a catchable TypeError.
- The capture uses no binding cell. Publication checks its value normally:
  shareable receivers permit publication, task-owned object receivers do not.
- Script-level arrows still reject `this` until the global object exists
  (clj-quickjs-104). Sloppy receiver substitution retains its existing gap.

### String length and index properties (clj-quickjs-142)

Supported (`cargo test --test js_source string_length_and_index_properties`,
`cargo test --test js_source string_index_reads_allocate_no_wrappers`):

- `s.length` is the number of UTF-16 code units (`"\u{10ffff}".length` is 2) and `s[k]` for
  a canonical index `k` below it is the one-code-unit string there (`"abc"[1]`, `"abc"["1"]`;
  `"\u{10ffff}"[0] === "\u{dbff}"`); other keys (`"abc"[3]`, `[-1]`, `[1.5]`) continue at
  `String.prototype`. Constant-time reads, no wrapper allocated.
- The same own properties for `Object.getOwnPropertyDescriptor` (indices read-only,
  enumerable, non-configurable; `length` also non-enumerable), `Object.keys` /
  `Object.getOwnPropertyNames` of a primitive (`["0", "1", "length"]` for `"ab"`; none for a
  number or boolean), writes (strict `TypeError`, sloppy no-op) and `delete`.
- String wrappers (`new String("ab")`) expose the same virtual own properties over
  `[[StringData]]`. Compatible `Object.defineProperty` calls succeed; incompatible
  values or attributes throw TypeError. Other properties use ordinary storage, with
  every index sorted before `length`, then other names in creation order.

### Arguments objects (clj-quickjs-97)

Ordinary functions that reference `arguments` directly or through arrows
create an unmapped ordinary object with `Object.prototype`, all actual values
(including extra arguments), enumerable writable configurable indices, and a
non-enumerable writable configurable `length`. Parameter defaults see original
actual values; arrows capture the nearest enclosing binding. A parameter,
function or lexical declaration can shadow the binding in sloppy code; a plain
`var arguments;` preserves it. At script level an unresolved `arguments` throws
ReferenceError. Objects and captures are task-owned, traced across yields,
waits, GC and migration, and rejected by publication. Functions without a
reference retain neither the object nor extra arguments.

Deferred: sloppy parameter aliasing, `callee`'s restricted accessor, the
arguments brand for `Object.prototype.toString`, and `Symbol.iterator`
(symbols are clj-quickjs-127). Verify with `cargo test --test js_functions arguments_`.
