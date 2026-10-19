# Compiler concurrency measurements

Numbers for multi-threaded compilation
([design](../docs/compiler-concurrency.md), "Gates and first steps"; the
research kit behind the design is in [concurrency/](concurrency/README.md)).
Every row is Penpot `app.main --optimize none` built by
[penpot.bb](penpot.bb) (`bb benchmarks/penpot.bb OUT main-none`) in a fresh
process, on the Apple M3 Ultra virtual machine (24 vCPUs, 32 GiB) other
workers share. Baseline and candidate runs alternate, with `uptime` load
recorded, and the binaries are immutable copies selected with
`LIGHT_CLJS_BINARY`. The gate for a pellet on the serial hot paths is +2%
wall and +3% peak RSS against its parent commit, on medians.

## Keyword provenance per macro call (light-cljs-379, October 19, 2026)

Interned keywords kept the provenance of the first form an isolate
imported them from, so Penpot main's source map depended on which isolate
analyzed which unit (docs/compiler-concurrency.md, "As built
(light-cljs-379)"). Bisected with Penpot `app.main --optimize none --stats
--source-map` and the pellet's three-line fixture:

| Binary | Penpot maps (`--jobs` 1, 8, 8) | Fixture maps (`--jobs` 1, 4, 8) |
| --- | --- | --- |
| `d2c6858` (light-cljs-338) | one | one |
| `19b789e` (light-cljs-339) | three | two (1 differs) |
| `b2cf31d` (master, parent) | three | two (1 differs) |
| This change | one, also at `--jobs 4` twice | one, also at 2 |

The JavaScript has one digest per binary in every run, and no build warns.
`bb benchmarks/jobs.bb OUT --jobs 1,8 --source-maps macros wide chain abnf`
(three rounds, load 4.6 to 7.5) gives one output and one map digest per
scenario: `macros` `171535049adac490/92e221109b4e2248`, `wide`
`efc72f3be4f07bca/d946fd64afb57c64`, `chain`
`fc2a4fe895bac093/73a5ca122381520c`, `abnf`
`2790787d86b1f104/e5e815e3e08fe73d`.

Serial gate, parent `b2cf31d` against this change (the runtime's form
bridge is the only functional difference), Penpot `main-none` with no
`--jobs`, four alternating pairs at 1-minute load 3.4 to 6.5:

| Build | Wall (s), round 1-4 | Median wall | Peak RSS (GB), round 1-4 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `b2cf31d` | 24.85, 23.42, 23.86, 32.94 | 24.36 | 0.925, 0.907, 0.911, 0.925 | 0.918 |
| Keywords per call | 24.66, 24.26, 24.86, 24.32 | 24.49 | 0.901, 0.936, 0.920, 0.938 | 0.928 |

The medians are +0.6% wall and +1.1% peak RSS, inside the +2% and +3%
gate (the parent's 32.94 s is one outlier under load).

## Stage A gate (light-cljs-343, October 19, 2026)

The benchmark that decides the `--jobs` default (docs/compiler-concurrency.md,
"Gates and first steps", item 4). [jobs.bb](jobs.bb) runs every
configuration once per round, rotating their order each round: the parent
commit's binary without `--jobs`, then the candidate at each `--jobs` value.
Penpot scenarios go through [penpot.bb](penpot.bb) (its 4 GB
`--macro-memory-limit` gives every slot an isolate); each output is hashed
and deleted. The candidate is the parent plus this pellet's `--stats`
counters, whose cost on the hot paths is one `try_lock` before the pool's
and the catalog's locks and a timestamp per pool job. One other worker
shared the machine throughout, so every set records its 1-minute load; no
set was free of it. Commands, one output directory per invocation:

```sh
bb benchmarks/jobs.bb OUT/penpot --parent PARENT --candidate CANDIDATE main-none main-advanced
bb benchmarks/jobs.bb OUT/fixtures --parent PARENT --candidate CANDIDATE small macros wide chain abnf
bb benchmarks/jobs.bb OUT/maps --candidate CANDIDATE --jobs 1,4,8 --source-maps main-none
bb benchmarks/jobs.bb OUT/stats --candidate CANDIDATE --rounds 1 --stats main-none
```

**Verdict: the gate is missed, so the default stays `--jobs 1`.**

| Gate | Needed | Measured | Holds? |
| --- | --- | --- | --- |
| Output across `--jobs 1/4/8` | identical, source maps included | JavaScript: one SHA-256 (`8cd7a9299b5490d3...`, 33,635,310 bytes) in all 73 Penpot `main-none` builds without a map at every N, parents included, and one (`f3b0885177569204...`) in the 8 with one; `--optimize advanced` one SHA-256 in all 36; `LIGHT_CLJS_STRESS_SEEDS=12 cargo nextest run --test compiler schedule_stress::` passes (12 tests at 1, 2, 4 and 8 slots, 76.9 s). Source maps: differ between N, and between two runs at the same N above one (light-cljs-379) | no (maps) |
| Penpot main wall at `--jobs 8` | at most 0.50x of `--jobs 1` | 0.79x, 0.83x and 0.84x in three sets | no |
| Peak RSS at `--jobs 8` | at most 1.5x of `--jobs 1` | 1.80x, 1.82x, 1.78x | no |
| Browser run of light-cljs-309's procedure | passes, if 309 landed | 309 was closed without the work: no procedure exists (docs/penpot-browser-bundle.md), so none was run | n/a |

### Penpot `app.main --optimize none`

Three sets of three rounds each. The **quiet set** ran after light-cljs-369
landed: parent `f74ee80`, load 2.0 to 3.3.

| Build | Wall (s), rounds 1-3 | Median | vs `--jobs 1` | User (s) | System (s) | Peak RSS (GB) | RSS vs `--jobs 1` |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Parent, no `--jobs` | 23.30, 23.37, 23.37 | 23.37 | 1.00x | 28.0 | 0.93 | 0.889 | 0.98x |
| `--jobs 1` | 23.19, 23.32, 23.32 | 23.32 | 1 | 28.0 | 0.92 | 0.907 | 1 |
| `--jobs 4` | 18.80, 18.93, 18.92 | 18.92 | 0.81x | 32.7 | 1.25 | 1.264 | 1.39x |
| `--jobs 8` | 18.97, 18.39, 18.32 | 18.39 | 0.79x | 37.4 | 1.79 | 1.634 | 1.80x |

The **review set**, with the review's fixes (queue waits and parse-pool
counts): parent `f74ee80`, load 2.3 to 3.3 in round 1 and 6.3 to 9.9 in
rounds 2 and 3, when the other worker's suite ran.

| Build | Wall (s), rounds 1-3 | Median | vs `--jobs 1` | User (s) | System (s) | Peak RSS (GB) | RSS vs `--jobs 1` |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Parent, no `--jobs` | 23.80, 25.25, 25.19 | 25.19 | 1.04x | 30.3 | 1.10 | 0.921 | 1.01x |
| `--jobs 1` | 23.54, 24.26, 25.13 | 24.26 | 1 | 29.2 | 1.03 | 0.915 | 1 |
| `--jobs 2` | 25.02, 27.96, 26.50 | 26.50 | 1.09x | 32.6 | 1.50 | 1.058 | 1.16x |
| `--jobs 4` | 19.13, 20.15, 20.31 | 20.15 | 0.83x | 34.9 | 1.47 | 1.286 | 1.41x |
| `--jobs 8` | 19.56, 20.24, 21.19 | 20.24 | 0.83x | 40.5 | 2.07 | 1.662 | 1.82x |
| `--jobs 12` | 21.21, 20.06, 21.23 | 21.21 | 0.87x | 45.6 | 3.68 | 2.094 | 2.29x |

The **first set**: parent `d816503`, load 3.2 to 5.2.

| Build | Wall (s), rounds 1-3 | Median | vs `--jobs 1` | User (s) | System (s) | Peak RSS (GB) | RSS vs `--jobs 1` |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Parent, no `--jobs` | 23.95, 24.71, 24.45 | 24.45 | 0.98x | 29.3 | 1.0 | 0.921 | 0.99x |
| `--jobs 1` | 24.99, 24.93, 24.73 | 24.93 | 1 | 30.0 | 1.1 | 0.931 | 1 |
| `--jobs 2` | 26.11, 26.13, 26.17 | 26.13 | 1.05x | 32.0 | 1.4 | 1.033 | 1.11x |
| `--jobs 4` | 19.96, 20.08, 20.14 | 20.08 | 0.81x | 34.5 | 1.4 | 1.287 | 1.38x |
| `--jobs 8` | 20.05, 21.07, 21.02 | 21.02 | 0.84x | 41.9 | 2.2 | 1.658 | 1.78x |
| `--jobs 12` | 20.07, 21.40, 21.15 | 21.15 | 0.85x | 45.8 | 3.5 | 2.063 | 2.22x |

- **Against the unchanged serial compiler.** `8ad35f6`, the commit before
  the scheduler (light-cljs-336), alternated with the first candidate at
  load 9.4 to 10.6: 26.40 s (26.27 to 26.96), 32.1 s user, 0.963 GB,
  against 25.23 s (25.23 to 25.75) at `--jobs 1` and 21.49 s (21.31 to
  21.55), 42.6 s user, 1.689 GB at `--jobs 8`. The parallel build is 1.23x
  faster than the compiler before stage A (0.81x its wall), and `--jobs 1`
  is 4% faster than it; its output differs, by light-cljs-338's
  merge-order change. A slower new baseline does not flatter the scaling.
- **Serial gate for the counters.** The first set and a four-round set at
  load 2.5 to 7.2 (parent 24.26 s, `--jobs 1` 23.86 s), seven pairs in
  all, give medians of 24.45 s and 24.73 s (+1.1%) and 0.921 and 0.931 GB
  (+1.1%); the quiet set gives -0.2% and +2.0%, and the review set -3.7%
  and -0.7%. Inside the +2% / +3% gate.
- **Two slots are slower than one** everywhere (1.05x to 1.09x here, 1.04x
  to 1.07x at advanced, 1.11x to 1.13x on the macro fixtures): one worker
  analyzes while the coordinator waits for it, so nothing overlaps but the
  transfer costs.
- **Source maps** (`--source-maps`, one round, load 2.8 to 3.7): the parent
  and `--jobs 1` give one map (`6bfb6e930e0da1bf...`); `--jobs 4` and `8`
  give others, and a second round gave two more, so the map depends on the
  schedule. The JavaScript stays `f3b0885177569204...` (the build with its
  `sourceMappingURL` line) at every N. Filed as light-cljs-379, with a
  three-line reproduction; light-cljs-371 fixed only macro-free units'
  scalars.

### Penpot `app.main --optimize advanced`

Two sets of three rounds; all 36 outputs have SHA-256
`61c1049decf41d58...`. The first: parent `d816503`, load 3.9 to 14.3 (the
other worker's suite started in round 2). The review set: parent `f74ee80`,
load 3.1 to 14.3.

| Build | Wall (s), first set | Median | Review set | Median | vs `--jobs 1` (first, review) | User (s) | System (s) | Peak RSS (GB) | RSS vs `--jobs 1` |
| --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Parent | 31.66, 32.84, 32.49 | 32.49 | 34.03, 32.09, 31.15 | 32.09 | 1.02x, 0.99x | 37.7 | 1.3 | 1.107 | 1.01x |
| `--jobs 1` | 31.80, 30.87, 32.18 | 31.80 | 31.13, 32.45, 32.26 | 32.26 | 1 | 36.7 | 1.3 | 1.100 | 1 |
| `--jobs 2` | 32.62, 42.14, 33.94 | 33.94 | 42.58, 33.68, 33.37 | 33.68 | 1.07x, 1.04x | 40.1 | 1.7 | 1.196 | 1.09x |
| `--jobs 4` | 25.93, 26.88, 27.61 | 26.88 | 31.61, 27.16, 26.91 | 27.16 | 0.85x, 0.84x | 41.5 | 1.5 | 1.433 | 1.30x |
| `--jobs 8` | 25.42, 28.62, 28.59 | 28.59 | 37.93, 27.95, 27.71 | 27.95 | 0.90x, 0.87x | 50.2 | 3.1 | 1.778 | 1.62x |
| `--jobs 12` | 26.68, 28.89, 29.52 | 28.89 | 27.08, 28.55, 25.96 | 27.08 | 0.91x, 0.84x | 54.9 | 4.1 | 2.181 | 1.98x |

User, system and RSS are the first set's medians; the review set's are
within 4% of them. Advanced preparation and emission run after analysis
on the calling thread, so the same ~5 s saving is a smaller share.

### What `--stats` says

One `--stats` round of the review set's candidate on `main-none`, at load
2.7 to 3.8, with light-cljs-369's worker rows and `wait` stage. The
`--jobs 1` trace has no pool.

| Counter | `--jobs 1` | 2 | 4 | 8 | 12 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `time.wall-ms` | 23,815 | 24,846 | 19,070 | 19,569 | 20,463 |
| `trace.top-level-ms` (main isolate) | 5,882 | 6,162 | 6,175 | 6,705 | 6,880 |
| `trace.wait-ms` (main isolate waiting for pooled units at drains) | 0 | 15,371 | 9,703 | 9,490 | 10,121 |
| `trace.engine-ms` (main isolate's V8) | 13,957 | 4,605 | 4,566 | 5,197 | 5,345 |
| `pool.workers` | | 1 | 3 | 7 | 11 |
| `time.worker-units-ms` (serial units: 16,765) | | 15,759 | 17,644 | 21,772 | 26,092 |
| `time.pool-ms` | | 23,948 | 18,215 | 18,718 | 19,596 |
| `pool.utilization` | | 0.658 | 0.323 | 0.166 | 0.121 |
| `time.worker-isolate-startup-ms` | | 43 | 133 | 399 | 741 |
| `pool.worker-initializations` (main isolate alone: 277) | | 273 | 551 | 1,090 | 1,615 |
| `pool.steals` / `pool.stolen` | | 0 / 0 | 1,456 / 1,785 | 3,415 / 4,303 | 4,631 / 5,908 |
| `time.pool.queue-wait-ms` (summed over 12,817 jobs) | | 232,676 | 81,900 | 45,808 | 37,821 |
| `time.pool.queue-wait.max-ms` | | 1,676 | 117 | 83 | 84 |
| `pool.lock-contended` | | 17 | 1,174 | 2,188 | 2,078 |
| `catalog.lock-contended` | | 1,226 | 1,856 | 3,934 | 5,853 |
| `parse-pool.jobs` | | 922 | 922 | 922 | 922 |
| `parse-pool.peak-admitted` | | 222 | 221 | 216 | 222 |
| `parse-pool.steals` | | 0 | 15 | 47 | 86 |
| `time.parse-pool.queue-wait-ms` (summed) / max | | 9,749 / 24.5 | 3,332 / 8.5 | 1,942 / 6.4 | 1,519 / 5.2 |
| `parse-pool.lock-contended` | | 2 | 59 | 213 | 251 |

At every N above one, 12,817 of 13,367 units go to the unit pool as one
batch per namespace, claimed in 3,567 claims; its peak admitted is the
widest drain, 304 units, and its backlog peaks at one batch. Utilization is
the workers' unit time over their capacity, `time.worker-units-ms /
(pool.workers x time.pool-ms)`. A queued unit waits 3.6 ms on average at
eight jobs (at most 83 ms), and a parse job well under a millisecond. Lock
contention is a few thousand acquisitions in a 19 s build, not a
bottleneck. The first set's `--stats` round (parent `d816503`, before
light-cljs-369, so its coordinator wait is in `trace.drain-ms`) agrees
within 7%: 10,150 ms of waiting, 22,772 ms of worker units and 0.165
utilization at eight jobs.

- **Critical path and the floor of this shape.** `dag.bb` on a `--jobs 1`
  `--stats-file` trace of the candidate gives a critical path of 2.92 s
  at namespace granularity and 1.73 s with units fully parallel, and
  Option U (serial top-level passes, units on 8 workers) 5.93 s. But the
  coordinator waits at each drain, so a namespace's units only overlap
  each other: with the trace's serial costs, each namespace's top-level
  pass plus `max(longest unit, unit total / W)` sums to 21.92 s at one
  worker, 16.56 s at 2, 14.36 s at 7 and 14.27 s at 11 (a 5.89 s floor of
  top-level passes, and 1.6 s in `instaparse.abnf`'s one unit). With about
  1 s of loading, preparation and emission the ideal `--jobs 8` build is
  about 15.4 s, 0.66x of `--jobs 1`: the 50% gate cannot be met in this
  shape at any N, however cheap the workers become.
- **Where the rest goes.** The quiet set's 18.4 s is 3 s above that ideal:
  units cost 21.8 s of worker time at eight slots against 15.8 s on one
  worker and 16.8 s serially (+38%), from cold isolates (0.40 s of
  start-up), helpers initialized again in each isolate (1,090 worker
  initializations, and 1,208 root executions against the serial build's
  277), and the shared machine's memory bandwidth at eight threads; the
  main isolate's top-level passes also slow by 10 to 15%.

### Fixtures

Generated by `jobs.bb` (`--optimize none`, the Penpot macro limits), the
review set's three rounds at load 3.4 to 4.6; each scenario's outputs have
one SHA-256 at every N. `/usr/bin/time` resolves 10 ms. The first set, at
load 8.1 to 12.0, agreed within 10% except `abnf` at eight jobs (1.16 s).

| Scenario | Build | Wall (s), rounds 1-3 | Median | vs `--jobs 1` | User (s) | System (s) | Peak RSS (MB) |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| `small`: one namespace, two definitions, pool start-up dominates | parent | 0.00, 0.00, 0.00 | 0.00 | | 0.00 | 0.00 | 11 |
| | `--jobs 1` | 0.00, 0.00, 0.00 | 0.00 | 1 | 0.00 | 0.00 | 11 |
| | `--jobs 2` | 0.05, 0.05, 0.05 | 0.05 | | 0.06 | 0.00 | 61 |
| | `--jobs 4` | 0.05, 0.05, 0.05 | 0.05 | | 0.12 | 0.01 | 91 |
| | `--jobs 8` | 0.05, 0.05, 0.05 | 0.05 | | 0.12 | 0.01 | 95 |
| | `--jobs 12` | 0.05, 0.05, 0.05 | 0.05 | | 0.11 | 0.01 | 94 |
| `macros`: 4 namespaces of 600 macro-expanding definitions | parent | 0.70, 0.69, 0.71 | 0.70 | 1.00x | 0.87 | 0.04 | 193 |
| | `--jobs 1` | 0.70, 0.68, 0.70 | 0.70 | 1 | 0.88 | 0.04 | 192 |
| | `--jobs 2` | 0.77, 0.78, 0.78 | 0.78 | 1.11x | 0.97 | 0.09 | 202 |
| | `--jobs 4` | 0.41, 0.42, 0.41 | 0.41 | 0.59x | 1.27 | 0.11 | 300 |
| | `--jobs 8` | 0.31, 0.30, 0.32 | 0.31 | 0.44x | 1.73 | 0.29 | 383 |
| | `--jobs 12` | 0.31, 0.30, 0.30 | 0.30 | 0.43x | 2.11 | 0.79 | 462 |
| `wide`: 64 independent namespaces of 60 such definitions | parent | 1.14, 1.11, 1.12 | 1.12 | 1.01x | 1.33 | 0.06 | 184 |
| | `--jobs 1` | 1.10, 1.11, 1.19 | 1.11 | 1 | 1.31 | 0.06 | 185 |
| | `--jobs 2` | 1.23, 1.27, 1.25 | 1.25 | 1.13x | 1.48 | 0.14 | 193 |
| | `--jobs 4` | 0.64, 0.65, 0.65 | 0.65 | 0.59x | 1.86 | 0.16 | 346 |
| | `--jobs 8` | 0.49, 0.49, 0.50 | 0.49 | 0.44x | 2.52 | 0.31 | 446 |
| | `--jobs 12` | 0.50, 0.47, 0.48 | 0.48 | 0.43x | 3.06 | 0.87 | 533 |
| `chain`: 64 namespaces, each requiring the previous | parent | 1.14, 1.15, 1.34 | 1.15 | 0.99x | 1.37 | 0.07 | 189 |
| | `--jobs 1` | 1.16, 1.16, 1.30 | 1.16 | 1 | 1.37 | 0.07 | 188 |
| | `--jobs 2` | 1.27, 1.30, 1.56 | 1.30 | 1.12x | 1.53 | 0.14 | 201 |
| | `--jobs 4` | 0.67, 0.67, 0.81 | 0.67 | 0.58x | 1.93 | 0.17 | 348 |
| | `--jobs 8` | 0.51, 0.51, 0.64 | 0.51 | 0.44x | 2.58 | 0.32 | 453 |
| | `--jobs 12` | 0.48, 0.49, 0.61 | 0.49 | 0.42x | 3.16 | 0.81 | 550 |
| `abnf`: Penpot's `instaparse.abnf` alone, one 1.6 s unit | parent | 1.15, 1.16, 1.13 | 1.15 | 1.00x | 1.48 | 0.05 | 180 |
| | `--jobs 1` | 1.16, 1.15, 1.14 | 1.15 | 1 | 1.48 | 0.05 | 180 |
| | `--jobs 2` | 1.18, 1.19, 1.34 | 1.19 | 1.03x | 1.55 | 0.06 | 190 |
| | `--jobs 4` | 1.11, 1.13, 1.13 | 1.13 | 0.98x | 1.71 | 0.08 | 249 |
| | `--jobs 8` | 1.36, 1.33, 1.35 | 1.35 | 1.17x | 2.35 | 0.14 | 355 |
| | `--jobs 12` | 1.10, 1.38, 1.36 | 1.36 | 1.18x | 2.72 | 0.28 | 457 |

Programs whose namespaces have hundreds of units and short top-level passes
reach 0.44x at eight jobs, inside the wall gate though not the memory one
(2.0x to 2.4x), but Penpot's units are spread over 914 drains averaging 15
units. A single huge unit gains nothing and can lose up to 0.2 s to the
other isolates' start-up, and every program pays about 50 ms and 80 MB for
the isolates. The fixtures' source maps differ between `--jobs 1` and `8`
(light-cljs-379, since fixed), except `small`'s.

- **The default macro memory limit.** The default `--macro-memory-limit`
  (64 MiB) gives two isolates, so a default `--jobs 8` would build as `--jobs 2`
  does, slower than one job: the `wide` fixture without the 4 GB limit takes
  1.22 s at `--jobs 1`, 1.40 s at 2 and 1.49 s at 8, with one worker
  (`pool.workers 1`). A flipped default would have had to raise
  it; with the default at 1 it stays 64 MiB.

## Unit traces from pool workers (light-cljs-369, October 19, 2026)

`--stats-file` keeps the units pool workers analyze, recorded where the
coordinator charges them, and blocked time is the new `wait` stage
(docs/compiler-concurrency.md, "As built (light-cljs-369)"). Parent
`7dca71e` (base) against this change (cand), alternating rounds at 1-minute
load 2.7 to 5.0.

| Configuration | Rounds | Base wall (s) | Cand wall (s) | Base RSS (GB) | Cand RSS (GB) |
| --- | --- | --- | --- | --- | --- |
| serial, untraced | 3 | 25.12 (23.46-25.20) | 24.52 (23.32-24.55) | 0.906 | 0.913 |
| serial, `--stats-file` | 4 | 25.03 (25.01-25.08) | 25.01 (24.56-25.30) | 0.927 | 0.946 |

The serial gate holds: -2.4% wall, +0.8% peak RSS. Unit rows in the trace:

| Configuration | Base | Cand |
| --- | --- | --- |
| serial | 13,361 | 13,361 |
| `--jobs 8` | 544, all on the coordinator | 13,361 (544 on the coordinator, 1,730 to 1,937 per worker) |

The trace's cost at `--jobs 8`, candidate traced against untraced, was
within the run-to-run spread, which is wide on the shared machine: medians
20.55 against 19.77 s over 3 rounds, 20.08 against 18.95 s over 5 (traced
range 18.44-20.87) and 20.94 against 21.03 s over 4. Untraced `--jobs 8` is
unchanged (cand 18.95 s against base 19.09 s over 5 rounds).

## Determinism stress (light-cljs-342, October 19, 2026)

The scheduling perturbation behind `tests/compiler/schedule_stress.rs`
(docs/compiler-concurrency.md, "As built (light-cljs-342)") is compiled
under `cfg(debug_assertions)`: a release binary keeps the parent's code
paths, apart from a pool panic's message, which no longer names the slot,
and a constant-false test in `Worker::push_local`. No Penpot rows were
taken: there is no serial-path change to gate, and two other workers were
testing on the machine. The suite's cost, with `cargo nextest run --test
compiler schedule_stress::` after the build, at 1-minute load 3.9 to 8.7
with two other workers active:

| Seeds | Cap per test | Tests | Shortest | Longest test = wall |
| --- | ---: | ---: | ---: | ---: |
| 3 (default) | 30 s | 12 | 0.1 s | 19.8 s |
| 12 (`LIGHT_CLJS_STRESS_SEEDS=12`) | 600 s | 12 | 1.0 s | 76.3 s |

The longest is the nine single-namespace fixtures of `macros.rs` at 1, 2, 4
and 8 slots in four modes per seed (144 builds per seed); the 30 s cap
(`LIGHT_CLJS_STRESS_SECONDS`) is not reached with three seeds and bounds the
run under load, where a test stops after the seed that crossed it.

## Recording on workers (light-cljs-374, October 19, 2026)

Capturing artifacts and recording a manifest keep macro-expanding units on
the workers' isolates (docs/compiler-concurrency.md, "As built
(light-cljs-374)"). Parent `02fc809` against this change, three
alternating rounds per configuration at 1-minute load 2.6 to 9.0; a cold
cache directory per run. All eighteen outputs are 33,626,443 bytes with one
digest.

| Build | Binary | Wall (s) | Median wall | Median peak RSS (GB) | Pooled units | Worker isolates |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| no cache, `--jobs 1` | parent | 24.09, 24.49, 26.76 | 24.49 | 0.918 | | |
| no cache, `--jobs 1` | this | 23.65, 23.76, 25.67 | 23.76 | 0.936 | | |
| cold `--cache-dir`, `--jobs 1` | parent | 25.96, 26.80, 27.06 | 26.80 | 0.931 | | |
| cold `--cache-dir`, `--jobs 1` | this | 26.09, 26.18, 26.71 | 26.18 | 0.918 | | |
| cold `--cache-dir`, `--jobs 8` | parent | 26.79, 27.62, 28.44 | 27.62 | 0.983 | 5,303 | 0 |
| cold `--cache-dir`, `--jobs 8` | this | 21.89, 24.79, 25.90 | 24.79 | 1.697 | 12,817 | 7 |

- **Serial gate.** Without the cache, -3.0% wall and +2.0% peak RSS on the
  medians, inside the +2% / +3% gate; with a cold cache, -2.3% wall and
  -1.4% peak RSS, though that build now reads the encode peak once more
  per outermost macro expansion.
- **Cold cache with eight jobs.** The parent's pool took only the 5,303
  macro-free units and made no worker isolate; this change gives the pool
  12,817 units, analyzed in seven worker isolates, for -10.2% wall on the
  median. Peak RSS grows with the isolates, +73%, as the uncached
  `--jobs 8` build's does: one shared budget, not a per-worker multiplier.
  The coordinator still waits at each drain (the 339 note), which bounds
  the gain.
- **`--jobs 8` without the cache** aborted at the parent, before any
  output, on Penpot's 4 GB `--macro-memory-limit`: a worker isolate's heap
  limit, a share of the budget, was not page aligned, and V8's code range
  check failed (`IsAligned(params.reservation_size, allocate_page_size)`).
  This change rounds an isolate's heap limit down to a MiB;
  `tests/cli.rs` `jobs_counts_work_slots_without_changing_output` builds
  with `--jobs 8 --macro-memory-limit 4000000000`.

## Diagnostics, limits and one memory budget (light-cljs-341, October 19, 2026)

Limits charged as running sums with exact JavaScript-macro errors, a soft
cap, one deadline, cancellation after a failure and one memory budget across
the isolates (docs/compiler-concurrency.md, "As built (light-cljs-341)").
The serial gate: parent `9bfe2a5` against this change at the default
`--jobs 1`, three alternating rounds at 1-minute load 6.4 to 22.7 with two
other workers testing, so the spread is wide. All six outputs are
33,626,443 bytes.

| Build | Wall (s) | Median wall | Peak RSS (GB) | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent | 26.16, 37.29, 27.97 | 27.97 | 0.922, 0.940, 0.960 | 0.940 |
| `--jobs 1` | 26.94, 26.49, 27.32 | 26.94 | 0.904, 0.981, 0.940 | 0.940 |

- **Serial gate.** -3.7% wall and +0.0% peak RSS on the medians, inside the
  +2% / +3% gate; the serial path gains only a thread-local check per
  JavaScript expansion and an empty link check per poll.
- **One budget.** A unit whose macro holds 96 MiB, at
  `--macro-memory-limit 268435456 --jobs 4` (four isolates): the parent
  fails it with the memory error (64 MiB even caps), this change builds it
  (32 MiB bases and 128 MiB of shared headroom), as both do at `--jobs 1`.

## Cache-blocker affinity (light-cljs-340, October 19, 2026)

A `--cache-blocker` namespace and the namespaces that require or name it
keep their units on the main isolate with `--jobs N`
(docs/compiler-concurrency.md, "As built (light-cljs-340)"). Penpot passes
no blocker, so the serial gate is all there is to measure: parent `3545cfb`
against this change at the default `--jobs 1`, three alternating rounds at
1-minute load 3.7 to 5.9 with one other worker active. All six outputs are
33,626,443 bytes with SHA-256 `360915b77d74c77c...`.

| Build | Wall (s) | Median wall | Peak RSS (GB) | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent | 27.05, 24.84, 23.96 | 24.84 | 0.906, 0.914, 0.912 | 0.912 |
| `--jobs 1` | 26.18, 24.63, 23.45 | 24.63 | 0.893, 0.921, 0.894 | 0.894 |

- **Serial gate.** -0.8% wall and -1.9% peak RSS on the medians, inside the
  +2% / +3% gate: without a blocker the loop does one empty check per
  namespace. A first set against `19b789e`, before the rebase, gave -2.5%
  and +1.0%.

## Per-worker isolates (light-cljs-339, October 19, 2026)

Every deferred unit but a `:test` definition's is analyzed by a pool worker
in a V8 isolate of its own, for the whole build, while the coordinator waits
for each in unit order at its drain (docs/compiler-concurrency.md, "As built
(light-cljs-339)"). Parent `d2c6858` against this change, three alternating
rounds of parent, `--jobs 1` and `--jobs 8`, at 1-minute load 9.2 to 13.8
with two other workers active. All ten outputs, and the fresh-isolate run
below, are 33,647,695 bytes with SHA-256 `e5a62f541b6c26f7...`: `--jobs 8`
emits the `--jobs 1` program.

| Build | Wall (s) | Median wall | User (s), median | Peak RSS (GB) | Median RSS |
| --- | --- | ---: | ---: | --- | ---: |
| Parent | 26.61, 26.13, 26.84 | 26.61 | 31.9 | 0.969, 0.946, 0.913 | 0.946 |
| `--jobs 1` | 26.14, 25.15, 26.69 | 26.14 | 31.4 | 0.927, 0.930, 0.973 | 0.930 |
| `--jobs 8` | 19.85, 22.15, 21.12 | 21.12 | 41.0 | 1.689, 1.705, 1.721 | 1.705 |

- **Serial gate.** `--jobs 1` is -1.8% wall and -1.7% peak RSS against the
  parent on the medians, inside the +2% / +3% gate: with one slot there is
  no pool and nothing new runs but a lock per unit for its charges.
- **Eight slots** (no gate yet; light-cljs-343 applies it): 0.79x the
  parent's wall and 0.81x of `--jobs 1`'s, for 1.31x the user time and
  1.83x the peak RSS. 12,817 of 13,367 units went to the pool; the 7
  isolates took 0.44 s in all to start, and initialized 1,090 compile-time
  units between them where the main isolate alone initializes 277. Their
  jobs took 21.8 s of thread time, against the 19 s the design's profile gives
  serial deferred units: cold isolates, and helpers initialized in each.
- **Why not more.** With `--stats` at eight slots the drains still take
  9.8 s of the coordinator's 19.3 s: it waits at each drain for that
  namespace's units, so the pool only ever has one namespace's units, and
  the 7.0 s of top-level passes do not overlap them. Going on past the
  drain needs the globals units add published after the next namespace's
  top-level pass, a second ID-order change ("Open questions" in the
  design).
- **After the review fixes, rebased onto `7b8e68a`**, six alternating
  rounds of parent and `--jobs 1` and three of `--jobs 8` at load 2.2 to
  6.8, all fifteen outputs 33,647,839 bytes with one SHA-256
  (`659ffab94af7e01e...`): parent 24.68 s / 0.896 GB median (23.52 to
  25.50 s), `--jobs 1` 25.13 s (+1.8%) / 0.922 GB (+3.0%; 23.56 to 29.52
  s), `--jobs 8` 19.84 s (0.79x of `--jobs 1`), 39.9 s user, 1.658 GB.
  The serial build is at the gate's edge on peak RSS; the first set above
  measured -1.7%.
- **A fresh isolate per unit** (`LIGHT_CLJS_FRESH_UNIT_ISOLATES=1`, the
  correctness reference) gives the same program in 120.5 s wall, 599.6 s
  user and 1.21 GB peak RSS at `--jobs 8`: 12,817 isolates, 169 s of summed
  start-up (13 ms each) and 49,449 initializations, since every unit
  initializes the helpers it needs again.
- **JIT share** per worker is not measured: the trace is per thread
  (light-cljs-369).

## Root read edges (light-cljs-359, October 19, 2026)

The trace now records every distinct (frame, unit) pair in which the engine
forced a unit's compile-time root or bundle, ready roots included (`read`
rows, trace version 2). On Penpot `app.main --optimize none`:

- **73,773 distinct read edges**, 33,224 from top-level passes and 40,549
  from units' analyses; **73,712 of them cross a namespace** (the demander's
  namespace is not the root's). They come from 3,772 demanders and reach
  only 277 distinct units, the same 277 whose roots are evaluated: the
  helpers Penpot's macros call are few, and nearly every macro-expanding
  unit and pass reads some of them after their first evaluation.
- **Stage A model.** As dependencies between `dag.bb`'s Option U tasks the
  execution and read edges give 73,770 edges (33,223 from a top-level
  pass). They change the schedule by at most 0.03 s: 16.48 s becomes
  16.50 s at W=1 and 8.24 s becomes 8.27 s at W=2, and from W=4 on both are
  6.05 s, the serial top-level passes. The helpers are drained long before
  their readers start, so no read waits on a running unit; what the edges
  do say is that each of the 277 helpers must be initialized in every
  worker isolate whose units expand a macro that calls it.

### Serial gate and the trace's overhead

Parent `1ae838b` against this change, alternating, at 1-minute load 3.0 to
12.0. Output is 33,628,234 bytes with the same SHA-256 in all 20 runs,
traced or not.

| Build | Wall (s) | Median wall | Peak RSS (GB) | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent, 7 rounds | 26.52, 24.22, 25.55, 24.31, 24.46, 25.97, 34.47 | 25.55 | 0.955, 0.917, 0.917, 0.928, 0.924, 0.914, 0.956 | 0.924 |
| Read edges, `--stats` off, 7 rounds | 26.74, 25.00, 26.52, 24.91, 25.63, 25.18, 27.04 | 25.63 | 0.978, 0.939, 0.936, 0.944, 0.912, 0.927, 0.976 | 0.939 |
| Parent, `--stats-file`, 3 rounds | 26.73, 24.99, 28.69 | 26.73 | 0.939, 0.909, 0.979 | 0.939 |
| Read edges, `--stats-file`, 3 rounds | 25.71, 24.42, 24.36 | 24.42 | 0.954, 0.900, 0.926 | 0.926 |

Untraced, the change is +0.3% wall and +1.6% peak RSS on the medians,
inside the +2% / +3% gate (the parent's 34.47 s run came at a load spike;
without it the medians are 25.01 s and 25.40 s, +1.6%). Untraced, a force
checks one variable and the bridge check one argument. Traced, recording
the reads costs nothing measurable against the parent's trace: notes stay
in JavaScript within an engine entry, and only pairs new since a unit's last
read cross into Rust, once per entry. The traced runs' spread (2.4 s for the
parent) is larger than any difference between them.

## Scalar provenance per macro call (light-cljs-371, October 19, 2026)

Parent `042d61b` (its `src/js_runtime.js`, the only functional difference)
against the bridge that gives each macro call its own scalar table, Penpot
`app.main --optimize none`, eight alternating pairs (parent first in rounds
1-4, candidate first in 5-8) at 1-minute load 5.1 to 15.3 while two other
workers were active. Each build's output has one SHA-256 in all eight runs;
they differ only in the runtime source that `--optimize none` embeds.

| Build | Wall (s), round 1-8 | Median wall | Peak RSS (GB), round 1-8 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `042d61b` | 24.99, 24.91, 25.19, 27.20, 27.45, 27.68, 26.70, 26.02 | 26.36 | 0.931, 0.939, 0.954, 0.979, 0.970, 0.995, 0.982, 0.952 | 0.962 |
| Per-call scalar tables | 24.47, 24.92, 27.75, 27.75, 27.69, 26.99, 26.93, 26.04 | 26.96 | 0.890, 0.923, 0.920, 0.946, 0.936, 0.950, 0.924, 0.911 | 0.923 |

The medians are +2.3% wall and -4.0% peak RSS; the median of the eight
paired differences is +0.13 s (+0.5%), and the load rose from 5 to 15
during the run. An earlier eight-pair run of the same mechanism at load 3.4
to 8.8 (which also registered the macro form's own scalars) gave medians of
25.68 s and 25.72 s (+0.2%, paired -0.16 s) and 0.941 and 0.912 GB
(-3.2%). The wall time is within noise of the +2% gate; peak RSS is lower
because a call's table holds only its own scalars and goes when its input
is released.

## Units merged after the next namespace's top-level pass (light-cljs-338, October 18, 2026)

Parent `66cbbf0` (light-cljs-335) against the whole of light-cljs-338 (the
macro-free units below, and each namespace's fragments merged after the
next namespace's analysis, with two-part artifacts), Penpot `app.main
--optimize none` at the default `--jobs 1`, three alternating pairs (parent
first) at 1-minute load 2.5 to 3.0 while two other workers were active,
before the review fixes, and three more at load 5.3 to 10.3 after them
(final).

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `66cbbf0` | 23.85, 24.29, 24.42 | 24.29 | 0.930, 0.923, 0.931 | 0.930 |
| Before review fixes | 23.68, 24.50, 24.80 | 24.50 | 0.910, 0.920, 0.935 | 0.920 |
| Parent `66cbbf0` | 26.32, 28.94, 26.78 | 26.78 | 0.979, 0.977, 0.943 | 0.977 |
| Final | 24.77, 25.95, 26.62 | 25.95 | 0.911, 0.957, 0.953 | 0.953 |

The medians are +0.9% wall and -1.2% peak RSS in the quiet set and -3.1%
and -2.5% in the loaded one, within the +2% and +3% gate. The parent's
output is 33,628,109 bytes and the final one 33,628,234, each the same in
its runs; they differ in 14,119 lines, the generated local and recur names
that follow expression IDs: the one ID-order change of stage A (the
globals a unit adds keep their IDs, `publish_globals`). With
`--source-map` and `--stats`, `--jobs 1`, `2` and `8` give byte-identical
JavaScript (SHA-256 `1fe5da19...`), source maps (`e95f9ee4...`) and
warnings (none) on the new order, in 26.3 s, 27.5 s and 27.5 s at load
about 6; 5,304 of the 13,367 units are macro-free, taking about 0.3 to
0.45 s of 17 s of deferred units at `--jobs 2` and `8`. Rebased onto
light-cljs-358 and light-cljs-371, `--jobs 1`, `2` and `8` again give
identical JavaScript (33,647,728 bytes, `93c6546d...`), source maps
(`8d2a3f10...`) and warnings (none), in 29.4 s, 28.9 s and 27.4 s at load
14 to 17. With 371's per-call scalar tables the fixtures' expression
spans and source maps are the same at every `--jobs` value too
(`same_at_every_slot_count`).

## Macro-free units on the pool (light-cljs-338, October 18, 2026)

Parent `ad8eebf` (light-cljs-335 on `916b75b`) against macro-free units
dispatched with `--jobs N`, Penpot `app.main --optimize none` at the
default `--jobs 1`, six alternating pairs (parent first) in two sets of
three, at 1-minute load 6.9 falling to 2.2 while two other workers were
active. Output is 33,624,590 bytes with the same SHA-256 in all twelve runs,
the parent's.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `ad8eebf` | 25.91, 24.55, 24.31, 24.87, 23.87, 23.79 | 24.43 | 0.934, 0.928, 0.934, 0.920, 0.937, 0.917 | 0.931 |
| Macro-free units | 25.45, 24.57, 26.05, 24.59, 23.74, 24.19 | 24.58 | 0.942, 0.946, 0.916, 0.933, 0.926, 0.909 | 0.930 |

The medians are +0.6% wall and -0.1% peak RSS (the first set alone, at the
higher load, +3.7% and +0.9%; the second +1.3% and +0.7%), within the +2%
and +3% gate: at `--jobs 1` no unit is judged, and the hot path gained a
lock per unit demand and a thread-local check per native expansion.

After the review fixes (each analyzed form now also checks that
thread-local, to record its poll in a macro-free unit) and a rebase onto
`52eb4f0`, whose output is 33,627,652 bytes, four more alternating pairs
against that parent `68cc85c` ran at load 6.9 to 19.8 while two other
workers ran suites: parent 29.64, 33.71, 25.88, 26.68 s, candidate 25.74,
40.29, 25.94, 26.46 s (medians 28.16 and 26.20 s); in the two quieter
rounds the pairs differ by +0.2% and -0.8%. Peak RSS medians are 0.959 GB
for both (-0.1%). All eight outputs have the same SHA-256. At `--jobs 1`,
`2` and `8` the JavaScript, source maps and warnings are again identical;
5,304 units are macro-free (a `js*` template with a named placeholder no
longer is).

With `--source-map` and `--stats`, `--jobs 1`, `2` and `8` give
byte-identical JavaScript (33,624,623 bytes with the map comment), source
maps (12,788,534 bytes) and warnings. At `--jobs 2` and `8`, 5,319 of the
13,367 units are macro-free (40%), and their analysis takes 310 ms and
520 ms (summed over the threads that ran them) of 19.2 s and 19.0 s of
deferred units; walls were 27.5 s, 27.8 s and 27.2 s at a load near 11, so
the first cut gives no measurable speedup, as the design expected.

## Shared session catalog (light-cljs-335, October 18, 2026)

Parent `00d9b70` against the catalog behind a `Mutex`, Penpot `app.main
--optimize none`, three alternating pairs (parent first) at 1-minute load
9.96 to 10.05 while two other workers were active. Output is 33,624,590
bytes with the same SHA-256 in all six runs.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `00d9b70` | 26.11, 25.80, 25.82 | 25.82 | 0.976, 0.960, 0.960 | 0.960 |
| Shared catalog | 25.86, 25.98, 26.14 | 25.98 | 0.976, 0.960, 0.955 | 0.960 |

The medians are +0.6% wall and 0.0% peak RSS, within the +2% and +3% gate.
Penpot registers about 13,000 units; each demand now takes an uncontended
lock two or three times where it borrowed a `RefCell`, and no thread waits.

## Host split (light-cljs-334, October 18, 2026)

Parent `6280d29` against the two-part macro host, Penpot `app.main
--optimize none`, six alternating pairs (parent first in rounds 1-3,
candidate first in 4-6) at 1-minute load 4.9 to 12.0 while two other
workers were active. Output is 33,624,590 bytes with the same SHA-256 in
all twelve runs.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `6280d29` | 26.90, 26.41, 26.31, 26.58, 24.67, 24.99 | 26.36 | 0.961, 0.973, 0.976, 0.959, 0.933, 0.934 | 0.960 |
| `MacroEnvironment` and `EngineBinding` | 26.29, 26.49, 26.44, 26.24, 24.81, 24.85 | 26.27 | 0.957, 0.971, 0.964, 0.944, 0.940, 0.927 | 0.951 |

The medians are -0.4% wall and -1.0% peak RSS, within the +2% and +3%
gate; the median of the six paired differences is -0.03 s, and both builds
ran 1.5 s faster once the load fell in rounds 5 and 6. A frame is built
from the same clones as before, and the platform is initialized once per
process either way.

## Generated names per unit (light-cljs-332, October 18, 2026)

Parent `8982972` against per-unit generated names, Penpot `app.main
--optimize none`, six alternating pairs (parent first in odd rounds) at
1-minute load 9.0 falling to 2.9 while two other workers were active.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `8982972` | 25.97, 25.72, 26.47, 25.31, 25.36, 26.43 | 25.85 | 0.933, 0.969, 0.945, 0.936, 0.928, 0.919 | 0.935 |
| Per-unit names | 25.53, 25.59, 27.01, 25.56, 25.80, 25.61 | 25.60 | 0.924, 0.921, 0.951, 0.927, 0.933, 0.955 | 0.930 |

The medians are -1.0% wall and -0.5% peak RSS, within the +2% and +3%
gate. Output grows from 33,610,277 to 33,622,574 bytes (+12,297, 0.04%):
the quoted symbols and names that spell a scope's 64-bit discriminator.
Both outputs are the same in every run of their build. The scope switch
costs one engine call per change of the heap's scope, not per expansion.

## Parse queue (light-cljs-337, October 18, 2026)

Parent `460b91a` against the parse queue, Penpot `app.main --optimize
none`, six alternating pairs (parent first in rounds 1-3, candidate first in
4-6) at 1-minute load 3.0 to 10.2 while two other workers were active.
Output is 33,604,484 bytes with the same SHA-256 in every run below, at
`--jobs 1` and `--jobs 8` too.

| Build | Wall (s), round 1-6 | Median wall | Peak RSS (GB), round 1-6 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `460b91a` | 25.73, 24.05, 28.29, 24.88, 27.02, 26.85 | 26.29 | 0.924, 0.949, 0.976, 0.940, 0.981, 0.962 | 0.956 |
| Parse queue, default `--jobs 1` | 24.62, 32.77, 26.98, 25.65, 27.77, 27.02 | 27.00 | 0.938, 0.948, 0.987, 0.956, 0.941, 0.983 | 0.952 |

The medians are +2.7% wall and -0.4% peak RSS; the median of the six paired
differences is +0.46 s (+1.7%), and round 2's 32.77 s is an outlier the
load does not explain. The change cannot account for that much: the whole
load phase is about 0.1 s. Three more alternating pairs with `--stats`
measured it directly, and there the build is even:

| Build | `trace.load-ms`, 3 runs | `trace.wall-ms`, 3 runs |
| --- | --- | --- |
| Parent `460b91a` | 101.1, 103.3, 100.7 | 25,038.6, 26,335.4, 25,950.6 |
| Parse queue, `--jobs 1` | 137.2, 103.5, 104.6 | 26,312.8, 25,160.6, 25,972.7 |

The median load phase is 103.3 ms against 104.6 ms (+1.3 ms for the inline
pool, the result table and the replay), and the median trace wall is
25.95 s against 25.97 s.

At `--jobs 8` the load phase (the coordinator's wall time from the first
parse job to the end of the replay, `trace.load-ms`) is 34.1, 36.3 and
34.8 ms, against 101.8, 104.8 and 104.1 ms at `--jobs 1` in the same
alternating runs: 3.0x, from 7 workers. It is 0.1% of the build either
way. It is probably held back by the graph (not profiled): the entry's file
must be parsed before anything else is known, and each level of requires
waits for its parent's job, so the frontier is narrow at first; and the
coordinator replays the walk and records the result serially.

## `Arc` instead of `Rc` (light-cljs-333, October 18, 2026)

Parent `d6439d4` against the `Arc` change with each `Arena` lookup hint
choice, three alternating rounds (parent, relaxed atomic, per-thread) at
1-minute load 11.8 to 12.5 (half the machine; another worker was building V8).
Output is 33,588,607 bytes with the same SHA-256 in all nine runs.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `d6439d4` (`Rc`, `Cell` hint) | 26.30, 25.03, 25.20 | 25.20 | 1.048, 1.039, 1.040 | 1.040 |
| `Arc`, relaxed `AtomicUsize` hint (landed) | 26.25, 25.33, 25.50 | 25.33 | 1.059, 1.056, 1.040 | 1.056 |
| `Arc`, per-thread hint keyed by arena | 25.95, 25.51, 25.48 | 25.51 | 1.039, 1.043, 1.018 | 1.039 |

The landed change is +0.5% wall and +1.6% peak RSS on the medians, within
the gate and within the run-to-run spread (1.3 s wall for the parent, 19 MB
RSS for the candidate). `Arc` and `Rc` have the same size, and a relaxed
load or store is a plain load or store on arm64 and x86-64, so no difference
is expected. The per-thread hint is a `thread_local!` (owner, chunk) pair
keyed by the arena's `starts` pointer: it was +1.2% wall, probably because
one hint per thread is shared by every arena that thread reads, so
interleaved reads of `exprs`, `locals` and `globals` evict each other (not
profiled). The atomic keeps one hint per arena, as the `Cell` did.

A second set of three alternating pairs, started when the machine had gone
quiet (load 6.3) but run at load 12.5 to 19 once other workers resumed; the
first parent run (30.23 s) is an outlier, probably a cold file cache after
the idle spell; the median does not use it. The machine was not quiet for a
whole set at any point that day.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `d6439d4` | 30.23, 25.43, 25.26 | 25.43 | 1.015, 1.031, 1.015 | 1.015 |
| `Arc`, relaxed atomic hint | 25.10, 25.42, 25.49 | 25.42 | 1.028, 1.019, 1.029 | 1.028 |

Here the change is -0.04% wall and +1.3% peak RSS, again within the gate,
with the same output in all six runs.

## One-worker trace (light-cljs-331, October 18, 2026)

The baseline every later scaling claim is measured against: the
exclusive-stage trace that `--stats` now records, written with
`--stats-file` and read by
[concurrency/dag.bb](concurrency/README.md#reading-a-trace-instead-of-probes).

### Serial gate and the trace's overhead

Parent `ae5045c` against the trace change without `--stats`, and the change
with `--stats-file`, three alternating rounds (parent, candidate, candidate
traced) at 1-minute load 9.2 to 13.1. Output is 33,588,607 bytes with the same
SHA-256 in all nine runs.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `ae5045c` | 26.34, 25.47, 26.02 | 26.02 | 0.996, 1.009, 1.006 | 1.006 |
| Trace change, `--stats` off | 27.25, 25.64, 25.94 | 25.94 | 0.997, 0.984, 1.001 | 0.997 |
| Trace change, `--stats-file` | 25.70, 26.25, 26.17 | 26.17 | 1.014, 1.036, 0.997 | 1.014 |

With `--stats` off the change is -0.3% wall and -0.9% peak RSS on the
medians, inside the +2% / +3% gate and the run-to-run spread (1.6 s): every
hook loads one relaxed atomic and returns, with no clock read and no
allocation. Tracing costs +0.9% wall and +1.7% RSS over the same binary
untraced: two clock reads per V8 service call (1.29 M calls) and 13,361 unit
rows.

The same three rounds again with the final binary (after review: the
recorder is checked per thread before any unit information is built, and
root evaluations are recorded), once the machine had quietened (load 2.7 to
6.9). Output is identical in all nine runs again.

| Build | Wall (s), round 1-3 | Median wall | Peak RSS (GB), round 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `ae5045c` | 26.87, 22.74, 24.05 | 24.05 | 0.969, 0.976, 0.967 | 0.969 |
| Trace change, `--stats` off | 23.77, 23.53, 25.27 | 23.77 | 0.959, 0.959, 0.948 | 0.959 |
| Trace change, `--stats-file` | 22.30, 24.05, 24.58 | 24.05 | 0.959, 0.951, 0.955 | 0.955 |

Here the change is -1.2% wall and -1.0% RSS untraced, and tracing adds
+1.2% wall and -0.4% RSS, all within a spread of 1.7 to 4.1 s. A lone traced
build on the quiet machine took 22.31 s and 0.954 GB.

### Totals against the design document's probe run

The design document's probe run was at `b51af6a` on October 17 (23.76 s wall
under the probes). To compare at one commit, the probes'
`NSPREP`/`NSPROBE` timers (without their V8 counters) were re-applied
temporarily on top of this change, and one build (26.31 s wall, load 11)
printed both the probe lines and the trace. `dag.bb` reads either; from the
trace it rebuilds the probes' inclusive columns.

| Quantity | Design (`b51af6a`) | Probes, same run | Trace, same run |
| --- | ---: | ---: | ---: |
| Top-level passes, with demanded units | 6.00 s | 6.56 s | 6.61 s |
| Drains, with their units | 16.53 s | 18.39 s | 18.39 s |
| Units | 13,361 | 13,361 | 13,361 |
| Drained units' own analysis | 15.86 s | 17.65 s | 17.53 s |
| V8 at depth 0 | 13.62 s | | 14.67 s |
| V8 service entries | 1,288,464 | | 1,288,469 |

At the same commit the trace and the probes agree: drains to the
millisecond, top-level passes +0.8% (the trace's frame starts before the
`#_` re-parse and the refer checks, the probe's timer after them), units
-0.7% (the trace times a unit from after its state check and macro-frame
copy; that setup, 9 µs a unit, is drain bookkeeping in the trace). Against
the design document everything is about 10% slower, uniformly: the parent
binary itself takes 26.0 s here against 23.2 s then, so the difference is
the busier machine and a day of commits, not the measurement.

### What the trace says

From the first traced gate run (25.70 s wall; the `trace.*` lines of its
report):

- **Exclusive stages** (they add up to the 24.74 s traced): deferred units
  17.13 s (10.70 s of it V8), top-level passes 6.52 s (3.78 s V8), drain
  bookkeeping 0.82 s (append 0.41, release 0.28, drain loop 0.12, index
  0.01), loading 0.11 s, other 0.12 s, warnings 0.03 s, preparing sources
  0.01 s. After analysis: preparation 0.10 s, emission 0.72 s.
- **Analysis demand edges are nearly absent.** 13,359 of the 13,361 units
  are started by their namespace's drain. Only `instaparse.gll/PROFILE`
  (0.1 ms) is demanded by a top-level pass and `cuerdas.core/repeat` (0.1 ms)
  by another unit; the top-level passes waited 0.1 ms in all. The helpers
  Penpot's macros call are drained in earlier namespaces before they are
  needed.
- **Execution edges.** 277 units' compile-time roots are evaluated (each
  once, in the one isolate), 240 first by a unit's analysis and 37 by a
  top-level pass; 258 of them belong to another namespace than their
  demander, and 2 had to be analyzed on the spot. These are the helpers a
  worker isolate must initialize again (the later reads of ready roots are
  recorded since light-cljs-359; see "Root read edges").
- **Macro host share.** 7,335 units (54.9%) entered the macro host, and they
  are 97.7% of unit time (16.74 of 17.13 s). The 6,026 macro-free units take
  0.39 s, so light-cljs-338's narrower first cut can move about 2% of the
  unit time. Only 2,797 units (0.12 s, from the final binary's trace) make no
  macro call at all; the others expand native macros such as `fn` and `let`,
  in Rust.
- **Longest unit**: `instaparse.abnf/abnf-parser` (line 129), 2.09 s, in the
  macro host; then `app.plugins.shape/shape-proxy` 0.50 s and
  `app.plugins.api/create-context` 0.12 s. Three units exceed 100 ms, 234
  exceed 10 ms.
- **Ready width per drain.** 914 drains start units; the median drain has 6
  ready units (p90 34, max 304 in `app.main.ui.ds.foundations.assets.icon`),
  and the median count of macro-free ones is 2 (p90 16). The widest drains
  are mostly cheap: `app.main.ui.icons` has 272 host units worth 0.28 s.
- **Lower bounds.** With a barrier at each drain, the serial rest (7.38 s)
  plus per drain `max(W/P, longest unit)` gives 17.04 s at P=4 and 16.53 s at
  P=12: one long unit per drain dominates. Without the barrier, the existing
  models on the trace give 6.58 s for stage A (`Option U`, units on 4 or more
  workers behind serial top-level passes) and 2.96 s for stage C at 8 workers
  (`Model C`), against 6.06 s and 2.74 s from the probe data.
