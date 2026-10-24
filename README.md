# clj-quickjs

A Rust-native JavaScript runtime for highly concurrent compile-time execution.
Requirements: [`docs/design.md`](docs/design.md). Backlog: Pellets (`pl list`).

This is an architectural slice in progress, not a finished JavaScript engine.
No JavaScript source frontend exists yet.

## Build and test

The toolchain comes from `flake.nix` (`nix develop` if `cargo` is not on PATH).

```sh
cargo build --all-targets
cargo test --all-targets          # unit + integration tests
cargo test --doc                  # doc tests, including compile-fail handle tests
cargo run                         # early VM test program (hand-assembled bytecode)
cargo run --example resumable_counter   # budgeted yield/resume with a host counter
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
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Crate layout

Library (`src/lib.rs`) plus a thin binary (`src/main.rs`). Modules:
`runtime` (embedding entry point, `collect`), `task` (task state, execution
guard, run outcomes), `vm` (instruction format, executable validation,
task-owned frames, dispatch, test assembler), `heap` (object identity, roots,
scoped and weak handles, tracing contract, mark/sweep collector), `value`,
`ids`, `error`, `host` (leaf host effects), `collections` (persistent
vectors, maps and occurrences), `dep` (dependency keys, single-assignment
cells, read-or-subscribe, per-task dependency log). See
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
  Hand-assembled bytecode: an early VM test, not JavaScript source support.

Deferred:

- `Waiting(key)` and dependency waits in the VM (pellets clj-quickjs-23, -7;
  the cells themselves: clj-quickjs-6 below).
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

- Collection while tasks run: `collect` refuses with
  `CollectError::MutatorActive` (it never waits) while any `RunGuard` exists
  or a scope is open on another thread. Cooperative safepoints and mutator
  registration are pellet clj-quickjs-17; executor integration is
  clj-quickjs-25; per-root-kind coverage is clj-quickjs-31.
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
  plain objects: never shareable), and a reserved explicit-concurrent-
  reference category for atoms and dependency cells (shareable without
  being immutable; no kind uses it yet). Primitives are shareable.
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
- No VM intrinsic for the check; dependency results (clj-quickjs-6), atom
  contents (clj-quickjs-10) and closure captures (clj-quickjs-20) will call
  it at their handoffs.

### Dependency cells (clj-quickjs-6)

Supported:

- Typed identities: `DependencyKey` = world (`Runtime::world`,
  `Runtime::new_world`; each task runs in one world, `Runtime::spawn_in`) +
  host-assigned `DependencyId` + host-assigned `ResultKind`. Different
  worlds or result kinds are different cells. `ProducerId` per claim; the
  consumer and wake handle is the `TaskId`.
- Single-assignment cells with immutable outcomes `Ready(value)`,
  `Failed(error)`, `Cancelled`. `Runtime::complete(key, producer, ..)`
  accepts only the claimed producer, only once (`NotProducer`,
  `AlreadyComplete`); values and errors must pass the publication check
  (`NotShareable`). It returns the waiting tasks' ids for the caller to
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

- The `__rt.need` VM operation and `RunOutcome::Waiting` (clj-quickjs-23);
  needs are issued from Rust through a run guard for now.
- Task parking state machine, deterministic handoff-window tests and an
  atomic ready fast path (clj-quickjs-7); consumer and cell cancellation
  other than a producer completing with `Cancelled`; deadlock reporting;
  invalidation or removal of cells (they live as long as the runtime).
