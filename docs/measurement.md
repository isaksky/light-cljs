# Measurement

Reproducible cost measurements of this standalone runtime (`docs/design.md`
§15; pellet clj-quickjs-15). They measure this implementation only. Nothing
here is compared with light-cljs, QuickJS or V8: no matched comparison has
been performed, and upstream figures (for example the "Main Features"
section of `research/quickjs/doc/quickjs.texi`) are not results of this
project. `research/quickjs/tests/microbench.js` (revision 7d1c13c5d8f8)
informed only the reporting style (named per-operation rows, an empty-loop
reference row); none of its workloads, code or scores are used.

## Commands

```sh
cargo run --release --example bench              # full run: about 30 s on a quiet machine
cargo run --release --example bench -- --quick   # smaller sizes, 3 rounds: about 2 s
cargo run --release --example bench -- memory    # one row group (startup, reads, park,
                                                 # persistent, memory, work, overhead)
cargo run --release --example bench -- work --workers 4 --rounds 9
cargo test --test measurement                    # the workloads at small sizes, asserted
```

The harness is `examples/bench/main.rs` (timing, counting allocator,
report) and `examples/bench/workloads.rs` (the workloads, shared with
`tests/measurement.rs`). The JavaScript is the synthetic fixtures
`examples/bench/*.js` and `examples/e2e/work.js`, not light-cljs output.
It is std only: no benchmark framework or other dependency.

## Method

- **Rounds.** Each timing row runs one warmup round (discarded), then
  `--rounds` measured rounds (5 full, 3 quick), each in a fresh `Runtime`
  created outside the timed interval. It prints the median and the
  `[min .. max]` over the measured rounds, per operation. The spread is the
  variance to read; nothing is averaged away.
- **Setup is outside the timed interval.** Compilation, input building,
  spawning (except in the startup row), completing cells for ready reads
  and building the collections that lookups read all happen before the
  clock starts.
- **Work cannot disappear.** Every task's result is checked against a
  value computed in Rust (sums, counts, identities); the checks run after
  the timed interval.
- **Baseline loops.** JavaScript rows that run one intrinsic per loop
  iteration are paired with `loop.js`, the same loop with the intrinsic
  replaced by arithmetic; the `_minus_baseline` rows subtract it. The
  difference is an estimate, not an isolated instruction cost.
- **Threads.** Rows run on one embedder thread except the `work.*` rows,
  which use the reference executor with `--workers` workers (default 2).
- **Environment.** The report starts with the build profile, target,
  available parallelism, CPU model, mode, rounds and worker count, and
  prints the load average at start and end. Wall-clock times on a shared
  machine are noisy; a debug build says so in its header and is not
  representative.

### Memory figures and what they separate

| Figure | Source | Meaning |
|--------|--------|---------|
| `input_B`, `input_ob` | counting allocator, `HeapStats` | Shared input: building and rooting one form (3001 heap objects; persistent trie bodies included) |
| `code_B` | counting allocator | Shared code: compiling `consumer.js` once |
| `spawn_B/t`, `park_B/t`, `allocs/t` | counting allocator | Per consumer: spawning, then running to the park. Includes task-owned VM state, the task's root-source registration, its waiter entry in the cell, its builder object and its share of object-table growth |
| `heap_ob`, `heap_ob/t` | `HeapStats`, `HeapUsage` | Live heap objects while every consumer is parked, and the increase per consumer |
| `vm_est_B` | `Task::stats` | Shallow lower-bound estimate of one parked task's VM state (lengths times type sizes) |
| `Vec<Task>` | computed | Host-owned storage: the harness's task handles, allocated before the spawn phase and excluded from it |
| RSS, high-water | `/proc/self/status` | Process resident memory, including allocator caching and everything that ran before |
| `overhead.*` | timed | Instrumentation cost: counting allocator on vs off, `Instant::now`, `heap_usage`, `Task::stats` |

The counting allocator counts requested bytes (not allocator overhead) and
is switched on only for the memory rows and the counted update pass. It
uses relaxed atomics read by the measuring thread after the phase it
measured, which ran on that thread. Allocator figures are deterministic
for a given build; the memory rows run once.

### Read-only stats API

- `Runtime::heap_usage() -> HeapUsage`: live objects by kind
  (`ObjectCounts`), known-shareable objects, object-table slots and free
  slots, and shallow bytes the object table owns (slot and free-list
  capacity, one boxed object per live object). It excludes persistent trie
  bodies, atom cells, native payloads, executable code, task state and
  dependency cells. It walks every slot under the object lock: O(slots),
  for tests and measurement only.
- `Task::stats() -> Option<TaskStats>`: a suspended or unstarted task's
  saved frames, value-stack length, resumable native frames, whether a need
  is pending, and a shallow byte estimate. `None` while a run holds the
  task or after it finished. Like `Task::source_location`, it holds the
  task body for an instant.

Both are additive snapshots: they change no state, take no new locks
beyond the object lock and the task body, and do not alter `HeapStats`.

## Representative results

One full run, recorded as printed (`cargo run --release --example bench`,
2026-10-24). Environment: Linux 6.18.57, x86_64, AMD Ryzen 7 9850X3D (4
available threads), rustc 1.95.0, release profile. Two other workers were
building and testing on this machine during the session; the load average
during this run was 0.2 to 0.5.

```text
clj-quickjs measurement harness (examples/bench)
  build profile: release (debug_assertions off)
  target: linux x86_64; available parallelism: 4; cpu: AMD Ryzen 7 9850X3D 8-Core Processor
  mode: full; rounds: 1 warmup + 5 measured per row; executor workers: 2
  load average at start: 0.23 0.42 0.45
  RSS 2596 kB, high-water 2596 kB
  times: median [min .. max] over measured rounds, wall clock, single embedder thread unless a row says workers

task startup: spawn N tasks of identity.js, then run each to completion
  startup.spawn                  n=20000                     126.2 [120.9 .. 128.4] ns/task
  startup.first_run_to_complete  n=20000                     179.0 [178.3 .. 179.6] ns/task

already-ready dependency reads (cell completed before the timed run)
  reads.js_need_loop             reads=1000000               385.5 [383.8 .. 386.3] ns/iteration
  reads.js_baseline_loop         reads=1000000               249.8 [248.0 .. 250.0] ns/iteration
  reads.js_need_minus_baseline   reads=1000000               135.8 [135.2 .. 136.4] ns/read
  reads.rust_runguard_need       reads=1000000               100.0 [98.3 .. 100.3] ns/read

suspension and wake-up: one task needs N pending cells in turn, host completes each
  park.full_cycle                cycles=100000               846.0 [827.7 .. 847.2] ns/cycle
  park.resume_need_park          cycles=100000               698.4 [679.2 .. 699.6] ns/cycle
  park.complete_and_wake         cycles=100000               119.3 [118.7 .. 119.9] ns/cycle

native persistent operations from JavaScript (per loop iteration)
  persistent.vec_nth_loop        size=32 ops=1000000         311.2 [309.4 .. 313.1] ns/iteration
  persistent.vec_nth_minus_baseline size=32 ops=1000000          52.9 [51.5 .. 55.3] ns/op
  persistent.map_get_loop        size=32 ops=1000000         310.8 [309.5 .. 312.2] ns/iteration
  persistent.map_get_minus_baseline size=32 ops=1000000          53.1 [50.6 .. 54.2] ns/op
  persistent.baseline_loop       size=32 ops=1000000         257.0 [256.4 .. 261.7] ns/iteration
  persistent.vec_conj_loop       from empty, ops=100000      474.8 [473.0 .. 477.1] ns/iteration
  persistent.map_assoc_loop      from empty, ops=100000     1124.3 [1056.9 .. 1172.9] ns/iteration
  persistent.update_garbage_objects from empty, ops=100000   200002.0 [200002.0 .. 200002.0] objects left until a collection
  persistent.update_reclaimed    from empty, ops=100000   200064.0 [200064.0 .. 200064.0] objects reclaimed by it
  persistent.vec_nth_loop        size=1024 ops=1000000       306.9 [305.9 .. 312.1] ns/iteration
  persistent.vec_nth_minus_baseline size=1024 ops=1000000        53.1 [51.5 .. 54.7] ns/op
  persistent.map_get_loop        size=1024 ops=1000000       316.0 [314.7 .. 319.8] ns/iteration
  persistent.map_get_minus_baseline size=1024 ops=1000000        61.7 [61.0 .. 62.4] ns/op
  persistent.baseline_loop       size=1024 ops=1000000       254.5 [253.8 .. 257.4] ns/iteration
  persistent.vec_nth_loop        size=32768 ops=1000000      305.7 [304.6 .. 307.5] ns/iteration
  persistent.vec_nth_minus_baseline size=32768 ops=1000000       51.7 [49.5 .. 55.8] ns/op
  persistent.map_get_loop        size=32768 ops=1000000      344.5 [342.9 .. 345.4] ns/iteration
  persistent.map_get_minus_baseline size=32768 ops=1000000       91.0 [87.8 .. 93.7] ns/op
  persistent.baseline_loop       size=32768 ops=1000000      253.6 [251.7 .. 255.7] ns/iteration
  counted pass, ops=100000 from empty: vec_conj 1013.2 B/op (9.32 allocs/op), map_assoc 4005.9 B/op (9.14 allocs/op) still allocated at the end of each run; the collection after both changed allocated bytes by -496666952
  RSS 647628 kB, high-water 699280 kB

memory: N consumers of consumer.js parked on one pending cell, sharing one input form and one executable (counting allocator; deterministic, one round)
  consumers     input_B input_ob     code_B   spawn_B/t    park_B/t  allocs/t   heap_ob  heap_ob/t  vm_est_B  identical  after_drop
          1      611788     3001       1924       664.0       980.0     15.00      3002       1.00       552        1/1        3001
         10      611788     3001       1924       625.6       470.0     10.20      3011       1.00       552      10/10        3001
        100      611788     3001       1924       620.5       415.1      9.21      3101       1.00       552    100/100        3001
       1000      611788     3001       1924       616.4       416.0      9.03      4001       1.00       552  1000/1000        3001
      10000      611788     3001       1924       626.2       448.0      9.00     13001       1.00       552 10000/10000        3001
  at 10000 consumers: heap objects by kind ObjectCounts { plain: 0, native: 0, vector: 2001, map: 1000, occurrence: 0, builder: 10000, function: 0, cell: 0, atom: 0 }; known-shareable 3001; table slots 13001 (0 free); shallow table+object bytes 782184 (excludes trie bodies, code, task state); host-owned Vec<Task> 80000 B; probe change from spawn to after drop and collection 780452 B; first consumer TaskStats { frames: 1, stack_values: 6, native_frames: 0, pending_need: true, shallow_bytes: 552 }
  RSS 466672 kB, high-water 699280 kB

useful work while consumers wait: 2 executor workers run work.js tasks alone, then while consumers are parked on one pending cell
  work.alone                     tasks=64 n=2000             890.4 [867.3 .. 1252.5] us/work task (wall / tasks)
  work.while_consumers_parked    same, +1000 parked          893.7 [868.9 .. 964.5] us/work task (wall / tasks)
  work.consumers_still_parked    same, +1000 parked         1000.0 [1000.0 .. 1000.0] consumers after the work
  work.publish_to_all_resumed    same, +1000 parked         1273.7 [1254.1 .. 1507.1] ns/consumer (wall / consumers)

instrumentation overhead
  overhead.startup_counting_off  n=20000                     275.7 [275.3 .. 280.3] ns/task
  overhead.startup_counting_on   n=20000                     311.6 [311.2 .. 311.8] ns/task
  overhead.instant_now                                        18.2 [18.2 .. 18.2] ns/call
  overhead.heap_usage_snapshot   3001 objects               2656.2 [2643.2 .. 2774.0] ns/call
  overhead.task_stats_snapshot   unstarted task                8.8 [8.8 .. 8.9] ns/call

  load average at end: 0.50 0.47 0.47
```

The memory rows alone (`-- memory`, same day) printed identical allocator
and object figures and `RSS 11824 kB, high-water 15644 kB`: the RSS in the
full run above is dominated by the update rows that ran before it.

## What these runs show, and what they do not

- **Consumers share the input and the code.** From 1 to 10,000 parked
  consumers the input stays 3001 objects and 611,788 bytes and the code
  1924 bytes, built once. Each consumer adds exactly one heap object (its
  own transient builder) and about 1.0 to 1.1 KB of allocations (spawn
  about 620 B, run-to-park about 415 to 450 B once amortized; the
  single-consumer row also pays first-use growth). Every consumer returned
  the input's entry with its identity, and after dropping the tasks and
  collecting only the 3001 input objects remain. `tests/measurement.rs`
  asserts the object-count facts (not the bytes) at 1, 8 and 64 consumers.
  About 78 bytes per consumer stayed allocated after the drop and
  collection: object-table and registry capacity, which is not returned.
- **Retained memory of a suspended task** is about 1 KB here, of which
  `Task::stats` accounts 552 bytes as VM state (one frame, six stack
  values); the rest is per-task runtime bookkeeping and allocator
  granularity. Deeper frames, native continuations and builders add to it.
- **Ready reads** cost about 136 ns each from JavaScript above the
  baseline loop, and about 100 ns through `RunGuard::need` from Rust (which
  also creates and drops a persistent root per read).
- **Suspension and wake-up**: one need-park-complete-wake-resume cycle on
  one thread is about 850 ns, of which completing the cell and delivering
  the wake is about 120 ns. Cross-thread handoff is not isolated here; the
  `work.publish_to_all_resumed` row (about 1.3 us per consumer with 2
  workers, including executor queueing and each consumer's run to
  completion) is the closest figure.
- **Useful work while consumers wait**: with 1000 consumers parked, two
  workers completed the same `work.js` tasks at the same rate as with none
  (about 890 us per task, wall time over tasks), and all 1000 consumers
  were still parked afterwards: waiting consumers occupy no worker.
- **Persistent operations**: vector `nth` costs about 52 ns above the
  baseline at every size measured; map `get` grows from about 53 ns (32
  entries) to about 91 ns (32,768). Updates are much more expensive in
  memory than in time: every `conj` keeps about 1.0 KB and every `assoc`
  about 4.0 KB allocated until the next collection, because each
  intermediate version is a heap object and the collector runs only on
  request (no automatic trigger yet; README "Garbage collection"). The
  collection after the update rows reclaimed them, but the process RSS
  stayed high (allocator caching).
- **Instrumentation**: the counting allocator adds about 13% to task
  startup, so timing rows run with it off; `heap_usage` costs about 2.7 us
  at 3001 objects (linear in slots) and `Task::stats` about 9 ns.
- **Not measured**: contention between many workers on one cell or on the
  heap's object lock, collection pause times under load, cross-thread
  wake latency in isolation, and any comparison with another engine.
