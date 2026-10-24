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
                                                 # persistent, memory, work, overhead, strings)
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
| `spawn_B/t`, `park_B/t`, `allocs/t` | counting allocator | Per consumer: spawning, then running to the park. Includes task-owned VM state (frames, native continuations), the task's root-source registration, its waiter entry in the cell, its own heap objects (a builder in `consumer.js`, the fold callback in `consumer_fold.js`) and its share of object-table growth |
| `kept_B/t` | counting allocator | Per consumer: runtime storage not given back after the tasks are dropped and a collection runs (object-table and registry capacity). Host storage is allocated before and kept after this interval, so it is excluded |
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
rose from 0.84 to 1.06 during this run, and the `work.*` rows show it (see
below).

```text
clj-quickjs measurement harness (examples/bench)
  build profile: release (debug_assertions off)
  target: linux x86_64; available parallelism: 4; cpu: AMD Ryzen 7 9850X3D 8-Core Processor
  mode: full; rounds: 1 warmup + 5 measured per row; executor workers: 2
  load average at start: 0.84 0.40 0.40
  RSS 2644 kB, high-water 2644 kB
  times: median [min .. max] over measured rounds, wall clock, single embedder thread unless a row says workers

task startup: spawn N tasks of identity.js, then run each to completion
  startup.spawn                  n=20000                     131.2 [124.0 .. 135.9] ns/task
  startup.first_run_to_complete  n=20000                     185.7 [183.2 .. 193.4] ns/task

already-ready dependency reads (cell completed before the timed run)
  reads.js_need_loop             reads=1000000               389.2 [388.2 .. 391.4] ns/iteration
  reads.js_baseline_loop         reads=1000000               248.8 [248.5 .. 251.0] ns/iteration
  reads.js_need_minus_baseline   reads=1000000               140.0 [138.2 .. 142.9] ns/read
  reads.rust_runguard_need       reads=1000000                99.7 [98.3 .. 100.3] ns/read

suspension and wake-up: one task needs N pending cells in turn, host completes each
  park.full_cycle                cycles=100000               835.7 [832.7 .. 848.8] ns/cycle
  park.resume_need_park          cycles=100000               687.7 [684.7 .. 700.9] ns/cycle
  park.complete_and_wake         cycles=100000               119.7 [119.5 .. 119.7] ns/cycle

native persistent operations from JavaScript (per loop iteration)
  persistent.vec_nth_loop        size=32 ops=1000000         301.2 [299.8 .. 303.4] ns/iteration
  persistent.vec_nth_minus_baseline size=32 ops=1000000          44.2 [41.6 .. 47.6] ns/op
  persistent.map_get_loop        size=32 ops=1000000         307.5 [305.3 .. 307.7] ns/iteration
  persistent.map_get_minus_baseline size=32 ops=1000000          50.8 [46.6 .. 51.8] ns/op
  persistent.baseline_loop       size=32 ops=1000000         255.9 [255.6 .. 258.7] ns/iteration
  persistent.vec_conj_loop       from empty, ops=100000      473.3 [469.8 .. 476.2] ns/iteration
  persistent.map_assoc_loop      from empty, ops=100000     1172.3 [1162.0 .. 1191.9] ns/iteration
  persistent.update_garbage_objects from empty, ops=100000   200002.0 [200002.0 .. 200002.0] objects left until a collection
  persistent.update_reclaimed    from empty, ops=100000   200064.0 [200064.0 .. 200064.0] objects reclaimed by it
  persistent.vec_nth_loop        size=1024 ops=1000000       300.5 [299.8 .. 304.7] ns/iteration
  persistent.vec_nth_minus_baseline size=1024 ops=1000000        47.1 [45.7 .. 50.0] ns/op
  persistent.map_get_loop        size=1024 ops=1000000       310.3 [309.1 .. 315.7] ns/iteration
  persistent.map_get_minus_baseline size=1024 ops=1000000        56.6 [55.5 .. 61.0] ns/op
  persistent.baseline_loop       size=1024 ops=1000000       253.9 [252.9 .. 254.8] ns/iteration
  persistent.vec_nth_loop        size=32768 ops=1000000      302.4 [302.1 .. 304.4] ns/iteration
  persistent.vec_nth_minus_baseline size=32768 ops=1000000       50.3 [48.7 .. 52.0] ns/op
  persistent.map_get_loop        size=32768 ops=1000000      340.0 [338.6 .. 348.6] ns/iteration
  persistent.map_get_minus_baseline size=32768 ops=1000000       89.6 [85.4 .. 97.4] ns/op
  persistent.baseline_loop       size=32768 ops=1000000      253.2 [250.4 .. 254.1] ns/iteration
  counted pass, ops=100000 from empty: vec_conj 1013.2 B/op (9.32 allocs/op), map_assoc 4005.9 B/op (9.14 allocs/op) still allocated at the end of each run; the collection after both changed allocated bytes by -496666952
  RSS 665880 kB, high-water 699296 kB

memory: N consumers parked on one pending cell, sharing one input form and one executable (counting allocator; deterministic, one round per row)
  consumer.js: parks at a need in its only frame, holding a builder
  consumers     input_B input_ob     code_B   spawn_B/t    park_B/t  allocs/t   heap_ob  heap_ob/t  vm_est_B   identical after_drop  kept_B/t
          1      611788     3001       1924       664.0       980.0     15.00      3002       1.00       552         1/1       3001     372.0
         10      611788     3001       1924       625.6       470.0     10.20      3011       1.00       552       10/10       3001      67.6
        100      611788     3001       1924       620.5       415.1      9.21      3101       1.00       552     100/100       3001      33.6
       1000      611788     3001       1924       616.4       416.0      9.03      4001       1.00       552   1000/1000       3001      90.4
      10000      611788     3001       1924       626.2       448.0      9.00     13001       1.00       552 10000/10000       3001      85.2
  at 10000 consumers: heap objects by kind ObjectCounts { plain: 0, native: 0, vector: 2001, map: 1000, occurrence: 0, builder: 10000, function: 0, cell: 0, atom: 0 }; known-shareable 3001; table slots 13001 (0 free); shallow table+object bytes 782184 (excludes trie bodies, code, task state); host-owned Vec<Task> 80000 B (excluded above); first consumer TaskStats { frames: 1, stack_values: 6, native_frames: 0, pending_need: true, shallow_bytes: 552 }
  consumer_fold.js: parks in a __rt.reduce callback: a native continuation plus a callback frame
  consumers     input_B input_ob     code_B   spawn_B/t    park_B/t  allocs/t   heap_ob  heap_ob/t  vm_est_B   identical after_drop  kept_B/t
          1      611788     3001       2540       680.0      1588.0     17.00      3002       1.00       736         1/1       3001     372.0
         10      611788     3001       2540       641.6      1078.0     12.20      3011       1.00       736       10/10       3001      61.2
        100      611788     3001       2540       636.5      1023.1     11.21      3101       1.00       736     100/100       3001      28.5
       1000      611788     3001       2540       632.4      1024.0     11.03      4001       1.00       736   1000/1000       3001      20.8
      10000      611788     3001       2540       642.2      1056.0     11.00     13001       1.00       736 10000/10000       3001      52.5
  at 10000 consumers: heap objects by kind ObjectCounts { plain: 0, native: 0, vector: 2001, map: 1000, occurrence: 0, builder: 0, function: 10000, cell: 0, atom: 0 }; known-shareable 13001; table slots 13001 (0 free); shallow table+object bytes 782184 (excludes trie bodies, code, task state); host-owned Vec<Task> 80000 B (excluded above); first consumer TaskStats { frames: 2, stack_values: 9, native_frames: 1, pending_need: true, shallow_bytes: 736 }
  RSS 665884 kB, high-water 699296 kB

useful work while consumers wait: 2 executor workers run work.js tasks alone, then while consumers are parked on one pending cell
  work.alone                     tasks=64 n=2000            1396.1 [863.7 .. 1507.6] us/work task (wall / tasks)
  work.while_consumers_parked    same, +1000 parked         1011.9 [967.6 .. 1097.6] us/work task (wall / tasks)
  work.consumers_still_parked    same, +1000 parked         1000.0 [1000.0 .. 1000.0] consumers after the work
  work.publish_to_all_resumed    same, +1000 parked         1282.6 [1209.2 .. 2017.4] ns/consumer (wall / consumers)

instrumentation overhead
  overhead.startup_counting_off  n=20000                     275.4 [274.2 .. 276.1] ns/task
  overhead.startup_counting_on   n=20000                     311.1 [311.0 .. 311.5] ns/task
  overhead.instant_now                                        18.2 [18.2 .. 18.2] ns/call
  overhead.heap_usage_snapshot   3001 objects               2680.2 [2662.5 .. 2690.3] ns/call
  overhead.task_stats_snapshot   unstarted task                8.8 [8.8 .. 8.9] ns/call

  load average at end: 1.06 0.49 0.43
```

The memory rows alone (`-- memory`, same build) print the same allocator
and object figures with an RSS high-water mark of about 22 MB: the RSS in
the full run above is dominated by the update rows that ran before it. An
earlier full run of the same harness on a quieter machine (load 0.2 to 0.5)
gave timing medians within about 10% of these, except `work.alone`
(890 us there).

## What these runs show, and what they do not

- **Consumers share the input and the code.** From 1 to 10,000 parked
  consumers the input stays 3001 objects and 611,788 bytes and the code
  1924 bytes (`consumer.js`) or 2540 bytes (`consumer_fold.js`), built
  once. Each consumer adds exactly one heap object of its own (its
  transient builder, or its fold callback function), and every consumer
  returned the input's entry with its identity. After dropping the tasks
  and collecting, only the 3001 input objects remain. `tests/measurement.rs`
  asserts these object-count facts (not the bytes) at 1, 8 and 64
  consumers for both fixtures.
- **Retained memory of a suspended task.** A consumer parked in its only
  frame costs about 1.04 to 1.07 KB of allocations once amortized (spawn
  about 620 B, run-to-park about 415 to 450 B), of which `Task::stats`
  accounts 552 B as VM state (one frame, six stack values). A consumer
  parked inside an `__rt.reduce` callback (a native continuation plus the
  callback frame: two frames, nine stack values, one native frame) costs
  about 1.67 to 1.70 KB, of which 736 B is VM state by the shallow
  estimate. The rest is per-task runtime bookkeeping (root-source
  registration, cell waiter entry, object slot, dependency log) and
  allocator granularity. The single-consumer rows also pay first-use
  growth. After the tasks are dropped and collected, 20 to 90 B per
  consumer stays allocated (`kept_B/t`: object-table and registry
  capacity, not returned).
- **Ready reads** cost about 140 ns each from JavaScript above the
  baseline loop, and about 100 ns through `RunGuard::need` from Rust (which
  also creates and drops a persistent root per read).
- **Suspension and wake-up**: one need-park-complete-wake-resume cycle on
  one thread is about 840 ns, of which completing the cell and delivering
  the wake is about 120 ns. Cross-thread handoff is not isolated here; the
  `work.publish_to_all_resumed` row (about 1.3 us per consumer with 2
  workers, including executor queueing and each consumer's run to
  completion) is the closest figure.
- **Useful work while consumers wait**: with 1000 consumers parked, all
  1000 were still parked after two workers finished the 64 `work.js` tasks,
  so waiting consumers occupied no worker. The throughput rows are noisy
  on this shared machine: in this run `work.alone` spread from 864 to
  1508 us per task, wider than the difference between the two rows, so this
  run supports "no worse with consumers parked", not any finer claim. On
  the quieter earlier run the two medians were 890 and 894 us.
- **Persistent operations**: vector `nth` costs about 44 to 50 ns above
  the baseline at every size measured; map `get` grows from about 51 ns (32
  entries) to about 90 ns (32,768). Updates are much more expensive in
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
  wake latency in isolation, `__rt.needAll` continuations (the fold row
  stands for resumable natives), and any comparison with another engine.

## Repeated string concatenation (clj-quickjs-146)

Run `CARGO_BUILD_JOBS=2 nix develop -c cargo run --release --example bench -- strings --rounds 3`
(or omit `nix develop -c` with the toolchain on PATH). The `strings` group
uses synthetic JavaScript, not light-cljs output. It builds increasing UTF-16
units by append or prepend, times the first contiguous read separately, then
times a JavaScript `charCodeAt` pass that checks every unit. Compilation,
runtime creation and spawning are outside the intervals; task/rope release
is also outside them. `--quick` uses 1k/2k/10k instead of 10k/20k/100k units.
Each row has one discarded warmup and three measured rounds.

Measured on 2026-10-24, release, Rust 1.95.0, Linux x86_64, AMD Ryzen 7
9850X3D (14 available logical CPUs), single embedder thread, other workers
active. Flat baseline was the parent implementation with only the new
benchmark workload added. Load averages at start/end: flat 3.27/3.27,
rope 3.69/3.69 (one-minute values). Medians [min..max], **ns per unit**:

| Direction / units | Flat build | Rope build | Rope first flatten |
| --- | ---: | ---: | ---: |
| append / 10,000 | 690.5 [681.8..692.3] | 728.7 [722.2..744.1] | 3.7 [3.6..6.0] |
| prepend / 10,000 | 730.1 [728.0..733.1] | 753.1 [742.2..805.6] | 3.2 [3.1..3.3] |
| append / 20,000 | 794.0 [792.5..794.2] | 742.7 [723.4..919.8] | 7.1 [6.5..10.4] |
| prepend / 20,000 | 841.9 [840.5..852.8] | 888.1 [734.3..908.3] | 3.1 [3.1..4.3] |
| append / 100,000 | 2428.6 [2412.0..2431.2] | 709.6 [703.0..758.3] | 14.5 [11.3..16.8] |
| prepend / 100,000 | 2481.8 [2480.2..2500.8] | 742.2 [732.6..746.5] | 3.9 [3.8..4.7] |

Flat first reads round to 0.0 ns/unit because their buffers already exist.
At 100k units, build plus first flatten was about 72.4 ms append / 74.6 ms
prepend, versus 242.9 / 248.2 ms for flat construction. Small inputs did not
consistently improve. Cached JavaScript reads also regressed slightly in this
sample: append 474.2 [473.2..476.5] flat versus 510.1 [505.2..514.8] rope,
prepend 475.1 [474.3..475.4] versus 512.0 [510.5..520.1] ns/unit at 100k.
These are noisy wall-clock observations, not complexity assertions.

`cargo test --test measurement string_concatenation_is_amortized_linear`
checks complexity deterministically: each direction constructs 100,000 units,
collects while the task is suspended partway through construction, flattens,
and checks every unit through JavaScript. Concat copies 527 short-string
units, then the first flatten copies 100,000; the asserted bound is 2N.
Subsequent indexed reads add zero copies. The thread-local diagnostic
`JsString::copied_code_units()` counts concat, lazy flatten and incremental
native-builder copies, excluding input construction and single-unit index
results. Differences must be taken on the thread doing the work. Tests also
exercise method concat, template interpolation, shared cache initialization,
retained prefixes and deep-chain flatten/drop on a 64 KiB native stack.

Ropes allocate a node per large concatenation and retain their children even
after flattening, so this trades memory and small-string overhead for linear
construction. This run does not quantify that memory tradeoff. First flatten
and final release remain synchronous; neither is an instruction-budgeted
latency measurement. No comparison with another JavaScript engine was made.
