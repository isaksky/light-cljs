# Compiling on one shared heap: the big-boy-pants-js strategy

Status, October 25, 2026: strategy, not a delivered contract. The engine is in
the workspace at [`crates/big-boy-pants-js`](../crates/big-boy-pants-js/README.md)
(bbp below). The compiler does not run macros in it yet. The integration work
is queued in the Pellets group `bbp-integration` (light-cljs-462 to 481). This
document gives the reasons for that queue, the architecture it should arrive
at, and the conditions under which the bet pays off. Measurements are from
`298325f8` on the 24-core Mac (Apple M3 Ultra VM). They are listed under
"Method and reproduction".

## Summary

Each light-cljs compile runs a large amount of JavaScript: user macros,
helpers they demand, and the compile-time `cljs.core`. V8 and QuickJS allow
one thread per heap, so using more cores meant more heaps.
[Stage A](compiler-concurrency.md) did that: an isolate per worker, with
forms copied across a bridge and every helper re-initialized in each isolate.
It works, and it hit a ceiling built into its design. At eight jobs, Penpot
`app.main` takes 0.54x the wall time of `--jobs 1` and 1.64x its memory.
Units cost 32% more on workers than serially, and helper initializations
rise from 277 to 1,088. A macro's compile-time effect is visible only in the
isolate that made it.

bbp removes that design constraint. It is one runtime with one heap and many
lightweight, resumable tasks on any number of OS threads, following
Clojure's concurrency model:

- Immutable persistent values are shared by every task and by Rust, without
  copying.
- Mutable JavaScript objects belong to one task.
- Shared change goes through explicit atoms and single-assignment dependency
  cells.
- A task that needs a result nobody has produced yet parks at that
  instruction, frees its worker and resumes later, possibly on another
  worker, without replaying anything.

These are the operations a compiler needs. Forms are immutable values. Helpers
are results that are produced once and read many times. Compile-time state
is a small number of atoms. Demand between units is a dependency graph. In
the end state the whole compile is one demand-driven dataflow graph over one
heap. There is no bridge, no per-worker runtime, no repeated helper state and
no isolate boundary in the semantics.

The vision is sound, and the parts of the engine that implement it are
proven in its own tests. Two measured facts decide the plan:

1. **Today the engine is very slow on one thread.** It takes about 30 to 45
   times as long as V8 with its JIT turned off on small JavaScript kernels.
   Compared with V8 as the compiler actually runs it, the gap is roughly two
   orders of magnitude. Parallelism cannot hide this, because the longest
   chain of dependent work runs on one thread.
2. **Today the engine does not scale on object-heavy code.** Every object
   allocation and property access takes one global mutex on the object
   table. Arithmetic scales 7.3x on eight threads. An allocate-and-traverse
   kernel gets *0.37x* the throughput of one thread, and 80% of its samples
   are waiting on that mutex.

Neither fact contradicts the design. The engine's data model is what allows
lock-free access: shared objects are immutable, and mutable objects have
exactly one owner. The heap does not use that guarantee yet. The strategy is
therefore to treat "fast and scalable on one heap" as the first engine
milestone, alongside the semantic integration already queued, rather than
deferring it to tuning at the end.

| Phase | What it delivers | Gate |
| --- | --- | --- |
| 0. Engine hot path | Task-owned objects accessed without the global lock; exotic-kind tags; allocation buffers; shapes and inline caches | Alloc kernel scales ≥ 6x at 8 threads; single-thread kernels within 3x of `node --jitless` |
| 1. Serial real macros (465, 466, 462) | Real runtime boots; native forms; real `defmacro`s run in bbp with no legacy engine | Expansions and output match; subtree identity asserted |
| 2. Resumable demand (463) | Helpers become dependency cells; a macro parks and resumes; the existing pool drives tasks | Determinism at `--jobs 1/2/4`; one producer per helper |
| 3. Native compile-time core | Compile-time collections, keywords, protocols and lazy seqs on bbp intrinsics, shareable by construction | Most published helper values are shareable; measured per-thread factor on Penpot units |
| 4. One-heap parallel build | Namespaces and units as tasks over one runtime; ordering from `need`, not up-front scheduling; source-order effect lanes | Penpot output identical at every N; RSS ~1x; wall versus Stage A |
| 5. Worlds for watch and REPL | A retained runtime with versioned worlds; precise invalidation from recorded edges | Watch rebuilds touch only dependents |

Phases 0 and 1 can run in parallel. Phase 0 is engine-only and can be
measured with synthetic kernels shaped like compiler output.

## Why per-isolate parallelism has a ceiling

The numbers come from [compiler-concurrency.md](compiler-concurrency.md),
"Gates and first steps", light-cljs-343 and -391. They are Penpot `app.main`
results with `--optimize none`.

| Cost | Where it comes from | Measured |
| --- | --- | --- |
| Bridge | Forms, `&env`, results and the frontier are copied between Rust and each isolate | ~4.8 s of the 12.7 s of sampled V8 self time (import 1.45, env 1.03, export 0.97, frontier 0.75, arguments 0.65) |
| Repeated helper state | Each isolate compiles and initializes the helpers it touches | 1,088 helper initializations at eight jobs against 277 serially |
| Unit inflation | Cold isolates, repeated initialization and JIT warm-up per isolate | Worker units 21.1 s against 16.0 s serially (+32%) |
| Memory | One heap, runtime and helper set per isolate | 1.64x peak RSS at eight jobs, 2.0x at twelve |
| Fixed start-up | Isolate boot | ~50 ms and ~80 MB for a one-namespace program |
| Semantics | Compile-time effects stay in the isolate that made them | Needs `--cache-blocker` declarations; undeclared effects diverge silently (`1 1` instead of `1 2`) |

Each row exists *because* there are separate heaps. Better scheduling cannot
remove them. On one heap they go to zero, or become a single cost paid once
per build.

## What the engine provides, mapped to compiler problems

| Compiler problem | bbp mechanism | Where |
| --- | --- | --- |
| Forms copied in and out of the engine | Persistent vectors, maps and occurrences readable by Rust and by every task; unchanged subtrees keep their identity | `src/collections.rs`, `Runtime::publish`; light-cljs-466 adds Clojure equality, keywords, symbols and metadata |
| Helpers re-initialized per isolate | One immutable `Executable` per helper shared by all tasks; a helper's value is published once in a dependency cell | `src/dep.rs`; `__rt.need` |
| Synchronous re-entrant `__lightCompileUnit` | A task parks at a `need` and is woken when the producer completes, with frames, handlers, `finally` state and native continuations saved | `src/task.rs`, `docs/architecture.md`, "Dependencies, notifications and cancellation" |
| Cross-isolate invisibility of compile-time atoms | Atoms are shared references with CAS and single-writer publication; readers keep valid old values | `src/atom.rs` |
| Cycle detection across workers | Recorded edges are kept separate from active waits; only real wait cycles are reported | `tests/sim_host.rs` |
| Cache and watch invalidation | Every read is recorded as an edge, including ready reads; results are versioned by world | `DependencyKey` (world, query, kind) |
| Thread-pool ownership | The engine owns no pool; the compiler's `src/scheduler.rs` drives `Task::run_with` | `docs/architecture.md`, "Embedding" |
| Per-platform V8 builds, 72 MB binary | Pure Rust; the engine binary is 5.8 MB | light-cljs-308 (deferred) becomes moot |

The engine's tests prove these mechanisms on synthetic fixtures: two
consumers sharing a Rust-built form, parking on one helper, migrating,
collecting while parked and never replaying effects (`tests/e2e.rs`). The
integration pellets must prove them on real compiler output.

## Target architecture

### One runtime per compiler session

A compile owns one `Runtime`. The realm, the compile-time `cljs.core` and
every compiled helper `Executable` exist once. Workers are the compiler's
existing pool threads, registered with the runtime. A worker has no engine
state of its own. It runs whichever task its job names. Retained sessions
(`--watch`, `--repl`) keep the runtime between rebuilds instead of keeping
isolates.

### Forms are shared values

A macro call passes `&form` and `&env` as native persistent values, and the
macro returns one. The near-term rule, from light-cljs-466 and -462, keeps
compiler `Datum` engine-independent:

- Import is memoized per `Datum` node. Structural sharing means an unchanged
  subtree that has been imported once is never imported again.
- `&env` becomes a persistent map that is updated incrementally across
  expansions, instead of being rebuilt per call. This closes the
  O(namespace)-per-expansion trap recorded in the October 15 scaling work.
- On export, unchanged subtrees map back to their original `Datum` by
  occurrence identity, and only new nodes are built.

A later option is for `Datum` nodes to *be* engine persistent values, with no
conversion at all. That is attractive, but it ties parser and IR lifetimes to
the heap, which the group context says not to do by accident. Decide it from
the Phase 2 bridge measurements, not up front.

### Helpers are dependency cells

Each compile-time result a macro can demand gets a `DependencyKey`:

- the world (build generation);
- the unit;
- the result kind: binding discovered, callable available, initializer
  complete, or analysis result.

The kinds are kept separate because constructing a function that refers to
another binding does not need that binding's initializer. A macro that calls
`(helper x)` reads the callable through `need`:

- If it is ready, the read returns the shared value and records the edge.
- If it is pending, the read claims the one producer request, and the task
  parks.

The producer is ordinary compiler work on the pool:

1. analyze and emit the unit;
2. compile its JavaScript once into a shared `Executable`;
3. run the initializer as a task;
4. publish the outcome.

Completing the cell enqueues the waiters. No JavaScript runs inside
publication.

A publishable initializer value, such as a function, a persistent collection
or a primitive, is shared by every later reader. A value that cannot be
published, such as a mutable JS object, falls back to per-task initialization.
That is exactly today's per-isolate behavior, so it is no regression, and
`--stats` counts these values so their share is visible. Phase 3 exists to
make that share small.

### Analysis is restartable, JavaScript is resumable

The Rust analyzer is not resumable. It calls a macro in the middle of a
recursive walk. Two ways to handle a macro task that parks:

1. **Help while waiting** (Phase 2). The analyzer's worker blocks on the
   cell and runs other ready jobs from inside the wait, nesting on its
   stack. This is simple, and it is close to today's nested demand. Its
   risks are stack depth and priority inversion, which the trace can
   measure.
2. **Restart the form** (Phase 4, if 1 shows those problems). A top-level
   form's Rust analysis is a pure function of its definition-time snapshot,
   so it can unwind with "pending", be requeued on the cell, and run again
   later. Each macro task is memoized by (unit, occurrence, expansion
   ordinal), so the rerun reads completed expansions rather than running
   macros again. Rust work may be repeated; JavaScript work never is. This
   pairs the cheap side (deterministic Rust) with the side that must not
   replay (effectful JS).

### Order comes from demand, not from an up-front schedule

Stage C as designed for isolates needs relocatable namespace fragments and a
scheduler that knows the require graph ahead of time. On one heap, a
namespace's top-level pass can be a job that `need`s what it uses:

- its requires' declarations;
- the macros it calls;
- the helpers those macros call.

Work starts as soon as its inputs exist. The critical path is the real one,
not the 63-level require depth, because a namespace waits only for the
results it actually reads. Canonical output order still comes from the
source index at merge time, as it does now.

## Compile-time state and determinism

The canonical example is the `counter`/`next-value` test in
[compiler-concurrency.md](compiler-concurrency.md), "Thread safety today". It
prints `1 2` serially and `1 1` when `dep`'s unit runs in another isolate.

On one heap, `counter` is one atom. The `1 1` divergence cannot happen.
Order can still vary: two parallel units that both `swap!` it see a
schedule-dependent order. The group context requires today's stateful macro
behavior on the supported path, and the user's October 22 decision allows
isolation semantics. The proposal below keeps the stronger guarantee
automatically.

**Effect lanes by source position.** Every unit and top-level pass has a
canonical position: source index, then form order. A task that touches a
*shared* compile-time atom parks at that access until every task at an
earlier position has finished. Shared means an atom created before the task
started, not one the macro made for itself. The task then proceeds. Because
bbp can park at any instruction, the task waits before the access, and
nothing is replayed. As a result:

- Units that never touch shared atoms, which should be nearly all of them,
  run freely.
- Units that do touch them run in exactly the order `--jobs 1` uses, so
  `1 2` holds at every N, with no declaration.
- A unit that an earlier position demands, such as a helper initializer,
  inherits the demander's position. This is priority inheritance, and it
  matches how nested demand already runs at the demander's point serially.
  Without it, an earlier unit waiting on a parked later unit would deadlock.
- The cost is proportional to how much code uses effects, and `--stats`
  reports it as time spent waiting for lanes.

Isolates cannot provide this, because they cannot see each other's atom
accesses. bbp can, because atoms are explicit engine objects and not
ordinary JS properties. `--cache-blocker` keeps its cache meaning. For
parallelism it becomes unnecessary, although it can remain a way to force a
namespace onto a lane.

Two cases remain outside the guarantee:

- Mutable JS objects held in Vars cannot be shared. They take the per-task
  fallback above, which is today's isolate semantics.
- Host effects, such as `println` at macro time, keep source order only
  inside lanes.

Both are documented and tested, not silently inherited. This proposal needs
the user's agreement (see "Decisions").

*Delivered (light-cljs-480):* lanes, priority inheritance through demands
and the `--stats` counters are built as proposed; the contract, the rescue of
an earliest unit no free thread starts, and which registries are
compiler-owned are in [compiler-concurrency.md](compiler-concurrency.md),
"Effect lanes (light-cljs-480)".

Deterministic names stay as they are: `src/generated_names.rs` scopes are
already per unit, and the engine's `gensym` counter is lowered to the same
per-unit scheme.

## Where the engine is today

### Fixed macro fixtures (October 25, 2026)

The native-only release baseline at `6405a52c`, before native compile-time core
lowering and 472's automatic GC/budget changes, ran five fresh processes for
each supported fixture/job cell, with cache disabled. Collections (13 native
macro calls) and repeated environment access (16 calls) passed at jobs 1:
median process wall **18.137 ms** (15.408–20.692) and **15.818 ms**
(15.176–18.984), respectively. Median instrumented form bridge time was
**0.183 ms / 0.191 ms**; peak RSS was about **20.7 MB** for each. Results,
JavaScript and maps were identical across all five samples per fixture.

The actual demanded-helper fixture failed with `helper not preloaded`
(existing 463); every jobs 2/4/8 cell failed with `requires --jobs 1`. Those
first-failure attempts are excluded from timing summaries. There is no
parallel scaling result. No verified same-commit release V8 reference was
available, and native traces contained no macro `unit` rows, so median,
longest-unit and critical-path k cannot be calculated. The one bounded
collections profile ended before sampler attachment (17 ms), leaving no
usable stacks; it does not justify attributing cost to collections, property
access or calls. Runtime initialization, separate JS compile/execute,
allocation, GC, suspension/help-depth and utilization counters are unavailable
on this revision. These small startup-inclusive samples are not macro-unit
throughput estimates or substitutes for the existing 477/478/463/472 work.
See [the fixture report](../benchmarks/native-macros-6405a52c/README.md) for
frozen inputs/commands, raw samples, identity checks, all missing-counter
causes and the unsuccessful bounded-profile evidence.

### Project attempts (October 25, 2026)

The bounded real-project baseline at `b3fac889` used the native-only release
compiler (`--no-default-features --features bbp-macros`), cache off, jobs 1,
and a 120-second process limit. Both the pinned port of Penpot `app.main`
(`none`) and the seeded 200k generated project (`advanced`, seed 42) failed
at their first variadic source macro. The engine rejects a sloppy-mode `this`
read when the emitted function is called with an undefined receiver: Penpot's
`app.common.math/max` and generated `workload.fonam/stipopi`. One reduction
per workload reproduces the same failure; light-cljs-489 covers it.

These are unsupported full-project outcomes, not performance results. There
are no successful JS/source maps, five-run matrix, k or profile; jobs 2/4/8
are outside this native backend's supported boundary. This source baseline
predates 472's automatic-GC/aggregate-budget work. Exact inputs, commands,
timeouts, raw diagnostics/resource samples and unavailable metrics are in
[the project-attempt report](../benchmarks/project-results/native-2026-10-25/README.md).

### Deferred units across the pool (October 26, 2026)

light-cljs-481's checkpoint, on the native-only release compiler rebased on
light-cljs-477's native collections (Apple M3 Ultra, load average about 3),
cache off, one run per cell. Both full projects are still unsupported at
`--jobs 1`, now later: the generated 200k project (seed 42) at
`workload.fonam/buvaiskum` ("native macro runtime core.mapSeq", lazy
sequences, light-cljs-479), Penpot `app.main` at `cuerdas.core/unsafe-concat`
("native macro runtime core.comp"). The largest supported workloads are
therefore the pool fixtures of `tests/bbp_macros.rs` and a generated chain of
eight namespaces, each requiring the previous, with 50 helper/macro/caller
triples apiece (400 helpers, all demanded on use).

Before this checkpoint the chain failed at `--jobs 2` and above with
`cannot read property 'call' of undefined`: a producer task took
registrations its worker's engine never got. Now JavaScript and source maps
are identical at 1, 2, 4 and 8 jobs, and the pool fixtures pass 576 perturbed
builds over 12 seeds. Before this work (`c5ac529d`) the chain took 25.1 s and
745 MB RSS at `--jobs 1`; with shared unit factories and block registration
replay it takes 9.5 s and 333 MB. Every helper value now publishes, so helper
initializations equal the serial count at every N (400 here, 4 on the
three-namespace stress fixture, 24 on the twelve-helper one):

| Jobs | Wall | vs 1 | Peak RSS | vs 1 | Helper inits | Task contexts |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 9.5 s | 1.00 | 333 MB | 1.00 | 400 | 401 |
| 2 | 11.5 s | 1.21 | 388 MB | 1.16 | 400 | 402 |
| 4 | 24.9 s | 2.62 | 461 MB | 1.38 | 400 | 404 |
| 8 | 50.0 s | 5.25 | 603 MB | 1.81 | 400 | 408 |

Both targets are missed. An 8-second sample at eight jobs is mostly
condition-variable and mutex waits; of the rest, about three quarters is
stop-the-world `Heap::mark_and_sweep` with its SipHash mark set, the cost
light-cljs-495 covers, which every worker waits out. The chain also retains
about 1 MB per macro call at `--jobs 1`, even without helpers
(light-cljs-499). No same-commit Stage A (V8) comparison was run.

The second attempt (rebased on light-cljs-478) lends task contexts: a pool
slot parks its context after each macro unit and the next macro unit, on any
slot, borrows it (docs/native-macros.md); workers catching up install root
registrations in catalog-aligned blocks of 64; and a parked task's next step
is queued on the pool from the completion's wakeups. Same machine, release
native-only build, peer workers building (load average about 4), one run per
cell. JavaScript and source maps are identical at 1, 2, 4 and 8 jobs, and the
pool fixtures pass 576 perturbed builds over 12 seeds:

| Jobs | Wall | vs 1 | Peak RSS | vs 1 | Helper inits | Contexts booted / borrowed | In-task evaluations |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 9.9 s | 1.00 | 338 MB | 1.00 | 400 | 401 / 0 | 3,680 |
| 2 | 11.8 s | 1.19 | 375 MB | 1.11 | 400 | 402 / 399 | 3,490 |
| 4 | 27.8 s | 2.80 | 483 MB | 1.43 | 400 | 404 / 397 | 3,960 |
| 8 | 47.6 s | 4.81 | 574 MB | 1.70 | 400 | 408 / 393 | 3,920 |

The chain is not serial: each namespace's 50 callers are independent
deferred units, and at eight jobs seven run at once, so seven worker
contexts are in use together and lending cannot share one; it removes the
per-slot duplication only when fewer macro units run at once than there are
slots. Block installs cut in-task evaluations at eight jobs from 16,197 to
3,920 and task steps from 61,758 to 38,508, yet wall and RSS hardly moved:
the build is bound by collection. At `--jobs 1`, 383 automatic collections
pause 6.9 s of the 9.9 s, each marking the retained live set
(light-cljs-499) with the SipHash mark set (light-cljs-495). At eight jobs a
10-second sample has 47,067 samples in condition-variable waits, nearly all
at `Heap::park_at` and `RunGuard::collect_at_point` while one thread
collects, and the top running frames are `Heap::mark_and_sweep` (1,790),
SipHash `Hasher::write` and `hash_one` and `HeapObject::trace`: collection
is serial and stops every worker, and the live set grows with the contexts
and producers alive at once. No helper waits on another task's result in
this chain, so no task was woken from a wakeup (`native.tasks.woken` 0);
the evaluator test and the stress fixtures (at four and eight jobs) exercise
that path.

Rebased again on light-cljs-495 (`2d5ba2d9`: marking without hashing,
collection by budget headroom), same chain, commands and load:

| Jobs | Wall | vs 1 | Peak RSS | vs 1 | Helper inits | Contexts booted / borrowed |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 3.17 s | 1.00 | 577 MB | 1.00 | 400 | 401 / 0 |
| 2 | 3.54 s | 1.12 | 669 MB | 1.16 | 400 | 402 / 399 |
| 4 | 2.56 s | 0.81 | 693 MB | 1.20 | 400 | 404 / 397 |
| 8 | 3.38 s | 1.07 | 754 MB | 1.31 | 400 | 408 / 393 |

Output is still identical at every N. At `--jobs 1` collections fell from 383
(6.9 s of pauses) to 35 (0.3 s), and the heap now grows toward the budget
before collecting, so RSS rose. Wall at eight jobs is near `--jobs 1` and four
jobs is faster, which meets the wall target on this chain. Peak RSS at eight
jobs is 1.31x (1.26x with `--macro-memory-limit 1073741824`): the seven worker
contexts in use at once each hold every catalog registration, and producer
tasks replay their parent's, so per-task registries grow with the macro units
that run at once (light-cljs-503).

### Shared catalog data and ordinary task objects (October 26, 2026)

light-cljs-503 shares immutable registration data across a compilation's native
tasks. Each frontier is a persistent vector prefix; root and declaration
descriptors contain only strings, numbers, booleans and other immutable vectors.
Versions share their storage. Each task roots its own frontier; the shared
cache holds weak handles and prunes collected entries after GC. Advancing or
dropping the last task at a frontier releases that snapshot. On an exact
frontier cache miss, a task extends the greatest live cached prefix within
its requested registration range, constructing only the missing descriptors.
If no suitable cached prefix survives, it extends its own rooted prefix.
A worker imports its newly registered suffix. A helper producer imports its
parent's exact snapshot and populates a fresh ordinary JavaScript object through
the existing registration functions, in registration order. Initializer slots,
getters, bundle products and protocol tables belong to the task. Registration
source is no longer retained, parsed or evaluated for the compilation catalog.

This deliberately uses ordinary properties and their existing registration
semantics: every root or bundle registration runs, including repeated and mixed
redefinitions. Accessor and inherited preloads, saved/copied getters, partial
descriptors, key ordering, deletion, sealing and freezing therefore follow the
ordinary object paths. There is no descriptor coalescing, virtual property
backing, or catalog-specific redefinition handling. The differential object
table and once-per-task mutable initializer tests cover those contracts.

Populating an ordinary object still allocates task-owned getters and slots for
every registration a producer inherits. Sharing data removes catalog source
replay; it does not remove the O(helpers × registrations) property-population
work or its transient allocations. Measurements below separate this remaining
cost from in-task source evaluations. light-cljs-508 owns the residual
concurrent-task memory premium.

Initial measurements before the historical-frontier lifetime fix, at
`5b8f73b0`, based on master `b3b655cc`, on macOS 15.7.3,
Apple M3 Ultra (Virtual), 24 vCPUs and 32 GiB RAM, Homebrew Rust/Cargo 1.99.1
and Node 26.12.1. Build with
`CARGO_BUILD_JOBS=3 cargo build --locked --release --no-default-features --features bbp-macros --bin light-cljs`.
Use the eight-namespace chain above, with `h0` through `h49` in each namespace:
`(defn hi [x] (list (quote +) x i))`, `(defmacro mi [x] (hi x))`,
`(defn fi [] (mi 0))`, substituting each index for `i`. Namespace `n0` has no
require; each later namespace requires its predecessor. Run each job count with:

```sh
/usr/bin/time -lp target/release/light-cljs --compile --entry chain.n7 \
  --source-root target/catalog-chain-400 --jobs N --no-cache --stats \
  --source-map-inline --macro-timeout-ms 600000 \
  --macro-memory-limit 4294967296 --macro-expansions 1000000 \
  --macro-nodes 100000000 > target/chain.js 2> target/chain.stats
```

Three runs per cell, cycling 1/2/4/8 jobs. The table gives medians; RSS is
`time -l`'s maximum resident set size in decimal MB. Runs were admitted only
with no competing Cargo, rustc, nextest or compiler process before and after
each run; an earlier batch during which peer tests restarted was excluded.
The one-minute load average declined from 9.18 to 7.53 after those suites
finished (five-minute 8.39 to 8.06, fifteen-minute 8.26 to 8.15).

| Jobs | Wall | Peak RSS | RSS vs 1 | Contexts | In-task evaluations | Task steps |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2.64 s | 100.2 MB | 1.00 | 401 | 2,810 | 15,879 |
| 2 | 3.14 s | 109.1 MB | 1.09 | 402 | 2,004 | 17,194 |
| 4 | 2.39 s | 128.5 MB | 1.28 | 404 | 2,218 | 17,642 |
| 8 | 3.01 s | 181.2 MB | 1.81 | 408 | 2,154 | 17,684 |

RSS ranges were 100.1–101.0 / 108.6–109.5 / 127.7–135.2 / 179.2–185.7 MB.
Paired eight/one ratios were 1.810 / 1.789 / 1.839: the 1.15 target is missed.
Every run compiled and initialized exactly 400 helpers, all published, with
zero per-task fallbacks. All twelve JavaScript outputs, including inline
source maps, were byte-identical. With 25 triples per namespace (200 helpers),
one serial run took 0.95 s and 69.1 MB, with 1,410 evaluations and 7,429 task
steps. Doubling helpers gives 2,810 evaluations and 15,879 steps: source
evaluations are linear, while ordinary catalog property population remains
and increases instruction work. These figures are from the simplified data
snapshot design on its stated base, not a before/after comparison with the
older registry designs or bases.

A separate three-second `/usr/bin/sample PID 3` at eight jobs, with no competing
build/test process, showed these collapsed top-of-stack counts:
`__psynch_cvwait` 7,759, VM `execute_budgeted` 1,408,
`OrdinaryObject::push` 1,080, `__psynch_mutexwait` 1,015,
`Heap::allocate_accounted` 624, `Heap::mark_and_sweep` 252, and VM
`new_function` 249. These are sampled thread stacks, not retained-byte
attribution. Concurrent tasks still retain their ordinary registries and
initializer slots, and producers still allocate their inherited properties;
the profile includes property insertion, function allocation and collection
alongside waits. Live task state, transient allocation and allocator residency
have not been separated. The residual ratio and this evidence were appended
to light-cljs-508 rather than creating a duplicate follow-up.

On the rebased tree, the focused native selection passes 143 tests;
`LIGHT_CLJS_STRESS_SEEDS=12` `native_pool_builds` passes in 249 s;
QuickJS plus native library/macro/reader/proxy coverage passes 431 tests,
including the ordinary-object differential table. Before this native-only
base update, full default V8 and QuickJS/native suites passed 2,129/2,130 and
2,193/2,194 tests respectively: both have only the documented playground
workspace-discovery failure in nested worktrees. Their unchanged legacy
compiler paths reuse those results; affected native coverage was rerun.
Default strict all-target Clippy passes. Native strict lint reaches the
existing engine diagnostics. Focused native lint and combined all-target lint
pass allowing only `deprecated`, `chunks_exact_to_as_chunks` and
`byte_char_slices`. Native-only/engine-free full test-target compilation is
also limited by existing ungated legacy-engine tests; focused engine-free
library/binary strict lint passes. Both formatter checks pass.

#### Historical frontier lifetime fix (October 26, 2026)

The shared registry no longer roots all historical vector prefixes. Live
tasks own their current immutable snapshot and the registry caches it weakly.
The GC regression checks that a lagging task preserves its older frontier,
can rebuild a newer frontier after its sole owner drops, and releases each
snapshot when the last task advances or drops.

For 200/400 root registrations added one at a time, with collection every
32 registrations, the retained catalog data is 145,272/290,104 accounted
bytes (1.997x). The test isolates catalog data by dropping the ordinary task
object and frames, then measures the bytes released by collecting the final
snapshot. That final snapshot is also asserted to die: the cache cannot keep
historical prefixes alive or mask their retained bytes after task teardown.
The existing ordinary-object differential table, saved-getter, mixed
redefinition, preload and once-per-task initializer regressions are retained.

Fresh measurements use native-only release `9a828324` on master `529e2db6`,
the same machine, fixture, build command and CLI flags above. Three runs per
job count were admitted with no competing Cargo/rustc/nextest/compiler process
before and after each run; none needed discarding. Load averages declined
from 2.69/7.06/9.73 to 2.35/6.41/9.36. These are fresh-base figures, not an
isolated before/after comparison with the earlier implementation.

| Jobs | Median wall | Median peak RSS | RSS vs 1 | Contexts | Median evaluations | Median task steps |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2.91 s | 97.7 MB | 1.000 | 401 | 2,810 | 16,779 |
| 2 | 3.52 s | 109.0 MB | 1.115 | 402 | 2,004 | 18,348 |
| 4 | 3.08 s | 146.9 MB | 1.503 | 404 | 2,008 | 18,493 |
| 8 | 3.07 s | 203.7 MB | 2.084 | 408 | 2,016 | 18,604 |

Peak RSS ranges are 97.3–99.0 / 108.8–109.7 / 144.7–154.8 / 195.6–204.8 MB.
Paired eight/one ratios are 2.106/2.002/2.057, so the 1.15 gate is missed.
All twelve JavaScript outputs and inline source maps are byte-identical;
every run compiles and publishes 400 helpers with zero per-task fallbacks.
The 200-helper serial run takes 1.04 s and 68.3 MB, with 1,410 evaluations
and 7,754 task steps versus 2,810 and 16,779 for 400 helpers. Catalog source
evaluations remain linear; ordinary per-producer property population remains.

A separate eight-job `/usr/bin/sample PID 3` after competing tests finished
has collapsed top-of-stack counts: `semaphore_timedwait_trap` 11,421,
`__psynch_cvwait` 4,160, VM `execute_budgeted` 410, VM fault drop 131,
SipHash `write` 108, `Heap::mark_and_sweep` 104, `lookup_cursor` 73,
`OrdinaryObject::push` 49 and `Heap::allocate_accounted` 41. These are sampled
thread stacks, not CPU or retained-byte attribution. Removing historical
catalog roots does not remove the live ordinary task registries, initializer
slots, or per-producer property population. Those task-owned/transient
allocations and allocator residency remain candidate sources of the measured
RSS premium; their split is still unmeasured. The fresh ratios and profile
were appended to the existing light-cljs-508 follow-up rather than creating
a duplicate or restoring virtual-property machinery.

On master `529e2db6`, the combined QuickJS/native library, macro, reader and
proxy selection passes all 442 tests, and the native-only 12-seed pool stress
passes in 306 s. On the preceding `52a1aa9f` base the native selection passed
158 tests, full default V8 passed 2,129/2,130 and full QuickJS/native passed
2,206/2,207. Both full-suite failures are the unchanged nested-worktree
`playground_crate_checks_for_wasm32` workspace-discovery failure; the only
subsequent base changes are native comparison-diagnostic hashing, so the
unaffected legacy coverage is reused and affected native coverage rerun.
Default strict all-target Clippy passes. Native strict lint reports eight
existing engine diagnostics; focused native and final combined all-target
lint pass allowing only the three documented engine lint categories.
Engine-free library/binary strict lint passes, while engine-free all-target
lint still cannot compile the existing ungated legacy examples/tests.

#### Live cached prefix reuse (October 27, 2026)

light-cljs-534 fixes duplication on an exact-frontier cache miss. Construction
now upgrades the greatest live weakly cached frontier in `first..end` and
allocates descriptors only for the suffix after that prefix. It falls back
to the task-owned prefix when no suitable cached snapshot survives. Applying
the catalog still starts at `first`, so every requested ordinary registration
runs in order. The cache still owns no strong snapshot roots.

The new regression keeps tasks at staggered frontiers: an older task starts
at 17 registrations, another reaches 1,000, and another reaches 1,004. A fresh
task requests 1,001 and the older task catches up to 1,003. After collection,
overlapping descriptors must share object identity; the ahead frontier must
not leak later registrations into an earlier request. Keeping just these four
catalog snapshots after dropping ordinary task state retains 2,635,652 bytes
before the fix and 457,376 after it (82.6% less). The original 200/400-entry
linear-retention check remains 145,272/290,104 bytes, including on the rebased
tree. Existing frontier lifetime, object-semantics and once-per-task initializer
tests remain intact.

The 400-helper RSS comparison uses native-only release `ba7def01` before and
the same base plus this fix (`713c7201`, before rebasing) after. The source
change is now carried by `b6a30129`; later engine integration changes are
excluded from this controlled release comparison. Machine, fixture, build
command and CLI flags are the same as above: macOS 15.7.3, Apple M3 Ultra
(Virtual), 24 vCPUs/32 GiB, Rust 1.99.1 and Node 26.12.1. Three cycles at
jobs 1/2/4/8 give these medians, with RSS in decimal MB:

| Jobs | Before wall | After wall | Before RSS | After RSS | Before RSS vs 1 | After RSS vs 1 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2.92 s | 2.83 s | 94.2 MB | 94.5 MB | 1.000 | 1.000 |
| 2 | 3.48 s | 3.37 s | 104.1 MB | 105.4 MB | 1.105 | 1.115 |
| 4 | 3.00 s | 2.87 s | 143.8 MB | 146.8 MB | 1.526 | 1.554 |
| 8 | 3.01 s | 2.93 s | 192.1 MB | 197.2 MB | 2.039 | 2.087 |

Before RSS ranges at jobs 1/2/4/8 are 94.0–94.4 / 103.8–106.6 /
142.7–146.8 / 186.4–198.5 MB; after ranges are 94.2–95.0 / 103.1–106.1 /
136.8–156.9 / 196.7–199.3 MB. Paired eight/one ratios are
2.035/2.107/1.983 before and 2.109/2.092/2.072 after. There is no measured
peak-RSS improvement in this sample, and the existing 1.15 target remains
missed. The descriptor identity and retained-byte regression demonstrate
the cache fix; live task state, transient allocations and allocator residency
still require light-cljs-508's separate attribution work.

No competing Cargo build/test, rustc, nextest or compiler process was present
before or after any admitted run. One preliminary full batch overlapping
restarted peer builds/tests was excluded; a later partial batch stopped on a
formatter process and was rerun. The final after batch required a minute
without competing build/test processes, excluding formatter processes from
that gate. Load averages before decline from 5.59/7.50/6.72 to
4.29/6.92/6.54; after they decline from 1.94/3.76/5.31 to 1.84/3.53/5.16.
These different ambient loads preclude attributing the small wall-time change
to prefix reuse.

All 24 JavaScript outputs and inline source maps are byte-identical. Every
run compiles and publishes 400 helpers with zero per-task fallbacks; contexts
are 401/402/404/408 and median evaluations remain 2,810/2,004/2,008/2,016.
Median task steps are 16,779/18,351/18,501/18,603 before and
16,779/18,348/18,496/18,610 after. The results were appended to the existing
light-cljs-508 evidence; no additional memory follow-up was created.

Verification on master `7c7a8ed2` plus the prefix-reuse change passes all 156
native evaluator/macro/reader/proxy tests with `LIGHT_CLJS_STRESS_SEEDS=12`;
the pool stress takes 304 s and the effect-lane stress 425 s. Full default V8
(`LIGHT_CLJS_V8_CACHE=require`) passes 2,131/2,132 tests; full QuickJS/native
passes 2,219/2,220. Each full suite has only the documented nested-worktree
`playground_crate_checks_for_wasm32` workspace-discovery failure. Default
all-target and engine-free library/binary strict Clippy pass. Combined
QuickJS/native strict lint reaches the same eight existing engine diagnostics;
all-target lint passes allowing only `deprecated`, `chunks_exact_to_as_chunks`
and `byte_char_slices`. Engine-free/native-only all-target lint also reaches
the existing ungated legacy examples/tests. The three generic worker service
tests selected accidentally by a broad native-only `catalog` filter reproduce
the same unsupported `__lightCompileUnit`/`__lightBuildService` failures with
the original catalog implementation; they pass with QuickJS available.
Formatter checks pass.

### Native concurrent-task storage attribution (October 27, 2026)

light-cljs-508 keeps the ordinary-object catalog design. Specialized wrapper,
Date, RegExp, iterator and Intl payloads now occupy optional boxed storage;
the optional property-dictionary header is boxed too. On aarch64 this reduces
`HeapObject` from 304 to 136 bytes. The former inline ordinary-object payload
enlarged every function, binding cell and form allocation as well. Payload
boxes remain charged to the aggregate budget and traced through their owner;
intrinsic stand-ins preserve the same brands and immutable payloads. There
is no virtual registry, registration coalescing or change to property semantics.

`--stats` now reports one shared native-runtime snapshot as
`macro.gc.{collections,live-objects,managed-bytes,peak-managed-bytes,live-bytes,admitted-bytes,byte-limit}`.
Unlike the indexed per-expansion records, it includes worker tasks whose local
host records have been dropped. `live-*` describes the last collection;
`managed-bytes` is the current admission charge, including uncollected garbage.
Neither is allocator residency or process RSS, and their difference from RSS
must not be presented as an exact allocator-overhead measurement.

The `concurrent_catalog_task_memory` test isolates root sources on one runtime:
collect the realm, warm the shared reader-tag cache and drop its task, then
boot one/eight tasks, drop the seven extra tasks, install
600/1,200 root registrations in the remaining task, spawn seven producers,
then drop them and the parent. Collections occur at every boundary. The
ordinary-object counts return exactly to the one-task counts after dropping
the producers, and to the warmed no-task counts after dropping the parent.
The runtime intentionally retains 13 pure reader-tag functions and their form.
This is a
lifetime regression and a diagnostic fixture, not a timing/RSS gate:

```sh
CARGO_BUILD_JOBS=3 cargo test --locked --no-default-features \
  --features bbp-macros --lib concurrent_catalog_task_memory -- --nocapture
```

On integration base `bbd5d97b`, before the layout changes (release code
`8da03981`) and after (`713d5982`), collected accounted bytes in this fixture
are below. Both runs use the fixture's shared-reader-cache warmup.

| Root source / phase | Before bytes | After bytes |
| --- | ---: | ---: |
| Realm only, initially | 1,153,008 | 1,070,424 |
| No tasks, warmed reader cache | 1,863,072 | 1,778,136 |
| One booted task | 3,201,084 | 2,943,612 |
| Eight booted tasks | 14,588,816 | 13,123,592 |
| One task, 600 registrations | 9,245,592 | 8,685,216 |
| Eight tasks, 600 registrations each | 42,545,508 | 39,363,828 |
| One task, 1,200 registrations | 13,271,104 | 12,408,328 |
| Eight tasks, 1,200 registrations each | 70,633,956 | 65,738,676 |

Each parked task has one frame, 29 stack values, no native continuation, and
1,312 shallow saved-state bytes at both registration counts. This excludes
vector capacity, dependency logs and compiler bookkeeping. Eight booted tasks
retain 7,133 functions and 392 binding cells; eight 1,200-registration tasks
retain 16,733 functions and 9,858 plain objects. All object-kind counts are
unchanged by the layout reduction. Their shallow heap-object storage falls
from 8,790,768 to 3,932,712 bytes; total collected admission falls 6.9%.

The largest measured live task cost remains ordinary registration state,
not saved VM frames. Each root has a task-owned `roots` Map entry, a four-field
initializer slot and a getter on the ordinary root object. Their respective
conservative backing-store charges are 2,048, 4 × 512 and 512 bytes per root,
before the getter's captures and heap-object metadata. At eight tasks and
1,200 roots these three stores alone account for 44,236,800 bytes. These are
admission charges, not measured malloc sizes. The final realm-only phase
still retains object-table high-water storage: 5,213,812/6,766,348 accounted
bytes after the 600/1,200 fixtures, with all task-owned functions and catalog
vectors collected. Registry population also allocates temporary descriptor objects;
automatic collection and allocator residency contribute to the transient peak.

The controlled native release comparison uses the same `bbd5d97b` integration
base and shared-heap instrumentation on both sides (`8da03981` release code
before, `713d5982` after). Only the ordinary-object layout changes. Build with
`CARGO_BUILD_JOBS=3 cargo build --locked --release --no-default-features --features bbp-macros`.
The 400-helper chain and CLI budgets are the same as above. On macOS 15.7.3,
Apple M3 Ultra (Virtual), 24 vCPUs/32 GiB, Rust/Cargo 1.99.1 and Node 26.12.1,
three cycles alternate before/after at each jobs value, reversing pair order
on the middle cycle. Medians are decimal MB and seconds:

| Jobs | Before RSS | After RSS | Before wall | After wall |
| --- | ---: | ---: | ---: | ---: |
| 1 | 99.3 | 95.0 | 2.99 | 2.93 |
| 2 | 111.4 | 103.5 | 3.50 | 3.38 |
| 4 | 146.4 | 130.8 | 3.00 | 2.87 |
| 8 | 196.7 | 171.7 | 3.21 | 2.97 |

Eight/one ratios of median RSS are **1.981 before and 1.806 after**; paired
ratios are 1.897/1.993/2.025 before and 1.896/1.846/1.759 after. Every after
pair misses 1.15. The eight-job RSS premium over one job falls from 97.4 to
76.6 MB (21.4%), while total eight-job RSS falls 12.7%. RSS ranges at jobs
1/2/4/8 are 98.7–100.2 / 108.3–114.2 / 143.9–150.9 / 188.3–202.9 MB before
and 93.0–95.1 / 102.4–105.1 / 130.0–135.8 / 167.2–180.3 MB after.

All 24 JavaScript outputs, including inline source maps, are byte-identical.
Every build compiles and publishes 400 helpers with zero per-task fallbacks.
Contexts remain 401/402/404/408 and source evaluations remain
2,810/2,004/2,008/2,016. Median task steps are 16,729/18,337/18,475/18,589
before and 16,729/18,332/18,477/18,586 after. Registration source replay has
not returned; ordinary producer property population still scales with the
catalog each producer inherits.

The aggregate heap counters distinguish retained admission from allocation
churn. These are medians of the same runs; live/peak are MB and total admitted
volume is GB:

| Jobs | Before last live | After last live | Before peak | After peak | Before admitted | After admitted |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 22.2 | 19.1 | 56.5 | 49.7 | 2.533 | 2.243 |
| 2 | 32.7 | 28.8 | 69.2 | 61.6 | 3.362 | 3.007 |
| 4 | 46.1 | 40.4 | 102.2 | 90.7 | 3.362 | 3.006 |
| 8 | 80.7 | 73.1 | 151.8 | 140.6 | 3.411 | 3.054 |

The last collection and the RSS peak occur at different times. The remaining
peak includes task-owned registries/initializer slots/getters, temporary
descriptor allocation and allocator residency; the precise malloc/residency
split remains unmeasured. Saved VM frames are far smaller than this live task
state. Reducing object metadata helps both retained state and transient churn,
but does not eliminate ordinary registration replication.

Two preliminary partial batches were excluded after a formatter and another
worker's V8 run appeared. Accepted pairs use ten idle seconds at admission or
retry, process snapshots before/after each build, and process sampling every
second throughout it. Only the measured compiler is excluded from that scan;
an observed competing build/test/compiler invalidates the whole pair. No such
process was observed during any accepted run. The 400-helper window's load
averages change from 1.98/6.41/9.96 to 2.48/3.65/7.34; the longer averages
include preceding peer activity. All three admitted pairs are retained.

One admitted pair also builds the seed-42 generated project (`--lines 200k`,
199,129 nonblank source lines, 450 worker namespaces) at one job. From
`target/generated-200k`, run `/usr/bin/time -lp BINARY release bench --no-cache
--stats --jobs 1` with the generated configuration's 2-GiB aggregate macro
budget. Native RSS changes from 1,343.4 to 1,311.3 MB and wall time from
38.61 to 34.83 s; peak managed admission changes from 578.3 to 524.1 MB.
The generated JavaScript hashes match. This is one pair, not a repeated
large-project performance result. The eight-job pair was rejected when peer
builds began during the after run. Its RSS is excluded.

The residual private initializer-slot representation is isolated as
light-cljs-552: compact the four-field task-owned bags while preserving the
ordinary root objects, getter identity/state and registration order. This
does not move this pellet's remaining verification into that follow-up.

The saved pre-compaction eight-job `/usr/bin/sample` profile on `ba1bb3bd`
has collapsed top-of-stack counts 10,906 `semaphore_timedwait_trap`, 4,062
`__psynch_cvwait`, 447 `execute_budgeted`, 111 VM-fault drops, 111 SipHash
writes, 107 `Heap::mark_and_sweep`, 81 `lookup_cursor`, 63
`Heap::allocate_accounted` and 58 `OrdinaryObject::push`. These are thread
samples, not CPU percentages or retained-byte attribution. The subsequent
literal/protocol integrations prevent treating it as a final-tree CPU profile.

On the integrated compact-storage tree `713d5982` (later documentation changes only),
all 2,165 engine tests and six doctests pass, as do all 56 evaluator tests with
the QuickJS differential oracle. Native-only macro integration passes all
110 tests with `LIGHT_CLJS_STRESS_SEEDS=12` in 535 s, including both the pool
and source-order effect-lane stress cases. Commands are:

```sh
CARGO_BUILD_JOBS=3 cargo test --locked -p big-boy-pants-js --all-targets -- --test-threads=3
CARGO_BUILD_JOBS=3 cargo test --locked -p big-boy-pants-js --doc -- --test-threads=3
LIGHT_CLJS_STRESS_SEEDS=12 CARGO_BUILD_JOBS=3 cargo test --locked \
  --no-default-features --features bbp-macros --test bbp_macros -- --test-threads=3
```

Full compiler compatibility runs use `cargo nextest run --locked -j 8
--no-fail-fast`: default V8 with `LIGHT_CLJS_V8_CACHE=require` passes
2,132/2,133; QuickJS-only (`--no-default-features --features quickjs`) passes
2,055/2,056; QuickJS/native (`--no-default-features --features bbp-macros,quickjs`)
passes 2,247/2,248. Each has only `playground_crate_checks_for_wasm32` failing
with the existing nested-worktree signature: Cargo finds
`/Users/admin/src/light-cljs/Cargo.toml` as the workspace of this worktree's
`playground/crate/Cargo.toml`. No assertion or required regression is skipped
to obtain these results.

Compiler default all-target strict Clippy and engine-free library/binary
strict Clippy pass. Engine strict all-target Clippy still has eight existing
library diagnostics and one additional test diagnostic; combined native/QuickJS
strict Clippy reaches the eight library diagnostics. The isolated all-target
check passes with only `-A deprecated -A clippy::chunks_exact_to_as_chunks
-A clippy::byte_char_slices` after `-D warnings`. Engine-free all-target strict
Clippy remains blocked by seven E0599 errors for the ungated benchmark calls
to `Evaluator::{evaluate_js,string,number}` in `benches/support/{callees,levels}.rs`;
those files and `src/evaluator.rs` are unchanged from `ba1bb3bd`. Both formatter
checks pass. These are explicit strict-lint/full-suite limitations.

The final rebase onto protocol integration `7004c99b` changes native protocol
lookups/runtime code and native tests, with no changes to the ordinary-object
layout or shared-heap reporting. Affected coverage is rerun: all 2,166 engine
tests, seven doctests, 56 evaluator tests and 113 combined QuickJS/native macro
tests pass, as do both formatters and combined all-target lint with only the
three existing diagnostic categories allowed. The native macro rerun uses
the default one stress seed. The 12-seed result above is on the preceding
literal integration; it is not a claim of fresh 12-seed coverage on the final
protocol base. Default V8 and QuickJS-only results are reused across this
native-only delta; the complete combined compiler suite predates that delta.

That attempt released light-cljs-508 after peer activity interrupted the
remaining measurements. The resumed verification below records the final-base
checks and completes the previously missing 200-helper attribution.

#### Rebased verification (October 28, 2026)

The resumed branch rebases cleanly onto `e28daa30`; its delta from the previous
`7004c99b` base contains tests and documentation only. Native release code is
`6f2ed7bf`, with the same compiler flags, host, toolchain, chain inputs and
budgets as the controlled comparison above. A new exact-headroom regression
covers both Date and primitive-wrapper payload boxes: one byte short denies
allocation without leaking a reservation, exact headroom succeeds, collection
preserves a rooted box, and dropping its root releases the entire slot/body
charge on collection. No object semantics change in this verification pass.

Fresh 400-helper runs below use a full minute without competing build/test
processes before the batch, plus the same before/during/after admission scan.
Load is the one/five/fifteen-minute average immediately before and after each
run; RSS is decimal MB and wall time is seconds. No row overlaps a detected
peer build/test/compiler process, and no measured row in this batch is excluded.

| Round | Jobs | RSS | Wall | Load before → after |
| --- | ---: | ---: | ---: | --- |
| 0 | 1 | 94.5 | 2.79 | 1.60/3.02/4.59 → 1.87/3.05/4.59 |
| 0 | 2 | 104.1 | 3.29 | 1.87/3.05/4.59 → 1.80/3.02/4.57 |
| 0 | 4 | 140.9 | 2.76 | 1.80/3.02/4.57 → 1.80/3.02/4.57 |
| 0 | 8 | 187.4 | 3.04 | 1.80/3.02/4.57 → 1.90/3.02/4.56 |
| 1 | 1 | 95.2 | 2.70 | 1.90/3.02/4.56 → 1.82/2.98/4.54 |
| 1 | 2 | 100.6 | 3.29 | 1.82/2.98/4.54 → 1.76/2.95/4.52 |
| 1 | 4 | 134.4 | 2.95 | 1.76/2.95/4.52 → 1.76/2.95/4.52 |
| 1 | 8 | 182.0 | 2.84 | 1.76/2.95/4.52 → 1.78/2.93/4.50 |
| 2 | 1 | 91.8 | 2.76 | 1.78/2.93/4.50 → 1.78/2.93/4.50 |
| 2 | 2 | 104.4 | 3.31 | 1.78/2.93/4.50 → 1.87/2.94/4.49 |
| 2 | 4 | 138.5 | 2.72 | 1.87/2.94/4.49 → 1.88/2.92/4.48 |
| 2 | 8 | 180.4 | 2.80 | 1.88/2.92/4.48 → 1.89/2.90/4.46 |

Median RSS is **94.5/104.1/138.5/182.0 MB**, and median wall time is
2.76/3.29/2.76/2.84 s. The fresh eight/one ratio is **1.927**, with paired
ratios 1.984/1.913/1.966: the 1.15 target remains missed. This is confirmation
on the final base, not a replacement for the earlier controlled before/after
comparison. All twelve JavaScript outputs and inline maps match each other
and the controlled comparison. Every run compiles/publishes 400 helpers,
with zero per-task fallbacks; pooled runs initialize exactly 400 helpers.
Contexts and evaluations remain 401/402/404/408 and 2810/2004/2008/2016.
Median steps are 16729/18337/18469/18598.

The 200-helper chain (`--entry chain.n3`, otherwise identical flags and
source root) has three roots per helper: 600 registrations, matching the
smaller lifetime fixture. Its previously missing eight-job measurements are
now admitted, with an independent minute of idle admission before this batch:

| Round | Jobs | RSS | Wall | Load before → after |
| --- | ---: | ---: | ---: | --- |
| 0 | 1 | 63.3 | 0.99 | 1.95/2.70/4.26 → 1.95/2.70/4.26 |
| 0 | 8 | 125.2 | 0.96 | 1.95/2.70/4.26 → 2.12/2.72/4.26 |
| 1 | 1 | 64.2 | 1.00 | 2.12/2.72/4.26 → 2.12/2.72/4.26 |
| 1 | 8 | 119.4 | 1.07 | 2.12/2.72/4.26 → 2.12/2.72/4.26 |
| 2 | 1 | 64.1 | 1.00 | 2.12/2.72/4.26 → 2.12/2.72/4.26 |
| 2 | 8 | 121.0 | 1.00 | 2.12/2.72/4.26 → 2.03/2.69/4.24 |

Median RSS is **64.1/121.0 MB** (1.889x); both wall medians are 1.00 s.
Each run compiles/publishes 200 helpers with zero per-task fallbacks, with
201/208 contexts, 1410/1016 evaluations and median 7821/8270 task steps.
Output and inline maps match at both job counts. The final runtime counters complete the live/transient
accounting for both chain sizes (medians, bytes in decimal MB; total admitted
volume in decimal GB):

| Helpers | Jobs | Last collected live | Managed at report | Peak managed | Total admitted |
| --- | ---: | ---: | ---: | ---: | ---: |
| 200 | 1 | 11.7 | 27.3 | 36.0 | 0.882 |
| 200 | 8 | 47.2 | 72.9 | 82.3 | 1.085 |
| 400 | 1 | 18.5 | 26.2 | 48.7 | 2.301 |
| 400 | 2 | 26.3 | 33.9 | 63.8 | 3.066 |
| 400 | 4 | 41.7 | 68.9 | 93.0 | 3.065 |
| 400 | 8 | 67.6 | 106.9 | 143.7 | 3.108 |

The final collected 600-registration diagnostic separates root ownership
from those workload peaks. These are accounted bytes after explicit collection,
alongside the independently reported shallow object-table and object bytes;
the latter two are components/diagnostics, not additional amounts to add to
the admission total:

| Phase | Collected admission | Shallow table | Shallow objects |
| --- | ---: | ---: | ---: |
| Realm | 1,070,424 | 37,120 | 67,184 |
| Warmed reader cache, no tasks | 1,823,232 | 158,304 | 69,088 |
| One booted task | 3,109,770 | 158,304 | 209,440 |
| Eight booted tasks | 14,137,184 | 773,872 | 1,191,904 |
| Drop seven booted tasks | 5,131,418 | 810,736 | 209,440 |
| One task, 600 registrations | 8,851,374 | 810,736 | 454,648 |
| Eight tasks, 600 registrations | 40,377,420 | 1,088,296 | 2,581,416 |
| Drop seven producers | 9,797,094 | 1,162,024 | 454,648 |
| Drop parent and catalog | 5,258,908 | 1,162,024 | 69,088 |

Dropping the seven producers releases **30,580,326 bytes** of admission.
Seven booted tasks alone release 9,005,766 bytes; the remaining 21,574,560
bytes reflect their catalog population beyond bootstrap in this fixture.
The 4,200 extra roots' Map entries, four-field initializer slots and getter
properties charge **8,601,600 + 8,601,600 + 2,150,400 = 19,353,600 bytes**.
The remaining 2,220,960-byte difference includes getter captures, metadata,
other backing stores and differences in table high-water charges. These
conservative charges do not claim exact malloc sizes or an exhaustive
allocator attribution. Eight populated tasks retain 5,178 plain objects,
11,941 functions and 408 binding cells. After producer/parent release the
object-kind counts return exactly to the one-task/warmed-no-task baselines;
the shared reader cache and high-water table remain. The final no-task
admission exceeds the initially warmed baseline by 3,435,676 bytes, while
the table's shallow storage grows by 1,003,720 bytes. Segment reservations
also conservatively cover future directory/free-list storage, explaining why
these accounting measures differ. Frames remain one per task, 29 stack values,
no native continuation and 1,312 shallow bytes at both registration counts.

For the real 200-helper run, eight jobs have 35.5 MB more last-collected live
admission, 46.3 MB more peak managed admission and 57.0 MB more RSS than one
job. Those peaks need not coincide. Registration stores dominate the measured
task-owned live premium, with bootstrap state, temporary descriptors,
collection timing and allocator residency also contributing. Compiler data,
executable code, stacks and allocator storage are outside these heap counters;
the RSS/managed difference is not an exact allocator-residency estimate.
The 1.15 target is still missed for this recorded reason, and existing
light-cljs-552 tracks the private initializer-slot representation.

The final engine run passes all **2,167 tests and seven doctests**, including
the new payload admission/reclamation regression. All 56 evaluator tests pass
with `--no-default-features --features bbp-macros,quickjs --lib bbp_evaluator::tests::`,
covering the ordinary-object differential table, saved getters, registration
order and once-per-task initializer behavior. Strict engine all-target Clippy
retains the same eight library diagnostics and ninth test diagnostic described
above; allowing only the three documented existing categories passes. These
checks cover `6f2ed7bf`; subsequent edits in this verification are documentation.

On this rebased base, native-only `LIGHT_CLJS_STRESS_SEEDS=12 CARGO_BUILD_JOBS=3
cargo test --locked --no-default-features --features bbp-macros --test bbp_macros
-- --test-threads=3` passes all **112 tests in 510.45 s**. Both
`native_pool_builds_match_one_slot_under_seeded_scheduling_stress` and
`effect_lanes_keep_one_slot_atom_effects_under_seeded_scheduling_stress` pass;
this replaces the earlier one-seed-only final-protocol result.

Fresh full compiler runs use `cargo nextest run --locked -j 8 --no-fail-fast`,
with `LIGHT_CLJS_V8_CACHE=require` for default V8, and
`--no-default-features --features quickjs` for QuickJS. V8 passes **2132/2133**
in 268.63 s; QuickJS passes **2055/2056** in 298.23 s. Both fail only
`playground_crate_checks_for_wasm32`, with the same `tests/cli.rs:2000`
workspace-discovery signature quoted above: the worktree's playground package
finds `/Users/admin/src/light-cljs/Cargo.toml` as its workspace. These fresh
runs replace the reused legacy-engine results for this final base. The
109/108 normally ignored tests remain ignored by nextest's ordinary selection;
no failure or required regression is skipped. Both formatters, default
all-target strict Clippy and engine-free library/binary strict Clippy pass.

The final combined `bbp-macros,quickjs` check runs `cargo test --locked
--no-default-features --features bbp-macros,quickjs --lib --test bbp_macros
--test bbp_reader --test bbp_proxy_has --test compiler_cache -- --test-threads=3`
with `CARGO_BUILD_JOBS=3`. All **497 selected tests pass**, with one normally
ignored library test: 355 library, 114 macro, two proxy, 12 reader and 14 cache
tests. The combined macro run uses the default one stress seed; the separate
native-only run above supplies the required twelve-seed coverage. Combined
strict all-target Clippy has the same eight engine library diagnostics; the
check allowing only the three documented existing categories passes.

Engine-free all-target strict Clippy can stop at more targets than the earlier
seven-error benchmark result. To establish its complete baseline, run
`CARGO_BUILD_JOBS=3 cargo clippy --locked --no-default-features --all-targets
--keep-going -- -D warnings` on the final source and with `src/compile_stats.rs`
temporarily restored from `e28daa30` (all other code compiled without engines
is unchanged from that base). Restore the final file afterward. Both runs
produce **identical diagnostic/message and source-path multiplicities**:
seven benchmark errors, two evaluator-example errors, five benchmark-variant
test errors, 501 compiler-test errors, 107 evaluator-test errors and one
execution-test error. These are ungated missing evaluator methods/optional
QuickJS imports and empty-engine unreachable/unused diagnostics, not errors
introduced by the heap statistics or object layout. Engine-free library/binary
strict Clippy still passes. This broader baseline is recorded explicitly;
the all-target check is not claimed to pass and no source suppression is added.

A separate final eight-job, 400-helper `/usr/bin/sample PID 3 1 -mayDie`
on `6f2ed7bf` records collapsed top-of-stack counts: 11,306
`semaphore_timedwait_trap`, 4,144 `__psynch_cvwait`, 477 `execute_budgeted`,
137 SipHash writes, 119 VM-fault drops, 83 `Heap::mark_and_sweep`, 74
`lookup_cursor`, 59 `OrdinaryObject::push` and 54 `Heap::allocate_accounted`.
This sample deliberately records the final executable while peer tests are
active: load changes from **6.88/6.82/7.54 to 6.97/6.83/7.54**, with competing
processes observed before, during and after. These are observed thread stacks,
not CPU percentages, retained-byte attribution or a quiet before/after speed
comparison. The sampled build's JavaScript and inline map match the admitted
chain outputs. Its timing/RSS is excluded from the quiet measurement tables.

The default V8 200k guard isolates the compiler statistics change on the same
base: the before release temporarily restores `src/compile_stats.rs` from
`e28daa30`, and the after release uses `6f2ed7bf`; all other code compiled for
default V8 is identical. Build both with `LIGHT_CLJS_V8_CACHE=require
CARGO_BUILD_JOBS=3 cargo build --locked --release --bin light-cljs`, restoring
the final source after the baseline build. Run the same seed-42 generated
project with `/usr/bin/time -lp BINARY release bench --no-cache --stats --jobs 1`.
The three admitted pairs below use alternating pair order, reversing the
middle pair (seconds and decimal MB). Every process has no detected competing
build/test/compiler before, during or after; idle admission requires ten
seconds. Six interrupted attempts are excluded, including the whole pair
when only one side overlaps. All three accepted pairs are retained.

| Pair | Version | Wall | RSS | Load before → after |
| --- | --- | ---: | ---: | --- |
| 0 | Before | 7.12 | 1061.8 | 2.87/6.04/7.98 → 2.72/5.95/7.94 |
| 0 | After | 7.02 | 1063.3 | 2.72/5.95/7.94 → 2.58/5.87/7.90 |
| 1 | Before | 7.20 | 1076.1 | 2.51/4.74/6.36 → 2.39/4.68/6.33 |
| 1 | After | 7.24 | 1107.3 | 2.50/4.82/6.41 → 2.51/4.74/6.36 |
| 2 | Before | 7.04 | 1058.5 | 2.66/4.51/6.20 → 2.52/4.45/6.17 |
| 2 | After | 7.10 | 1107.0 | 2.52/4.45/6.17 → 2.44/4.37/6.12 |

Wall medians are **7.12 s before / 7.10 s after** (0.997x), showing no observed
build-time slowdown. Individual after/before ratios are 0.986/1.006/1.009;
wall ranges overlap at 7.04–7.20 / 7.02–7.24 s. RSS medians are
1061.8/1107.0 MB. All six generated JavaScript outputs are byte-identical (SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`).

### Compact task-owned initializer slots (October 28, 2026)

light-cljs-552 replaces only the private initializer bags with a fixed
`HeapObject::Native` payload. Unit, name and result remain full traced JavaScript
values; state and the independent definition flag are private scalar fields.
Reads and writes check the owning task under the existing per-object access
word. Slots cannot publish, survive saved-frame migration and collection, and
use the existing boxed-payload aggregate admission/reclamation path. The
ordinary root objects, Map registries, property descriptors and saved getter
identities keep their existing semantics. There is no virtual property,
registration coalescing or initializer replay.

The payload is **64 bytes**, plus the unchanged **136-byte heap object**.
An exact-headroom regression proves that one byte short denies the allocation
without retaining a reservation, exact slot/body headroom succeeds, collection
preserves a rooted payload, and releasing its root reclaims the complete
200-byte charge. Direct engine tests also cover arbitrary unit/name identities
(including fractional numbers, negative zero, NaN, symbols and mutable objects),
mutable/falsy results, cyclic edges, foreign-task read/write rejection,
publication rejection, byte-limit recovery, and collection between VM slices
on distinct OS workers.

Before choosing the fixed payload, the isolated 1,200-slot storage comparison
measures four-property bags, five-element JavaScript arrays and native slots
on separate byte-accounted runtimes, keeping the same outer array live.
Collected admission / total admitted bytes are respectively
**3,874,712 / 3,874,848**, **1,866,776 / 1,866,912**, and
**1,492,376 / 1,492,512**. Each case retains 1,694 heap objects; the array
alternative also retains its full array/exotic storage. These totals include
the common realm, container and high-water table costs. Admission is
conservative accounting, not a measurement of malloc sizes or RSS.

The existing `concurrent_catalog_task_memory` diagnostic runs unchanged on
`b3ef562c` before and release-code `674a1b58` after, using the same warmed
reader-cache protocol and 4-GiB aggregate budget described above:

| Collected phase | Before bytes | After bytes |
| --- | ---: | ---: |
| One task, 600 registrations | 8,851,374 | 7,659,438 |
| Eight tasks, 600 registrations each | 40,377,420 | 30,853,196 |
| One task, 1,200 registrations | 12,574,486 | 10,192,150 |
| Eight tasks, 1,200 registrations each | 66,752,268 | 47,704,844 |

Eight-task collected admission falls **23.6% / 28.5%** at 600/1,200 roots.
The 4,800/9,600 initializer objects move from plain objects to native payloads;
plain-object counts remain 378 at both sizes, with the same
11,941/16,741 functions and 408 binding cells as before. Object-kind counts
return exactly to the one-task counts after producer release, then to the
warmed no-task counts after parent release; no slot survives. Each task still
parks with one frame, 29 stack values, no native continuation and 1,312 shallow
saved-state bytes. The former four-property charge of 2,048 bytes is replaced
by the fixed 64-byte payload charge, saving 1,984 accounted bytes per slot
before small differences in interned keys/runtime storage.

The size diagnostic also reports an **80-byte ordinary property entry** on
this host. Four live entries therefore contain 320 bytes, before any spare
Vec capacity, shared shape storage or allocator overhead, versus the slot's
64-byte fixed payload. Both representations use the same 136-byte heap
object. This is a backing-storage comparison independent of the much larger
512-byte-per-property admission charge; neither measures allocator residency.

#### Quiet RSS and build-time comparisons

The before release is `b3ef562c`; the after release uses runtime code
`674a1b58`, with no other production changes. Both use
`CARGO_BUILD_JOBS=3 cargo build --locked --release --no-default-features
--features bbp-macros --bin light-cljs`. Host and toolchain remain macOS
15.7.3, Apple M3 Ultra (Virtual), 24 vCPUs/32 GiB, Rust/Cargo 1.99.1 and Node
26.12.1. Inputs, CLI limits and inline maps are the same eight-namespace,
50-helpers-per-namespace chain described above.

Admission requires a full minute without detected competing build/test/compiler
processes before each batch, then 10–16 consecutive idle seconds before each
run. Process scans occur before/after and every 100 ms during a run. Three
cycles pair before/after at each job count, reversing pair order in the middle
cycle. All three accepted pairs per cell are retained. One V8 200k pair overlaps
`rustfmt` and is excluded in its entirety, then retried; no other pair is
excluded. Load is the one/five/fifteen-minute average before and after each
run; RSS is decimal MB and wall time is seconds. The 400-helper raw pairs are:

| Round | Jobs | Version | RSS MB | Wall s | Load before → after |
| --- | ---: | --- | ---: | ---: | --- |
| 0 | 1 | before | 92.0 | 2.77 | 2.12/3.08/6.35 → 2.12/3.08/6.35 |
| 0 | 1 | after | 91.7 | 2.66 | 2.10/3.05/6.30 → 2.01/3.01/6.27 |
| 0 | 2 | before | 102.0 | 3.22 | 2.01/2.96/6.20 → 2.01/2.96/6.20 |
| 0 | 2 | after | 104.0 | 3.14 | 1.78/2.87/6.10 → 1.80/2.85/6.08 |
| 0 | 4 | before | 136.8 | 2.75 | 1.69/2.78/6.00 → 1.80/2.78/5.98 |
| 0 | 4 | after | 126.2 | 2.64 | 1.85/2.74/5.91 → 1.85/2.74/5.91 |
| 0 | 8 | before | 167.6 | 2.77 | 1.72/2.68/5.85 → 1.74/2.67/5.83 |
| 0 | 8 | after | 168.2 | 2.69 | 1.50/2.57/5.74 → 1.50/2.57/5.74 |
| 1 | 1 | after | 91.9 | 2.65 | 1.38/2.49/5.65 → 1.35/2.47/5.63 |
| 1 | 1 | before | 92.3 | 2.69 | 1.34/2.41/5.55 → 1.34/2.41/5.55 |
| 1 | 2 | after | 100.4 | 3.19 | 1.29/2.33/5.45 → 1.29/2.33/5.45 |
| 1 | 2 | before | 102.3 | 3.18 | 1.31/2.29/5.38 → 1.29/2.26/5.35 |
| 1 | 4 | after | 132.6 | 2.62 | 1.68/2.30/5.31 → 1.68/2.30/5.31 |
| 1 | 4 | before | 134.3 | 2.74 | 1.67/2.27/5.24 → 1.70/2.26/5.23 |
| 1 | 8 | after | 170.6 | 2.68 | 2.76/2.47/5.02 → 2.76/2.47/5.02 |
| 1 | 8 | before | 180.3 | 2.84 | 2.52/2.43/4.96 → 2.52/2.43/4.96 |
| 2 | 1 | before | 91.5 | 2.63 | 2.33/2.39/4.90 → 2.33/2.39/4.90 |
| 2 | 1 | after | 92.0 | 2.63 | 2.17/2.35/4.83 → 2.17/2.35/4.83 |
| 2 | 2 | before | 101.2 | 3.20 | 2.07/2.32/4.79 → 2.14/2.33/4.78 |
| 2 | 2 | after | 100.6 | 3.12 | 2.41/2.38/4.75 → 2.41/2.38/4.75 |
| 2 | 4 | before | 130.2 | 2.84 | 2.17/2.33/4.69 → 2.23/2.34/4.68 |
| 2 | 4 | after | 128.3 | 2.65 | 1.96/2.28/4.63 → 2.21/2.32/4.63 |
| 2 | 8 | before | 169.5 | 2.80 | 2.42/2.36/4.62 → 2.47/2.37/4.61 |
| 2 | 8 | after | 180.5 | 2.69 | 2.24/2.33/4.57 → 2.30/2.34/4.56 |
The resulting medians are:

| Jobs | Before RSS | After RSS | Before wall | After wall |
| --- | ---: | ---: | ---: | ---: |
| 1 | 92.0 | 91.9 | 2.69 | 2.65 |
| 2 | 102.0 | 100.6 | 3.20 | 3.14 |
| 4 | 134.3 | 128.3 | 2.75 | 2.64 |
| 8 | 169.5 | 170.6 | 2.80 | 2.69 |

At four jobs, all three after RSS values are lower than their paired before
values: 136.8→126.2, 134.3→132.6 and 130.2→128.3 MB. The median reduction is
**4.4%**. One/two-job median reductions are smaller. **No repeatable eight-job
RSS improvement is demonstrated**: before/after ranges overlap at
167.6–180.3 / 168.2–180.5 MB, and the after median is 0.6% higher.
Eight/one ratios of median RSS are **1.842 before / 1.856 after**; paired
ratios are 1.822/1.953/1.852 before and 1.834/1.856/1.963 after. Every after
pair still misses **1.15**. The reduction in accounted live storage must not
be described as an equivalent process-RSS reduction.

All 24 JavaScript outputs, including inline source maps, are byte-identical.
Every run compiles/publishes 400 helpers, executes 400 deferred helper units
and has zero per-task fallbacks; pooled runs initialize exactly 400 helpers.
Contexts remain 401/402/404/408 and source evaluations remain
2810/2004/2008/2016. Median task steps change from
16729/18342/18471/18586 to 16579/17979/18115/18235. The shared native heap
counters describe admission and collection, independently of RSS:

| Jobs | Version | Last collected live MB | Managed at report MB | Peak managed MB | Total admitted GB |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | before | 18.5 | 26.2 | 48.7 | 2.301 |
| 1 | after | 16.9 | 23.0 | 40.8 | 1.942 |
| 2 | before | 25.5 | 33.1 | 60.1 | 3.066 |
| 2 | after | 22.3 | 28.3 | 50.0 | 2.468 |
| 4 | before | 42.9 | 67.2 | 92.7 | 3.067 |
| 4 | after | 36.3 | 43.8 | 74.6 | 2.469 |
| 8 | before | 72.2 | 80.6 | 136.7 | 3.109 |
| 8 | after | 60.7 | 74.2 | 115.0 | 2.506 |

The 200-helper entry (`chain.n3`, otherwise identical flags) exercises 600
registrations, matching the smaller collected diagnostic. Its three paired
rounds at jobs one/eight are:

| Round | Jobs | Version | RSS MB | Wall s | Load before → after |
| --- | ---: | --- | ---: | ---: | --- |
| 0 | 1 | before | 65.3 | 1.00 | 1.87/2.19/4.27 → 1.87/2.19/4.27 |
| 0 | 1 | after | 63.0 | 1.01 | 2.03/2.22/4.24 → 2.03/2.22/4.24 |
| 0 | 8 | before | 113.4 | 1.00 | 2.15/2.24/4.21 → 2.15/2.24/4.21 |
| 0 | 8 | after | 116.1 | 0.97 | 2.03/2.21/4.17 → 2.03/2.21/4.17 |
| 1 | 1 | after | 63.8 | 0.98 | 2.74/2.36/4.19 → 2.74/2.36/4.19 |
| 1 | 1 | before | 65.2 | 1.02 | 2.34/2.29/4.13 → 2.34/2.29/4.13 |
| 1 | 8 | after | 118.1 | 1.06 | 2.17/2.26/4.08 → 2.17/2.26/4.08 |
| 1 | 8 | before | 113.9 | 1.01 | 1.91/2.20/4.03 → 1.91/2.20/4.03 |
| 2 | 1 | before | 65.6 | 0.98 | 2.15/2.23/4.01 → 2.15/2.23/4.01 |
| 2 | 1 | after | 62.3 | 0.97 | 2.13/2.23/3.99 → 2.13/2.23/3.99 |
| 2 | 8 | before | 122.1 | 1.00 | 2.11/2.22/3.95 → 2.26/2.24/3.95 |
| 2 | 8 | after | 113.4 | 0.96 | 2.14/2.22/3.92 → 2.14/2.22/3.92 |
One-job median RSS falls **65.3→63.0 MB (3.4%)**, with lower RSS in all three
pairs; median wall time changes 1.00→0.98 s. Eight-job medians are
**113.9→116.1 MB**, with wall time 1.00→0.97 s: this smaller workload also
shows no reliable eight-job RSS improvement. All twelve outputs and inline
maps match. Each build compiles/publishes 200 helpers, executes 200 deferred
helper units, and uses zero per-task fallbacks; pooled initialization count
is 200. Contexts remain 201/208 and evaluations remain 1406/1016.

The seed-42 generated project uses `--lines 200k` (199,129 nonblank source
lines, 450 worker namespaces), the existing generated configuration and its
2-GiB aggregate macro budget. From `target/generated-200k`, run
`/usr/bin/time -lp BINARY release bench --no-cache --stats --jobs 1`.
Native release uses one admitted pair; default V8 uses three admitted pairs,
with before/after pair order reversed in the middle round. Both default
releases are built with `LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3
cargo build --locked --release --bin light-cljs`:

| Workload | Pair | Version | RSS MB | Wall s | Load before → after |
| --- | ---: | --- | ---: | ---: | --- |
| native200k | 0 | before | 1316.9 | 33.72 | 1.68/2.06/3.71 → 2.03/2.11/3.66 |
| native200k | 0 | after | 1293.0 | 33.26 | 2.27/2.16/3.66 → 1.90/2.08/3.57 |
| v8200k | 0 | before | 1090.1 | 7.22 | 2.18/2.13/3.39 → 2.24/2.14/3.38 |
| v8200k | 0 | after | 1091.3 | 7.05 | 2.05/2.10/3.34 → 1.89/2.07/3.32 |
| v8200k | 1 | after | 1115.3 | 7.03 | 1.75/2.03/3.29 → 1.94/2.06/3.29 |
| v8200k | 1 | before | 1107.2 | 7.07 | 2.55/2.19/3.31 → 2.38/2.17/3.29 |
| v8200k | 2 | before | 1079.9 | 7.09 | 2.29/2.16/3.26 → 2.09/2.12/3.24 |
| v8200k | 2 | after | 1067.7 | 7.04 | 1.92/2.08/3.21 → 2.02/2.10/3.20 |
The native pair lowers RSS **1,316.9→1,293.0 MB (1.8%)** and wall time
33.72→33.26 s. Last-collected live admission falls 349.2→273.9 MB, managed
at report 360.5→284.2 MB, peak managed 505.9→411.4 MB and total admitted
8.198→7.058 GB. This is one large-project pair, not a repeated native timing
gate. Default V8 wall medians are **7.09→7.04 s (0.993x)**, with no observed
slowdown; RSS medians are 1090.1→1091.3 MB. All eight accepted large-project
JavaScript artifacts are byte-identical (SHA-256
`1a75bc87b26f312196671d4de9ed73110fc8aef077749fd8b6d5ce352bf4524e`).

The remaining retained contributors are the ordinary registries/getters,
getter captures, bootstrap state and heap-table high-water storage. In the
collected 1,200-registration diagnostic, eight tasks still retain **37.5 MB**
more accounted bytes than one task. Seven extra booted tasks account for
11.0 MB before registration. Their 8,400 extra Map entries/root properties
retain conservative charges of **17.20 + 4.30 MB**, with another **0.54 MB**
for compact slot payloads, plus metadata/captures and other storage. These
are admission components, not exact malloc sizes or a decomposition of the
RSS peak. Temporary descriptors/property population, collection timing,
compiler data, executable code, stacks and allocator residency also affect
process peaks; the RSS/accounting difference is not an allocator-overhead
measurement. The smaller actual slot backing store and lower admission are
verified, but the 1.15 target and an eight-job RSS benefit remain unproven.
No distinct new retained-memory cause was isolated, so no additional memory
follow-up is filed.

A separate eight-job `/usr/bin/sample PID 3 1 -mayDie` run of the after
executable records collapsed top-of-stack counts of 10,976
`semaphore_timedwait_trap`, 3,834 `__psynch_cvwait`, 465 `execute_budgeted`,
271 `__ulock_wait`, 125 SipHash writes, 107 VM-fault drops, 70
`Heap::mark_and_sweep`, 60 `lookup_cursor`, 51 `Heap::allocate_accounted`
and 34 `OrdinaryObject::push`. Peer compiler tests are active, and load is
**19.00/15.63/11.58** before and after. These are observed thread stacks,
not CPU percentages or retained-byte attribution. This sampled build's
wall/RSS is excluded from the quiet comparisons.

Correctness is checked independently of timing/RSS. Native-only
`LIGHT_CLJS_STRESS_SEEDS=12 CARGO_BUILD_JOBS=3 cargo test --locked
--no-default-features --features bbp-macros --test bbp_macros -- --test-threads=3`
passes all **112 tests** in 518.87 s, including both scheduling/effect-lane
stress tests. All 56 native evaluator tests pass, including the ordinary-object
differential table, saved getters, preloads, redefinitions, cancellation and
once-per-task execution. Combined `bbp-macros,quickjs` library, reader, proxy
and cache coverage passes **383 tests** (355 library, 12 reader, two proxy,
14 cache; one normally ignored library test), including the QuickJS oracle
for the object table. The full engine run passes **2,171 tests and seven
doctests**; the subsequently added exact-headroom unit test also passes.
Its addition and later documentation do not change engine production code,
so the full-engine result is reused without repeating the unchanged suite.

Full compiler nextest runs use `-j 8 --no-fail-fast`, with
`LIGHT_CLJS_V8_CACHE=require` for default V8 and
`--no-default-features --features quickjs` for QuickJS. V8 passes
**2,132/2,133** in 254.82 s; QuickJS passes **2,055/2,056** in 323.26 s.
Both fail only `playground_crate_checks_for_wasm32` with the unchanged
`tests/cli.rs:2000` workspace-discovery signature described above. Their
109/108 normally ignored tests remain ignored; no failure is skipped.
Both formatters and default/QuickJS all-target strict Clippy pass.
Engine-free library/binary strict Clippy passes; its all-target `--keep-going`
run reproduces the same 7/2/5/501/107/1 benchmark/example/benchmark-variant/
compiler/evaluation/execution errors recorded above. Engine all-target strict
Clippy retains the same eight library/ninth test diagnostics; combined
`bbp-macros,quickjs` all-target strict Clippy retains the eight engine library
diagnostics. Allowing only the three documented baseline categories passes
both configurations, with no source suppressions. Runtime code is
`674a1b58`, based on `b3ef562c`; these checks cover that code, with subsequent
changes confined to tests/documentation as identified above.

### Final deferred-pool acceptance (light-cljs-481, October 28, 2026)

This acceptance measures the integrated deferred-unit implementation; the
coordinator's top-level passes remain serial. The native backend uses one
compilation-owned runtime across the pool, shared helper factories and
dependency cells, parked/lent task contexts, completion wakeups and source-order
effect lanes. The default remains V8 plus QuickJS.

The release compiler at `a4e173db` cannot build Penpot `app.main`. Using the
five absolute read-only source roots and all budgets in the
[Penpot reproduction](native-macros.md#penpot-reproduction-light-cljs-507509-2026-10-26),
with `--entry app.main --jobs 1 --optimize none --no-cache --stats`, the exact
first error is:

```text
light-cljs: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/main/ui/shapes/text/fo_text.cljs:17:1: macro expansion: while expanding mf/defc: /Users/admin/src/light-cljs/research/penpot-light/frontend/src/app/main/ui/shapes/text/fo_text.cljs:17:1: macro rumext.v2/defc: big-boy-pants-js: Error: unsupported operation: native macro runtime core.flatten; definition /Users/admin/src/light-cljs/research/penpot-light/vendor/rumext/v2.cljs:334:1 (bytes 522..819)
```

The fallback workloads are the seed-42 generated 200k project (199,129
nonblank lines, 450 worker namespaces) and the eight-namespace, 400-helper
chain specified above. Neither input is checked in. Generate the larger
project with:

```sh
bb benchmarks/generated-project/generate.bb target/generated-200k --seed 42 --lines 200k
```

The opt-in test
`native_large_project_matches_one_slot_under_seeded_scheduling_stress` in
`tests/bbp_macros.rs` compares each of twelve seeds at jobs 1/2/4/8 with an
unperturbed one-job reference. It compares full JavaScript and serialized
source maps, including source contents, at both `none` and `advanced`, plus
warnings and compiled/published/per-task helper counts. The perturbation
randomizes job-boundary delays, batches and steals. It uses the generated
project's macro limits and 2-GiB aggregate budget, and requires an explicit
fixture so ordinary test runs do not generate large inputs. Reproduce with:

```sh
LIGHT_CLJS_STRESS_SOURCE_ROOTS="$PWD/target/generated-200k/src:$PWD/target/generated-200k/macros" \
LIGHT_CLJS_STRESS_ENTRY=workload.main LIGHT_CLJS_STRESS_SEEDS=12 CARGO_BUILD_JOBS=3 \
cargo test --locked --release --no-default-features \
  --features bbp-macros,scheduling-stress --test bbp_macros \
  native_large_project_matches_one_slot_under_seeded_scheduling_stress \
  -- --ignored --exact --nocapture --test-threads=1
```

For the chain, use the same command with
`LIGHT_CLJS_STRESS_SOURCE_ROOTS="$PWD/target/catalog-chain-400"` and
`LIGHT_CLJS_STRESS_ENTRY=chain.n7`.

The opt-in `scheduling-stress` feature exposes the existing debug scheduling
hooks in release builds. Ordinary release builds omit them, and debug builds
retain their existing hooks without requiring the feature. No assertions are
changed: the test's JavaScript, map, warning and count comparisons are ordinary
assertions and run in release builds. A sampled debug-assertion-enabled 200k
reference spends its time rescanning accumulated program prefixes in
`program_fragment::append`; debug-assertion-enabled attempts were stopped before
completing a seed matrix and supply no completed 200k stress result. The release feature permits the large
matrix without changing those debug checks or production defaults.

The debug chain run at `a4e173db` passes all **49 builds** (one reference plus
twelve seeds at four job counts), in 885.38 seconds. Every build compiles and publishes
**400 helpers**, with **zero per-task fallbacks**. JavaScript, full maps,
warnings and helper counts match at both optimization levels.

The ordinary native-only debug `bbp_macros` suite at `a4e173db` separately passes
**114 tests** with `LIGHT_CLJS_STRESS_SEEDS=12` and three test threads in 157.83 seconds,
including its randomized, fixed-assignment and one-job-admission pool/effect
stress. Its one ignored test is the large-fixture acceptance explicitly run
above. Its test profile uses optimization level 3, debug info 0, thin LTO and
one codegen unit, retaining debug assertions. The scheduling feature only
broadens the hook guards; their debug-build behavior is unchanged, so these
debug results are reused across that change.
With `--release --features bbp-macros,scheduling-stress`, all **34 scheduler
unit tests** pass in 4.20 seconds and all **114 ordinary native macro tests**
pass with twelve stress seeds and two test threads in 207.43 seconds (the
large-fixture test remains opt-in). The release feature's macro test-target
Clippy passes allowing only the same three existing engine categories.
The release-feature chain matrix also passes all **49 builds** in 867.51
seconds, with `(compiled, published, per-task) = (400, 400, 0)` throughout and
identical JavaScript/full maps at both optimization levels, warnings included.
The generated 200k release-feature matrix passes all **49 builds** in
1,717.26 seconds, with `(compiled, published, per-task) = (57, 57, 0)`
throughout. All twelve seeds match the serial reference at jobs 1/2/4/8 for
full JavaScript, serialized maps with sources, warnings and helper counts at
both `none` and `advanced`. Together the two large matrices cover 98 builds.
Compiler and engine format checks and default all-target strict Clippy pass.
Native test-target strict Clippy retains the eight existing engine-library
diagnostics; allowing only `deprecated`, `chunks_exact_to_as_chunks` and
`byte_char_slices` passes, without source suppressions. Production behavior
with the scheduling feature off is unchanged; full legacy compiler/engine
suites are not repeated.

#### Release measurement method

The measured binaries are rebuilt at `6712eab2` (production base `a4e173db`,
plus the opt-in test hooks) with `scheduling-stress` **off**. Native:
`CARGO_BUILD_JOBS=3 cargo build --locked --release --no-default-features
--features bbp-macros --bin light-cljs`. Default V8/QuickJS:
`LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3 cargo build --locked --release
--bin light-cljs`. Default all-target strict Clippy and both format checks
pass again on this final source. The host is macOS 15.7.3, Apple M3 Ultra
(Virtual), 24 vCPUs and 32 GiB, Rust/Cargo 1.99.1, Node 26.12.1 and Babashka
1.12.218.

Use the chain command above (default `prepare`, inline source map, 4-GiB
macro budget). For the generated project, use a standalone advanced ES module
so the measured artifact includes a source map; the chunked `release` command
does not support source maps. From `target/generated-200k`, replacing `BINARY`
with either saved release compiler and `N` with 1/2/4/8:

```sh
/usr/bin/time -lp BINARY --compile --esm --entry workload.main \
  --source-root src --source-root macros --optimize advanced \
  --jobs N --no-cache --stats --source-map-inline \
  --macro-expansions 10000000 --macro-nodes 1000000000 \
  --macro-timeout-ms 3600000 --macro-memory-limit 2147483648 \
  --macro-poll-budget 100000000000 > output.js 2> output.stats
```

Three rounds interleave native and V8 at each job count, reversing their
order in the middle round. Admission requires a full minute without detected
competing build/test/compiler processes, then ten idle seconds before each
row, with process scans every 100 ms during execution. Load is the
one/five/fifteen-minute average immediately before and after each row.
RSS is `/usr/bin/time -l`'s maximum resident set size in decimal MB, rather
than its separate peak-footprint counter or native heap admission accounting.

### Single-thread speed

`target/release/big-boy-pants-js` on a script, compared with Node 26 with
`--jitless` (V8's Ignition interpreter only). Process start-up of about
0.04 s is included in both.

| Kernel | bbp | `node --jitless` | Ratio |
| --- | ---: | ---: | ---: |
| Arithmetic loop, 3 M iterations, in a function | 0.98 s | 0.07 s | ~30x (start-up subtracted) |
| Cons list build, map with a closure, count; 300 × 1,000 | 3.70 s | 0.08 s | ~45x |

The engine's own benchmark agrees: `reads.js_baseline_loop` is 326 ns per
iteration of an eight-instruction loop. The compile itself runs V8 with its
JIT. On Penpot, turning off V8's optimizing tiers and Sparkplug raised the
build from 23 s to 60 s. The macro side alone therefore runs about 3 to 4
times faster with the JIT than in Ignition. Taken together, bbp is on the
order of 100x slower than V8 as the compiler uses it today.

A 5-second sample of the cons kernel shows that this is engine immaturity,
not something inherent in the interpreter:

- About 45% of samples are in `vm::proxy::inspect` and
  `module::namespace::probe`. Each property access asks whether the object
  is a Proxy or a module namespace by looking it up in the realm view and
  then under the object-table lock.
- Then come property lookup, `prepare_put`, `check_inherited`, `malloc` and
  `free`, mutex lock and unlock, SipHash on keys, and UTF-16 key encoding.
  There are no shapes and no inline caches.

### Multi-thread scaling

One `Runtime`, N threads, each running four tasks of the same executable
from start to finish. The figures are throughput relative to one thread.

| Kernel | 1 | 2 | 4 | 8 threads |
| --- | ---: | ---: | ---: | ---: |
| Arithmetic (no heap access) | 1.00x | 1.92x | 3.62x | 7.31x |
| Allocate and traverse (cons/map/count, closures) | 1.00x | 1.11x | 0.46x | 0.37x |

Thirty-two allocation tasks take 15.8 s on one thread and 42.6 s on eight.
The cost per task does not grow with heap size: one thread takes 0.49 s per
task for 4 tasks and for 32. At eight threads, 80% of samples are
`__psynch_mutexwait`/`mutexdrop`. `src/heap/mod.rs` states the cause:
"Objects are only reached through the `objects` lock". Allocation, reads and
writes of every object go through one `Mutex<ObjectTable>`. The engine has
no global interpreter lock, but it does have a global object lock on the hot
path, and on macOS a contended mutex is worse than a serialized one.

The fix follows from the engine's own ownership rules.

- A task-owned object is touched only by its owning task while that task
  runs. Tasks never run on two workers at once, so access needs no lock.
- A shareable object is immutable once published, so reads need no lock.
- Only allocation, reclamation and the stop-the-world collector need shared
  coordination. Worker-local allocation buffers and per-slot state that can
  be read without the table lock handle those. The architecture document
  already lists allocation buffers as deferred.
- The exotic-kind questions (Proxy? namespace? realm intrinsic?) become a
  tag in the value or object header instead of a locked lookup.

This is Phase 0. It is a redesign of the heap's access path, not tuning.

### What has to be true for the bet to pay off

A build cannot finish faster than its longest chain of dependent work, which
runs on one thread. So per-thread speed bounds wall time however many cores
there are. A rough model using the Penpot profile:

- 9 s of Rust analysis, which parallelizes;
- 6.7 s of macro bodies at V8 speed;
- the bridge, which disappears;
- a 2.8 s critical path, of which about 60% is JavaScript;
- the largest unit, instaparse's grammar precompile, at about 2 s of
  JavaScript.

Let *k* be bbp's per-thread time as a multiple of V8's on those macro bodies.
The model assumes 16 effective workers.

| k | CPU / 16 | Critical path | Wall bound | Compare |
| ---: | ---: | ---: | ---: | --- |
| 1 | 1.0 s | 2.8 s | ~3 s | V8 serial 23 s, Stage A at 8 jobs 12.4 s |
| 3 | 1.8 s | ~6 s | ~6 s | |
| 5 | 2.7 s | ~10 s | ~10 s | |
| 10 | 4.8 s | ~20 s | ~20 s | |
| 100 (today) | 42 s | ~170 s | ~170 s | |

These are idealized bounds, not forecasts. They ignore lane waits,
contention and collection pauses. They show the shape: the strategy wins
clearly at *k* ≤ 3 to 4, roughly breaks even with Stage A at 6, and loses
beyond that. Getting to *k* ≤ 4 takes two things, and neither is enough
alone:

- **A competent interpreter.** QuickJS ran macro bodies 5.1x slower than V8
  on the 200k generated project and 8.2x slower on Penpot
  ([benchmarks/compiler.md](../benchmarks/compiler.md), light-cljs-435).
  Ignition is in the same range. Matching that class from today means a
  30 to 45x improvement. That is large, but it is conventional engineering:
  no locks on access, shapes, inline caches and interned keys. On its own
  it lands at *k* ≈ 5 to 8, which is break-even at best.
- **Moving the hot operations out of JavaScript.** Compile-time ClojureScript
  spends much of its time in collection, keyword, equality, hashing and
  protocol operations. Today those are JavaScript in `src/js_runtime.js`,
  and V8 JIT-compiles them. In bbp they become Rust intrinsics on native
  HAMTs and tries. A native `assoc` beats JIT JavaScript `assoc`, so *k* on
  cljs-shaped code can fall well below *k* on generic JavaScript. This is
  Phase 3. It is required for the bet, not polish, and it is also what makes
  helper values shareable.

The warm cache already skips unchanged namespaces, so these numbers are
about cold builds. Watch builds and cache misses after small edits gain from
the retained single runtime (Phase 5) even before *k* is small.

## Phases

The phases are strategy. Pellets hold the work, and their order in the queue
is not strictly by phase: the heap pellets (470, 471) come before 466 and
462, because native form values and the backend build on the heap access
API that those pellets change.

**Phase 0: an engine hot path that scales.** light-cljs-468 adds the kernel
and scaling benchmark. It tracks two kernel suites, generic JS and
cljs-shaped code taken from real compiler output (light-cljs-465's corpus),
at 1, 2, 4 and 8 threads. 469 removes the per-access exotic-object probes.
470 allocates without the global lock. 471 accesses task-owned and published
objects without it, with a gate of at least 6x allocation scaling at eight
threads. 472 triggers collection automatically within one aggregate budget.
473 adds shapes, inline caches and keys interned once instead of encoded and
SipHashed per lookup. 474 cuts per-instruction overhead, with a gate of
single-thread kernels within 3x of `node --jitless`.

**Phase 1: real macros, serially.** light-cljs-465 boots the real runtime and
runs a checked corpus. 466 adds native Clojure form values. 462 adds a
selectable bbp backend that runs real `defmacro`s with no legacy engine.
467 hardens deep nesting. Gate: the pellets' own acceptance criteria. The
first measurement of *k* on real macro units is light-cljs-464, which runs
after Phase 0 and 463 and is the checkpoint before Phase 3.

**Phase 2: resumable helper demand.** light-cljs-463 replaces
`__lightCompileUnit` with dependency cells, help-while-waiting on the
existing pool, and deterministic output at `--jobs 1/2/4`. This is where the
bridge and helper-initialization costs are measured as gone or not.

**Phase 3: native compile-time core.** Lower the compile-time `cljs.core` to
bbp intrinsics:

- persistent collections, keywords and symbols with Clojure equality and
  hashing;
- protocols dispatched through engine-level tables instead of property
  lookups;
- records and `deftype`s without `^:mutable` fields as immutable native
  structs, with an atomic cache slot for the hash;
- lazy seqs as once-cells realized by one producer, with racing readers
  parked. These are dependency cells local to a value: a thunk with
  exactly-once realization.

Emitted applications are unaffected, because this is a compile-time
emission mode only. The pellets are light-cljs-477 (collections, keywords,
symbols), 478 (protocols and immutable types) and 479 (lazy seqs). Gate: the
share of non-shareable helper values in Penpot's `--stats` is small, and *k*
is measured again.

**Phase 4: one-heap parallel build.** Top-level passes and units become
tasks and jobs over one runtime. Namespaces are ordered by `need`, and
effect lanes enforce source order for shared atoms. Restart-the-form is
added if Phase 2's trace shows stack or inversion problems. Gate: identical
JavaScript and source maps on Penpot at every N with the 12-seed stress, the
`macros.rs` semantics including `1 2` without a blocker, peak RSS near 1x of
serial, and wall time compared with Stage A on the same commit. The pellets
are light-cljs-480 (effect lanes) and 481 (deferred units on one runtime
across the pool). Parallel top-level passes ordered by `need` are 475, which
is maybe-later and will be cut into focused pellets once 481 has numbers.

**Phase 5: worlds for watch and REPL.** A retained runtime creates a new
world per rebuild. Completed results stay stable for the tasks that hold
them. Recorded edges, which include ready reads, say exactly which units an
edit invalidates. Together with the persistent cache's namespace keys, this
gives per-unit incremental rebuilds. The pellet is light-cljs-476, which is
maybe-later.

## Risks

- **Per-thread speed may stall above *k* = 4.** That is the central risk.
  Measure *k* on real units in Phase 1. Do not wait for Phase 4. If the
  cljs-shaped kernels do not approach the target after Phase 0 and the first
  intrinsics, reassess before building Phase 4.
- **The heap redesign is invasive.** The collector, rooting and publication
  checks all assume the table lock today. The engine's root inventory and
  its exhaustive tests (`src/heap/inventory.rs`) are the safety net. Run them
  under multi-worker allocation, as `every_holder_retains_while_other_workers_allocate_and_collect`
  already does.
- **Stop-the-world collection on a shared heap.** Pauses stop every worker.
  Automatic triggering is deferred in the engine today, and a whole-build
  heap is larger than any one isolate's. Measure pause time and frequency in
  Phase 2.
- **Effect lanes might serialize more than expected.** A core library that
  touches a shared atom in every expansion would put everything on one lane.
  Measure lane wait in Phase 4. If a provider does this, give its atom a
  commutative contract, or keep the per-isolate isolation semantics for it,
  which the user has already accepted.
- **Non-shareable helper values.** Until Phase 3, many helper results will
  be ordinary JS objects. They take the per-task fallback, which keeps some
  of Stage A's repeated-initialization cost.
- **Browser and wasm.** The playground's wasm build has no threads by
  default. bbp runs single-threaded there, which is no worse than today, but
  it is a second configuration to test.

## Decisions

These were taken when the queue was planned on October 25. The user asked
for the planning and may overturn any of them.

1. **Effect lanes.** Source-order lanes for shared compile-time atoms are
   the contract for one-heap builds (light-cljs-480), so `--jobs N` has
   exactly the semantics of `--jobs 1` for atom effects. If lanes serialize
   too much, the fallback is the per-isolate isolation contract settled on
   October 22.
2. **Phase 0 placement.** The heap hot-path redesign is queued now, ahead of
   466 and 462, instead of waiting for 464 to find the object lock on real
   workloads.
3. **Forms.** `Datum` stays engine-independent behind a memoized bridge
   with an incrementally updated `&env` (light-cljs-462). Whether `Datum`
   nodes should become engine values is decided from Phase 2's bridge
   measurements.
4. **Unsafe code.** New `unsafe` in the heap is allowed when it is confined
   to one module with documented invariants (470, 471).

## Method and reproduction

These measurements were taken at `298325f8` (engine unchanged since the
import), with release profiles and `cargo build --release -p big-boy-pants-js`.
Node is the Homebrew `node` on the same machine. The runs are single runs on
a lightly loaded machine, enough for orders of magnitude and not for close
comparisons. Rerun them with `examples/bench` and the corpus from
light-cljs-465 before quoting any figure as a baseline.

Single-thread kernels, each wrapped in a function before running:

```js
// arithmetic
let i = 0; let s = 0;
while (i < 3000000) { s = s + i % 7; i = i + 1; }
return s;

// cons/map/count
function Cons(first, rest) { this.first = first; this.rest = rest; }
function cons(x, xs) { return new Cons(x, xs); }
function count(xs) { let n = 0; while (xs !== null) { n = n + 1; xs = xs.rest; } return n; }
function mapList(f, xs) { if (xs === null) return null; return cons(f(xs.first), mapList(f, xs.rest)); }
let total = 0;
for (let r = 0; r < 300; r = r + 1) {
  let xs = null;
  for (let i = 0; i < 1000; i = i + 1) xs = cons(i, xs);
  const ys = mapList(function (x) { return x + r; }, xs);
  total = total + count(ys);
}
return total;
```

They were run as `big-boy-pants-js FILE` and as
`node --jitless -e 'console.log(eval(fs.readFileSync(FILE,"utf8")))'`,
timed with `/usr/bin/time -p`. Profiles were taken with macOS `sample PID 5`.

The scaling probe was a scratch binary depending on the crate by path. One
`Runtime`; `std::thread::scope` with N threads, each calling
`register_thread`, then four times `spawn` followed by `run` to completion.
The functions are the kernels above, with 400,000 loop iterations and 40
list rounds respectively, taking the task index as an argument.
Throughput is tasks per second relative to one thread. The probe belongs in
the engine's `examples/bench` as a `scaling` row group when Phase 0 starts.

Stage A figures, the Penpot profile and the critical-path figures are from
[compiler-concurrency.md](compiler-concurrency.md) and
[benchmarks/compiler-concurrency.md](../benchmarks/compiler-concurrency.md).
The QuickJS ratio is from the October 22 quickjs-path baseline
(light-cljs-435). The *k* table is arithmetic on those figures, as stated
under "What has to be true for the bet to pay off".
