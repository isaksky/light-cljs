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

The chain is serial by construction, so this measures overhead, not
speedup, and both targets are missed. An 8-second sample at eight jobs is
mostly condition-variable and mutex waits; of the rest, about three quarters
is stop-the-world `Heap::mark_and_sweep` with its SipHash mark set, the cost
light-cljs-495 covers, which every worker waits out. Each worker slot that
analyzes a macro unit still boots its own task context, installs every
catalog registration into it and evaluates the source macros it calls, so
the heap grows with the worker count. The chain also retains about 1 MB per
macro call at `--jobs 1`, even without helpers (light-cljs-499). No
same-commit Stage A (V8) comparison was run.

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
