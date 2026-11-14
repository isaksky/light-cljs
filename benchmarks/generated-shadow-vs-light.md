# Generated projects: light-cljs vs shadow-cljs, advanced builds (October 22, 2026)

Projects from [the generator](generated-project/README.md), seed 42, at 5k, 200k
and 800k nonblank lines. Both compilers build at advanced: `light-cljs release
bench` (default `:advanced`) and `clojure -M:shadow release bench` (Closure
advanced). Every output printed the oracle's checksum for input 42 and 3
iterations, and `check-waste.bb --strict` passed for both. The tables up to
Findings are the baseline at `ac88cb2`; [the re-measurement at
`76c6366`](#re-measured-after-the-scaling-pellets-october-22-2026-at-76c6366)
follows them, and [the head-to-head at
`81f9837`](#re-measured-at-81f9837-october-23-2026) after that.

The [October 27 regression check and repair](#default-v8-regression-check-october-27-2026)
below records the later default-compiler regression and the restored timings.
The [October 29 comparison at `0ad2514f`](#re-measured-at-0ad2514f-cached-and-uncached-october-29-2026)
is the last on a reserved machine, with cached builds and leaf edits for both compilers.
The [five-round baseline at `627b9ed5`](#five-round-baseline-at-627b9ed5-unreserved-machine-november-13-2026),
is the latest full comparison: medians and ranges with CPU time, on an unreserved machine.
The [heap sweep of November 14](#shadow-cljss-maximum-heap-swept-unreserved-machine-november-14-2026),
compares peak memory against shadow-cljs's smallest adequate heap instead of `-Xmx8g`.
[Growth from 50k to 800k lines](#growth-from-50k-to-800k-lines-both-shapes-one-binary-one-day-unreserved-machine-november-14-2026),
at the end, fits how both compilers' time and memory grow over five sizes in both generated shapes, on one binary and one day.

Reproduce, one project at a time and with no other heavy work running:

```sh
bb benchmarks/generated-project/generate.bb /tmp/g200k --seed 42 --lines 200k
bb benchmarks/generated-project/compare.bb /tmp/g200k --light target/release/light-cljs --runs 3 --warm-jvm 4
```

## Setup

- 24-core Mac, macOS 15.7.3, 32 GB, OpenJDK 25.0.2. One job at a time.
- light-cljs built from `ac88cb2`. shadow-cljs 3.5.3 with the generated `-Xmx8g`.
- Uncached: light `--no-cache` with `.light-cljs` removed; shadow with
  `.shadow-cljs/` removed. Cached: one warm-up build, then rebuilds with no source
  change. Light reuses its whole output then; shadow still runs Closure.
- Shadow command-line times include JVM startup. The in-JVM rows repeat
  `shadow.cljs.devtools.api/release` in one JVM, as a running shadow server does.
- Medians of 5 runs (5k), 3 runs (200k) and 1 run (800k). Peak RSS from `/usr/bin/time -l`.

## Build time and peak memory

| Size | light uncached | light cached | shadow uncached | shadow cached | shadow in one JVM (1st / steady) |
|---|---|---|---|---|---|
| 5k (5,046 lines, 11 workers) | 0.43 s, 0.15 GB | 0.09 s, 0.05 GB | 10.2 s, 1.53 GB | 8.1 s, 1.00 GB | 4.6 s / 1.7 s |
| 200k (199,129 lines, 450 workers) | 29.9 s, 1.18 GB | 4.8 s, 1.02 GB | 43.6 s, 4.02 GB | 35.4 s, 3.30 GB | 29.5 s / 24.4 s, 5.3 GB |
| 800k (795,693 lines, 1,808 workers) | 310 s, 6.37 GB | 30.2 s, 4.05 GB | 181 s, 7.59 GB | 162 s, 7.93 GB | not run |

At 800k shadow's first uncached build took 422 s at 8.6 GB, close to its 8 GB
heap; the 181 s row is the second, ordinary run.

## Output size

| Size | light bytes / gzip -9 | shadow bytes / gzip -9 | light gzip vs shadow |
|---|---|---|---|
| 5k | 214,145 / 66,471 | 263,193 / 57,701 | +15% |
| 200k | 4,097,518 / 1,261,793 | 4,067,637 / 1,002,909 | +26% |
| 800k | 16,149,220 / 4,930,779 | 15,710,764 / 3,894,315 | +27% |

Waste markers kept at 200k (all `:removable` markers were removed by both):

| Category | light | shadow |
|---|---|---|
| unused arities (best-effort) | 86 / 204 | 51 / 204 |
| unreachable defmethods (retained) | 406 / 406 | 406 / 406 |

At 800k: unused arities 329 / 779 (light) and 215 / 779 (shadow).

## Findings

1. **Uncached compile time is superlinear in light-cljs.** 200k to 800k is 4x the
   code; light takes 10.4x longer (29.9 s to 310 s) and shadow 4.2x. Light wins at
   200k and loses at 800k.
2. **A cached rebuild with no change also grows superlinearly**: 0.09 s, 4.8 s,
   30.2 s (6.3x from 200k to 800k) and 4 GB at 800k, though nothing is compiled.
   Since light-cljs-418 such a build writes its stored output instead of
   preparing and emitting the application: 0.58 s at 100k and 2.49 s at 400k
   (4.3x), at 1.05 GB, 44% of an uncached build's peak
   ([below](#cached-rebuild-with-no-change-light-cljs-418)).
3. **gzip is 26-27% worse than shadow at the same raw size.** At 200k light's
   output has 98,232 distinct identifiers against shadow's 26,279: locals get
   program-unique names (`tVY`, `tVZ`, ...) where Closure reuses short names in
   each function scope. Light also emits 9,410 `(function(){...})()` expression
   wrappers and 3,177 calls to the truthiness helper. Since light-cljs-419
   locals are named per scope: 30,173 distinct identifiers at 200k, and gzip
   863,513 (14% under shadow) at 200k and 3,294,722 (15% under) at 800k
   ("Locals named per scope" in `benchmarks/compiler.md`). Since
   light-cljs-420 992 wrappers are left at 200k (8,451 at `53cdb94`) and
   3,904 at 800k, the output is 6.2% and 6.4% smaller raw, but gzip grows
   to 898,739 (10% under shadow) and 3,444,007 (12% under): with locals
   named per scope the wrappers' fresh scopes compressed well
   ("Value-position statements without wrappers" in `benchmarks/compiler.md`).
   Since light-cljs-424 operand locals take one name in every function and
   loop labels number per function: gzip 860,671 at 200k (14% under shadow)
   and 3,302,076 at 800k (15% under), raw 3,456,905 and 13,396,492 ("Names
   that repeat across functions" in `benchmarks/compiler.md`).
4. **Light keeps more unused arities** (86 vs 51 at 200k).
   Since light-cljs-421 light keeps 49 at 200k and 202 of 779 at 800k (shadow
   51 and 215): methods that no call selects of functions without a
   dispatcher, and dispatchers kept only for an unread alias, are gone
   ([details](compiler.md#unused-arities-without-a-dispatcher-and-unread-aliases--october-22-2026)).

## Re-measured after the scaling pellets (October 22, 2026, at `76c6366`)

The same comparison again, after light-cljs-415 to 421 and 423 to 425, with
light-cljs built from `76c6366` (light-cljs-417). The baseline above is at
`ac88cb2`. Same machine, generator, seed and shadow-cljs 3.5.3 with
`-Xmx8g`; the Clojure CLI reused the baseline's resolved classpath, so
nothing was downloaded. `compare.bb` with `--runs 5 --warm-jvm 4` at 5k,
`--runs 3 --warm-jvm 4` at 200k and `--runs 3` at 800k, one project at a
time and nothing else running; the 1-minute load average was 2.3-5.3 (other
idle sessions). Every output matched the oracle and `check-waste.bb
--strict` passed for both compilers. Medians, with the baseline in
parentheses.

| Size | light uncached | light cached | shadow uncached | shadow cached | shadow in one JVM (1st / steady) |
|---|---|---|---|---|---|
| 5k | 0.33 s, 0.14 GB (0.43 s) | 0.03 s, 0.03 GB (0.09 s) | 9.5 s, 1.46 GB (10.2 s) | 7.5 s, 0.97 GB (8.1 s) | 4.5 s / 1.7 s |
| 200k | 12.4 s, 1.18 GB (29.9 s) | 1.09 s, 0.54 GB (4.8 s) | 40.4 s, 4.09 GB (43.6 s) | 32.9 s, 4.54 GB (35.4 s) | 29.2 s / 24.0 s |
| 800k | 67.9 s, 4.10 GB (310 s) | 4.86 s, 1.98 GB (30.2 s) | 170 s, 7.72 GB (181 s) | 146 s, 8.21 GB (162 s) | not run |

Ranges: light uncached 12.40-12.87 s (200k) and 67.5-68.5 s (800k), cached
1.05-1.09 s and 4.85-5.17 s; shadow uncached 39.6-41.0 s and 170-173 s.
Shadow's first 800k build was an ordinary one this time (170 s, not 422 s).
Shadow is 6-10% faster than in the baseline, so the machine was a little
quieter; light's gains below are far larger than that.

| Size | light bytes / gzip -9 | shadow bytes / gzip -9 | light gzip vs shadow |
|---|---|---|---|
| 5k | 202,642 / 57,955 | 263,193 / 57,701 | +0.4% (+15%) |
| 200k | 3,456,905 / 860,671 | 4,067,637 / 1,002,909 | -14.2% (+26%) |
| 800k | 13,396,492 / 3,302,076 | 15,710,764 / 3,894,315 | -15.2% (+27%) |

Unused arities kept: 1 / 5, 49 / 204 and 202 / 779 (light) against 2 / 5,
51 / 204 and 215 / 779 (shadow). Both keep every unreachable `defmethod`
(406 at 200k, 1,627 at 800k) and remove every `:removable` marker.

The targets of this group, at `76c6366`:

| Target | Result | Met |
|---|---|---|
| uncached 800k / 200k wall at most 5x | 67.9 / 12.4 s = 5.5x (10.4x at `ac88cb2`; shadow 4.2x) | no |
| 800k uncached faster than shadow | 67.9 s against 170 s, 2.5x faster | yes |
| cached no-change 800k at most 10 s | 4.86 s, 1.98 GB | yes |
| 200k and 800k gzip within 5% of shadow | 14% and 15% smaller | yes |

Findings:

1. **Uncached builds are 2.4x faster at 200k and 4.6x at 800k**, and light
   now beats shadow at every size (3.3x at 200k, 2.5x at 800k). Growth from
   200k to 800k fell from 10.4x to 5.5x but is still above 4x and above
   shadow's 4.2x; top-level analysis is most of what remains
   ([below](#remaining-superlinear-growth-light-cljs-422)). Peak memory at
   800k fell from 6.37 to 4.10 GB, under shadow's 7.72 GB.
2. **A cached build with no change is 4.4x faster at 200k and 6.2x at 800k**
   (light-cljs-418 reuses the whole stored output), and grows 4.5x from 200k
   to 800k. It holds 1.98 GB at 800k, 48% of an uncached build.
3. **gzip went from 26-27% worse than shadow's to 14-15% better** at 200k
   and 800k (per-scope local names, light-cljs-419 and 424, and value
   position without wrappers, 420); raw output is 15% smaller too. At 5k
   gzip is level with shadow's.
4. **Unused arities are now on par**: light keeps fewer than shadow at every
   size (light-cljs-421).

`release bench --no-cache --stats` at `76c6366`, seed 42, 3 rounds of 100k,
200k and 400k (one run at 800k), load average 1.8-2.5, medians; the
baseline growth column is the group's table at `ac88cb2`:

| Counter | 100k | 400k | growth | `ac88cb2` growth |
|---|---|---|---|---|
| time.wall-ms | 5,711 | 27,410 | 4.8x | 7.3x |
| time.analysis-ms | 1,141 | 7,708 | 6.8x | 10.6x |
| trace.top-level-ms | 1,029 | 6,889 | 6.7x | 10.6x |
| trace.append-ms | 34 | 277 | 8.3x | 14.3x |
| trace.top-level.engine-ms | 387 | 2,632 | 6.8x | 9.6x |
| trace.units.host-ms | 3,191 | 13,416 | 4.2x (per unit 1.06x) | 7.1x (per unit 1.8x) |
| trace.drain.bookkeeping-ms | 237 | 1,030 | 4.3x | 7.4x |
| time.emission-ms | 387 | 1,569 | 4.1x | 6.7x |
| time.prepare-application-ms | 645 | 3,133 | 4.9x | 4.4x |
| memory.peak-rss-bytes | 0.69 GB | 2.16 GB | 3.1x | 3.8x |

Rounds differed by under 3% for most counters and by 7.5% at most
(`trace.top-level.engine-ms` at 400k).

## Re-measured at `81f9837` (October 23, 2026)

light-cljs master `81f9837` (after light-cljs-426 to 429, 432 to 437, 439,
441, 442 and 459) against shadow-cljs 3.5.3 with `-Xmx8g`, back to back in
one session on the machine of [Setup](#setup) with nothing else running.
Each round ran `compare.bb DIR --light WRAPPER --runs 1` once with shadow
and today's light-cljs default, then `--light-only` for each other
configuration; three rounds, medians (range). WRAPPER is a shell script that
appends the configuration's flags to an immutable copy of the release binary
(`release bench --no-cache --jobs 4 ...`; the command line overrides the
project's `:macro-memory-limit` of 2 GiB) and records user CPU and the load
average from an inner `/usr/bin/time -l`; a `clojure` wrapper on `PATH` does
the same for shadow. Every output printed the oracle's checksum
(`compare.bb` stops otherwise) and passed `check-waste.bb --strict`
(`:waste-ok` in each round's `compare-result.edn`).

Configurations: *today* is today's default, `--jobs 1 --macro-memory-limit
67108864`; *proposed* is light-cljs-434's proposed default, `--jobs 4
--macro-memory-limit 1073741824`; *8 jobs* is `--jobs 8
--macro-memory-limit 2147483648`. Uncached builds, wall (range), user CPU,
peak RSS:

| Size | shadow | light today | light proposed | light 8 jobs |
| --- | --- | --- | --- | --- |
| 200k | 40.19 s (38.83-41.82), 122.5 s, 3.36 GB | 7.17 s (7.04-8.28), 7.4 s, 1.05 GB | 5.03 s (5.00-5.06), 9.6 s, 1.34 GB | 5.18 s (5.00-5.23), 13.2 s, 1.58 GB |
| 800k | 179.15 s (166.14-203.05), 414.1 s, 7.75 GB | fails 2 of 2 | 27.20 s (26.59-28.62), 49.9 s, 4.43 GB | 27.57 s (27.05-27.97), 64.4 s, 4.75 GB |

Against shadow, uncached: 5.6x faster at 31% of its peak RSS (200k, today),
8.0x at 40% (200k, proposed) and 6.6x at 57% (800k, proposed). The 800k
build fails at today's default with "JavaScript session memory limit
exhausted" after about 12 s, in a different unit from run to run
(`workload.namir/gourujoun`, `workload.brombro/kumdarstul`), as
light-cljs-434 found. 1-minute load average: 2.2-3.3 for shadow and today's
light at 200k, 4.4-6.9 for the light builds that follow a shadow run (the
average lags shadow's JVM), 2.7-4.0 at 800k.

Cached, no change (one warm-up, then the timed rebuild): shadow 32.39 s
(32.37-33.65) and 3.43 GB at 200k, 146.72 s (145.09-150.18) and 7.96 GB at
800k; light 1.02 s and 0.53 GB (today), 1.04 s and 0.58 GB (proposed),
1.02 s and 0.57 GB (8 jobs) at 200k, 4.82 s and 2.01 GB (proposed) and 4.69
s and 2.02 GB (8 jobs) at 800k. Output is unchanged from `76c6366` at every
configuration: 3,456,905 bytes and 860,671 gzip at 200k, 13,396,492 and
3,302,076 at 800k, against shadow's 4,067,637 / 1,002,909 and 15,710,764 /
3,894,315 (-14.2% and -15.2% gzip).

QuickJS-only (`--no-default-features --target-dir target/quickjs`) at the
proposed default, 3 rounds: 200k 6.36 s (6.28-6.52), 12.9 s user, 1.16 GB,
the same output; Penpot in benchmarks/penpot-investigation.md.

*Before and after.* The parent of light-cljs-426 (`dd1c1be`) as a second
immutable binary, alternated with `81f9837` in the same rounds, `release
bench --no-cache --stats` on the 200k project:

| Build | `dd1c1be` | `81f9837` | change |
| --- | --- | --- | --- |
| `--jobs 1`, 64 MiB | 12.69 s (12.59-12.72), 14.9 s user, 1.11 GB | 7.02 s (6.88-7.05), 7.4 s user, 1.03 GB | -44.7% |
| `--jobs 8`, 2 GiB | 8.80 s (8.75-8.82), 29.7 s user, 1.78 GB | 5.05 s (4.98-5.06), 13.0 s user, 1.60 GB | -42.6% |
| `--jobs 8`, 64 MiB | fails 3 of 3 | 6.63 s (6.49-6.96), 9.2 s user, 1.10 GB | |

At one job V8 time (`trace.engine-ms`) fell from 4.58 to 0.91 s, engine
entries from 968,732 to 255,183, units entering the macro host from 7,345 to
2,493, top-level passes from 2.61 to 1.73 s and deferred units from 7.34 to
2.61 s. At eight jobs the pool takes all 13,477 units (8,222 at `dd1c1be`),
9,938 of them with no engine (3,924), the workers spend 2.93 s summed
(14.87 s) and the coordinator's V8 time is 0.51 s, all of it top-level
passes. At 64 MiB `dd1c1be` fails in `workload.nansimraim` and `81f9837`
runs the 9,938 engine-free units on seven engine-less workers with no worker
isolate. Output is byte-identical between the commits.

With the macro probe of light-cljs-426 (temporary, `--jobs 1`, one run): 0
core macro calls reach the engine (25,466 at `dd1c1be`, 5.76 s), and the
5,226 calls to source macros take 1.18 s, 226 us per call (2.82 s, 540 us).

*A cache-writing cold build is much slower than an uncached one.* The
untimed warm-up of the cached scenario, which writes the cache into an empty
directory, took 9.7 s at 200k (today and proposed) and 10.8 s with 8 jobs,
and 81.7 s (80.9-87.6, 229 s user) at 800k proposed and 102.5 s (102.4-104.7,
421 s user, 5.83 GB) with 8 jobs, against 27.2 and 27.6 s uncached. A
`--stats` pair at 200k proposed shows every unit entering the macro host
when the build records (13,477 against 2,406) and the coordinator waiting
2.65 s at drain points. Filed as light-cljs-461.

## Follow-up findings

### Top-level analysis and append (light-cljs-415)

A `sample` profile of the 400k project (seed 42: 903 worker namespaces, 30,878
top-level forms, about 100k globals, 1,469 macros, 2,484 records and types)
found these per-form costs that grow with the program so far, now removed:

- Merging a unit (`program_fragment::append`) found each of its new globals
  by scanning every global, visited every namespace's reader context, and
  walked six keyed tables whole (protocols, record constructors, field
  bindings, type methods, macro functions, namespace entries) to skip their
  shared entries. It now uses the program's name index, takes only the owned
  reader context, and skips a table the unit did not write.
- A record constructor call (`(R. ...)`) searched every expression for R's
  declaration to check its argument count; record layouts are now indexed.
- A namespace's top-level pass found its own globals among all globals; a
  unit scanned the namespace list for its own namespace.
- Every core macro expansion walked its whole `&env`, including the map of
  all declared macros, to decide whether it can skip converting it; the sizes
  of shared environment collections are now kept.

Back to back with the parent commit (`6a42488`), `release bench --no-cache
--stats`, 3 alternating rounds per size, load average 3-6, medians:

| Counter | parent 100k | parent 400k | growth | 415 100k | 415 400k | growth |
|---|---|---|---|---|---|---|
| time.wall-ms | 10,740 | 79,453 | 7.7x | 9,163 | 52,755 | 5.7x |
| trace.top-level-ms | 1,991 | 21,151 | 11.1x | 1,775 | 18,875 | 10.6x |
| trace.append-ms | 126 | 1,811 | 14.4x | 44 | 342 | 7.8x |
| trace.top-level.engine-ms | 634 | 6,138 | 9.7x | 613 | 6,063 | 9.9x |
| trace.units.host-ms | 4,817 | 35,684 | 7.6x | 3,709 | 15,392 | 4.1x |
| trace.drain.bookkeeping-ms | 379 | 2,787 | 7.5x | 286 | 1,235 | 4.2x |
| peak RSS | 0.7 GB | 2.5 GB | | 0.7 GB | 2.5 GB | |

Most of the wall-time gain is the record constructor search, which units paid
(units host time at 400k fell from 35.7 s to 15.4 s). Output at 100k is
byte-identical to the parent's at `none` and advanced. The targets of at most
5x for top-level and append are missed: append is now 0.34 s at 400k; what is
left is probably the units that define types or protocols, which still walk
the keyed tables they copied (not profiled further).

The largest remaining top-level term is outside this pellet's files:
declaring a source macro emits and evaluates a namespace registration of
every namespace with every global's name (`emit_deferred_unit` in
`register_javascript_macro`, src/emitter.rs), O(macros x globals). In the
400k profile it was about 9-10 s of 20-25 s of `trace.top-level-ms`, and it
is what keeps top-level growth near 11x. It is light-cljs-423. Smaller
remaining terms: copying the shared protocol table on each write while
snapshots share it (about 0.2 s at 400k), and the macro-environment map
rebuilt and re-measured after each new macro.

### Deferred-unit host time (light-cljs-416)

light-cljs-416 asked that host time per deferred unit
(`trace.units.host-ms / trace.units`) grow at most 1.2x from 100k to 400k, and
`trace.drain.bookkeeping-ms` at most 5x. Both were met by light-cljs-415's
changes, so 416 changed no code. The record constructor search that 415
removed was the size-dependent per-unit cost: each `(R. ...)` in a unit
scanned every expression of the program so far.

Measured at `fc79659` (415), `release bench --no-cache --stats`, 3 rounds
alternating 100k and 400k, load average 4-6, medians:

| Counter | 100k | 400k | growth |
|---|---|---|---|
| trace.units | 6,769 | 26,918 | 4.0x |
| trace.units.host-ms | 3,522 | 15,000 | 4.3x |
| host ms per unit | 0.520 | 0.557 | 1.07x |
| trace.drain.bookkeeping-ms | 270 | 1,176 | 4.4x |
| trace.drain-ms | 128 | 438 | 3.4x |
| trace.append-ms | 38 | 315 | 8.3x |
| trace.index-ms | 5 | 21 | 4.5x |
| trace.release-ms | 98 | 402 | 4.1x |
| trace.top-level-ms | 1,637 | 17,865 | 10.9x |
| time.wall-ms | 8,535 | 50,261 | 5.9x |
| peak RSS | 0.65 GB | 2.36 GB | 3.6x |

The rounds ranged from 3,433 to 3,637 ms (100k) and from 14,395 to 15,593 ms
(400k) of units host time, so the 1.07x per unit is close to the noise
between rounds. Append is the only bookkeeping stage that still grows faster
than the unit count (8.3x). It is 0.3 s at 400k, and its likely cause is the
one 415 named: units that define types or protocols still walk the keyed
tables they copied. Most of the wall time beyond linear growth is now
top-level analysis (10.9x, light-cljs-423), not deferred units.

### Cached rebuild with no change (light-cljs-418)

A warm `light-cljs release bench --cache-dir DIR` with no edit installed every
namespace from its artifact and then prepared and emitted the whole
application again, which grew faster than the program. A `--stats` run at
`89e2b30` spent at 400k 3.6 s preparing and 4.7 s emitting of 10.9 s, and
emission grew 7.2x from 100k. Now a build whose every namespace has an
artifact stores its output under a key of those artifacts and its emission
options, and the next build with that key writes the stored files instead
(docs/compiler-cache.md, "As built (light-cljs-418), whole outputs").
Copying the shared base of the macro host's layered tables on every merge
also went: installs were 5x from 100k to 400k, now 3.9x.

Seed 42, warm cache, no change, 3 rounds alternating the parent and the
candidate binary, each with its own cache, load average 2.9-3.1, medians
(peak RSS from `/usr/bin/time -l`):

| Commit | 100k | 400k | growth | 400k peak RSS |
|---|---|---|---|---|
| parent `89e2b30` | 2,104 ms | 11,421 ms | 5.4x | 2.01 GB |
| parent `53cdb94` (rebased) | 1,707 ms | 7,475 ms | 4.4x | 2.01 GB |
| light-cljs-418 | 575 ms | 2,488 ms | 4.3x | 1.05 GB |

The uncached 400k build peaks at 2.41 GB, so the warm one holds 44% of it.
What remains at 400k (100k to 400k growth): validating 0.86 s (4.3x), most of
it the SHA-256 of 199 MB of artifacts; decoding 0.90 s (4.1x); installing
0.20 s (3.9x); reading and parsing sources 0.13 s (4.0x); reusing the output
25 ms. The peak memory is the installed program.

A one-line edit to one leaf worker namespace (a `defn` appended to
`workload.baklestei` at 100k and `workload.babomhol` at 400k, required only
by a `workload.gN` namespace), the same edit on both commits and a different
one per round, then a warm build, 2 rounds alternating the parent `53cdb94`
and light-cljs-418, load average 3.1-3.8:

| Size | parent | light-cljs-418 | uncached | misses |
|---|---|---|---|---|
| 100k | 6.59, 6.63 s, 0.64 GB | 6.67, 6.70 s, 0.63 GB | 8.0 s | 130 of 288 |
| 400k | 39.2, 39.1 s, 1.92 GB | 39.5, 39.2 s, 2.02 GB | 45.0 s | 587 of 1,134 |

The edited build is 0.3-1% slower here, the cost of storing its output.

Every edited build's output equalled an uncached build of the edited project
by the same binary. Output reuse does not apply to them: half the namespaces
miss because every namespace that runs a source macro depends on every
earlier one (the coarse fallback, light-cljs-347), so the edit rebuild costs
most of an uncached one.

Penpot `app.main --optimize none`, cold (`--no-cache`), 3 rounds alternating
`53cdb94` and light-cljs-418: 21.73 s and 0.912 GB against 21.51 s and
0.901 GB (medians), within the +3% wall and +5% RSS gate.

### Macro registration (light-cljs-423)

Declaring a source macro, and every compile-time unit an engine evaluated,
first registered every namespace of its program with the engine: every
reader context and every global's name, emitted (`Emitter::registration`
walked all globals) and then evaluated, so 1,469 macros over about 100k
globals cost O(macros x globals) at 400k. Each compile-time engine now keeps
what it has registered (`src/reader_registration.rs`): a registration names
only the namespaces whose reader context changed since, fully, and of the
others only the definitions added since; a program that adds nothing
registers nothing. A macro's `MacroCode` keeps the tables its registration
reads instead of the emitted text of every namespace, which is most of the
400 MB drop in peak memory. A unit compiled while a macro runs registers the
namespaces it names in full, and the next registration names them again,
since the macro's reader view restores the earlier context when it returns.

`release bench --no-cache --stats`, seed 42, 3 rounds alternating the parent
`0b40519` and light-cljs-423 at 100k and 400k, load average 2.1-12.8 (another
worker was building), medians:

| Counter | parent 100k | parent 400k | growth | 423 100k | 423 400k | growth |
|---|---|---|---|---|---|---|
| time.wall-ms | 8,674 | 49,476 | 5.7x | 8,114 | 38,782 | 4.8x |
| time.analysis-ms | 1,867 | 19,701 | 10.6x | 1,328 | 8,950 | 6.7x |
| trace.top-level-ms | 1,733 | 18,703 | 10.8x | 1,196 | 7,973 | 6.7x |
| trace.top-level.engine-ms | 613 | 6,099 | 9.9x | 457 | 3,030 | 6.6x |
| peak RSS | 0.66 GB | 2.42 GB | | 0.65 GB | 2.03 GB | |

At 400k, 1,582 registrations named 2,705 namespaces, none restarting. In a
`sample` profile of the 400k build, registration
(`reader_registration::register`) is 1.5% of the top-level samples, and the
whole macro declaration (`register_javascript_macro`, which also analyzes and
evaluates the macro) 3.8%, against about 9-10 s of 20-25 s before. Output at
100k is byte-identical to the parent's at `none` and advanced, and
`benchmarks/generated-project/test.bb` passes.

The 5x target for top-level growth is missed (6.7x). What grows faster than
the program in the 400k top-level profile is outside registration: macro
expansion is 62% of the top-level samples (55% at 100k), most of it importing
the `&env` of each call into the engine, whose engine time grows 6.6x for 4x
calls, with more V8 garbage collection of a larger heap; and copying the own
layer of the macro host's layered tables on a definition while frames share
it (`register_runtime_var`, 5.1% at 400k, 3.2% at 100k), with copying the
shared protocol table on write (3.6% at 400k, 1.5% at 100k).

Penpot `app.main --optimize none`, cold, 3 rounds alternating `0b40519` and
light-cljs-423: 20.87 s and 0.893 GB against 19.83 s and 0.865 GB (medians),
within the +3% wall and +5% RSS gate; the output is the same size.

Rebased onto `26451ea` (light-cljs-420) and measured again the same way
against it, load average 2.6-4.4:

| Counter | parent 100k | parent 400k | growth | 423 100k | 423 400k | growth |
|---|---|---|---|---|---|---|
| time.wall-ms | 18,157 | 259,324 | 14.3x | 17,423 | 265,416 | 15.2x |
| trace.top-level-ms | 2,217 | 26,600 | 12.0x | 1,627 | 15,760 | 9.7x |
| trace.top-level.engine-ms | 670 | 6,238 | 9.3x | 473 | 3,167 | 6.7x |
| peak RSS | 0.65 GB | 2.37 GB | | 0.63 GB | 1.90 GB | |

Both binaries are slower here for a reason outside this pellet:
`Emitter::stable_local` (light-cljs-420) scans every expression of the
program on the first use in each emitter, and a compile-time unit's program
holds every expression before it, so emitting units is O(units x
expressions), light-cljs-425. In a `sample` of the 100k build it is 6.9 s of 11.2 s
(`emit_deferred_unit`), and it also falls in top-level, where each macro
declaration emits its unit. Penpot shows it too: `app.main --optimize none`
3 rounds alternating `26451ea` and light-cljs-423, 62.41 s and 0.881 GB
against 61.20 s and 0.867 GB (medians), within the gate. Output at 100k is
byte-identical to `26451ea`'s at `none` and advanced.

### Stable locals asked for only where statements are lowered (light-cljs-425)

Only `Emitter::lifts` gives `Emitter::head` a head, and it lifts only at
`Advanced` in a function's body, which only an application's emission is.
`head` now returns at once elsewhere, so a compile-time, interactive or
dev-build unit never asks `Emitter::stable_local`, and never scans its
program; an application's emission scans its own program once, as writing it
reads it. The answers are unchanged where they are asked, so output is too.

`release bench --no-cache --stats`, seed 42, advanced, 3 rounds alternating
`0b40519` (before light-cljs-420) and light-cljs-425 on `1311360`, with
`1311360` itself in the 100k rotation and once at 400k, medians:

| Counter | `0b40519` 100k | 425 100k | `1311360` 100k | `0b40519` 400k | 425 400k | `1311360` 400k |
|---|---|---|---|---|---|---|
| time.wall-ms | 7,723 | 8,024 | 17,655 | 50,472 | 40,456 | 234,645 |
| time.deferred-units-ms | 3,664 | 3,575 | 13,122 | 17,546 | 17,895 | 206,031 |
| peak RSS | 0.64 GB | 0.62 GB | 0.63 GB | 2.50 GB | 2.02 GB | 1.91 GB |

Load average 2.7-3.6 at 100k and 6.3-14.3 at 400k (another worker was
testing). The 100k wall is 3.9% over `0b40519`'s with deferred units level;
in a further pair at load 3-5 it was 8,408 against 8,557 ms, the 0.4 s that
light-cljs-423 saves in top-level set against 0.12 s more emission at
advanced from light-cljs-420's lowering. Penpot `app.main --optimize none`,
cold, 3 rounds alternating `0b40519` and this change, load 2.6-3.4: 21.44 s
and 0.868 GB against 22.41 s and 0.899 GB (medians); `1311360` once, 51.53 s.
The Penpot output is byte-identical to both, and the 100k output to master's
(`1311360`, and after rebasing `2bd106f`) at `none` and advanced.

### Advanced preparation and emission (light-cljs-417)

At `cf3d93a` the 6.7x emission growth this pellet was filed for was gone
(4.4x, after light-cljs-418, 420 and 425), and preparation grew 4.7x. A
`sample` profile of the 400k build showed no one superlinear pass: every
preparation pass grew 4.6-5x for 3.96x expressions (2.57 million at 400k,
96 bytes each), and the hottest frame in each was `Arena::index`. Analysis
leaves the program's tables as trees of shared 64-entry leaves
(`src/arena.rs`), so each lookup by ID walked about 16 levels at 400k,
two more than at 100k, with a cache miss at most of them. Two costs went:

- `prepare_owned_application` takes the program the CLI no longer needs and,
  at `Advanced`, flattens its tables into one run of entries each
  (`Program::unfreeze`) before any pass, moving the leaves rather than
  copying them. A lookup is then an array index, for preparation and
  emission alike. Below `Advanced` nothing changes.
- `ProgramFacts::new` (src/inference.rs) looked up every method of every type
  among the program's protocol markers by a linear scan, O(methods x
  protocols) (575 protocols at 100k, 2,324 at 400k); it is a hash set now.

`release bench --no-cache --stats`, seed 42, 3 rounds alternating the parent
`cf3d93a` and light-cljs-417, load average 1.8-2.3, medians (peak RSS from
`/usr/bin/time -l`):

| Counter | parent 100k | parent 400k | growth | 417 100k | 417 400k | growth |
|---|---|---|---|---|---|---|
| time.wall-ms | 7,240 | 34,466 | 4.8x | 5,799 | 27,666 | 4.8x |
| time.prepare-application-ms | 1,909 | 8,970 | 4.7x | 661 | 3,186 | 4.8x |
| time.emission-ms | 575 | 2,522 | 4.4x | 392 | 1,569 | 4.0x |
| peak RSS | 0.60 GB | 1.93 GB | | 0.69 GB | 2.10 GB | |

One run each at 800k: preparation 19.9 s and 7.7 s, emission 5.5 s and
3.3 s, wall 83.8 s and 67.9 s, peak RSS 3.66 GB and 4.13 GB. Advanced output
at 100k and 400k is byte-identical to the parent's, and
`benchmarks/generated-project/test.bb` passes.

The peak resident size rises by about the expression table (62 MB at 100k,
246 MB at 400k): the freed leaves stay resident in mimalloc's size-class
pages after the move, while the peak footprint macOS reports does not grow
(about 535 MB against 552 MB at 100k, single runs). That is why `none` and
`prepare` keep their tables as they are. Penpot `app.main --optimize none`, cold, 3 rounds
alternating `cf3d93a` and light-cljs-417, load 1.8-3.0: 19.08 s and
0.869 GB against 19.00 s and 0.865 GB (medians), byte-identical output.

The 4.5x target for preparation is missed (4.8x; emission, 4.0x, meets it).
What remains is no single pass. In the 400k profile against the 100k one,
folding grew 5.4x, removal (`retain`) 5.6x, inlining 5.1x, inference 5.6x
and call shapes 5.7x in samples, with the same three simplification rounds
at both sizes, while rapid type analysis grew 3.7x and emission's
`Emitter::emit` 4.0x and renaming (`terse::rewritten`) 3.3x. Probably each
pass that walks expressions in tree order rather than ID order slows per
expression as the table outgrows the caches (62 MB at 100k, 246 MB at
400k); it holds from 400k to 800k too (2.4x for 2x). The largest linear terms that would
shrink it: `ProgramFacts::new` runs again in the removal, folding and inlining
of every round (24% of preparation samples at 400k), and
`program_fragment::kind_children` clones each expression's kind to list
its children (about 14%).

### Remaining superlinear growth (light-cljs-422)

The 5x target for uncached 800k / 200k wall is missed at `76c6366` (5.5x).
The same `--stats` runs as in the re-measurement above, 200k (median of 3)
against 800k (one run):

| Counter | 200k | 800k | growth |
|---|---|---|---|
| time.wall-ms | 12,275 | 67,149 | 5.5x |
| time.analysis-ms | 2,804 | 25,037 | 8.9x |
| trace.top-level-ms | 2,511 | 22,399 | 8.9x |
| trace.top-level.engine-ms | 918 | 9,106 | 9.9x |
| trace.append-ms | 95 | 895 | 9.4x |
| time.deferred-units-ms | 7,212 | 31,693 | 4.4x |
| trace.units.host-ms | 6,600 | 28,249 | 4.3x |
| time.prepare-application-ms | 1,437 | 6,894 | 4.8x |
| time.emission-ms | 770 | 3,197 | 4.2x |
| peak RSS | 1.20 GB | 4.15 GB | 3.5x |

Macro calls (94,592 to 378,359), deferred units and namespaces all grow 4.0x.
At 4x growth throughout, the 800k build would take 49 s; of the 18 s more it
takes, top-level analysis is 12.4 s, deferred units 2.9 s, preparation
1.1 s. Meeting 5x (62 s at 800k) needs top-level growth under about 6.5x
with the rest as it is.

A `sample` profile of whole uncached builds at 100k and 400k, inclusive
samples over all threads (macro calls grow 4.0x):

| Frame | 100k | 400k | growth |
|---|---|---|---|
| `Analyzer::expand_macro` | 3,982 | 18,404 | 4.6x |
| `MacroEvaluator::import_environment` | 590 | 3,918 | 6.6x |
| `MacroEvaluator::import_macro_form` | 1,459 | 5,527 | 3.8x |
| `MacroEvaluator::macro_arguments` | 605 | 2,342 | 3.9x |
| `MacroHost::register_runtime_var` | 31 | 308 | 9.9x |
| V8 `Heap::CollectGarbage` | 314 | 2,380 | 7.6x |
| V8 `ConcurrentMarking` (background threads) | 102 | 1,936 | 19x |
| `prepare_owned_application` | 530 | 2,562 | 4.8x |

This is what light-cljs-423 named: importing each macro call's `&env` into
the engine grows faster than the calls, the V8 heap of the compile-time
engine grows and its garbage collection with it, and copying the macro
host's own layer on each definition (`register_runtime_var`) grows much
faster than the calls, though it is still small (about 0.3 s at 400k).
Preparation (4.8x) is the cache-size effect light-cljs-417 recorded. This
measurement pellet files no follow-up; each term needs its own look at the
macro environment import and the engine heap.

### Core macros expanded in Rust (light-cljs-426)

At `dd1c1be`, a probe around each branch of `Analyzer::expand_macro`
(`--jobs 1`, `--no-cache`) found 28,090 core macro calls expanded in the
compile-time engine on Penpot `app.main --optimize none` (6.27 s, 223 us per
call) and 25,466 on the 200k project (5.76 s), almost all of it the round
trip: building and importing `&env`, converting the call and its arguments,
and converting the result back. `and`, `or`, `when`, `when-not`, `if-not`,
`cond`, `condp`, `->`, `->>`, `cond->`, `cond->>`, `some->`, `some->>`,
`case`, `if-let`, `when-let`, `if-some`, `when-some`, `when-first`, `js-obj`,
`defmethod`, `defmulti`, `vswap!`, `for` and `doseq` now expand in Rust
(`src/native_core_macros.rs`); a macro that allocates names asks the engine's
`gensym` for all of them in one call. With the same probe afterwards, 300
core calls (0.06 s) still reach the engine on Penpot (`..`, `as->`,
`hash-map`, `doto`, three `js-obj` of more than eight entries) and none on the
200k project.

Parent (`dd1c1be`) and this change alternated, 3 rounds, load average 3.1 to
5.8, medians (`bb benchmarks/penpot.bb DIR main-none`; `release bench
--no-cache --stats` in the 200k project):

| Build | parent | 426 | change | parent peak RSS | 426 peak RSS |
|---|---|---|---|---|---|
| Penpot `app.main --optimize none` | 21.09 s | 15.09 s | -28.4% | 868 MB | 855 MB |
| 200k advanced | 14.07 s | 8.98 s | -36.2% | 1.17 GB | 1.07 GB |
| Penpot `app.main` advanced (one run each) | 22.75 s | 17.55 s | -22.9% | 1.11 GB | 1.11 GB |
| 100k `none` (one run each) | 5.84 s | 3.35 s | -42.6% | 544 MB | 466 MB |
| 100k advanced (one run each) | 6.60 s | 4.14 s | -37.3% | 708 MB | 641 MB |

On the 200k project `trace.units.host-ms` fell from 7,454 to 2,787 and
`time.analysis-ms` from 3,265 to 2,641; `macro-calls` (94,592) and the
per-namespace macro-call table are unchanged. Penpot `main-none` and
`main-advanced` and the 100k project at `none` and advanced are
byte-identical to the parent's, and `bb benchmarks/generated-project/test.bb
--light` passes. Most of what is left of macro time is user `defmacro`s
(20,126 calls, 10.7 s on Penpot with the probe), which light-cljs-427 takes
up.

### Source macros' fixed cost per call (light-cljs-427)

With the probe of light-cljs-426 extended to the parts of each call
(`--jobs 1`, `--no-cache`), user `defmacro`s on Penpot `app.main --optimize
none` at `cd4d0bb` took 10.4 s over 20,126 calls. For a trivial macro most of
it was not its own logic: building `withMacroReaderContext`'s reader view
(an alias map from every `:requires` entry and sets of every refer, exclude
and binding) cost about 70 us per call, and importing `&env` 20 to 270 us,
most of it the locals map and the reader context, which change at every new
binding and every new definition.

Three changes cut that. The compile-time runtime builds the reader view when
the reader first reads it (a hook of `compiler_runtime` in `build.rs`, so
output is unchanged). A source macro whose function reads `&env` only as
`(:k &env)` or `(get &env :k)`, or not at all, is given only those entries
and the `:ns` and `:light-cljs/reader-context` ones its reader wrapper needs
(`source_environment_keys` in `src/compiler_bootstrap.rs`), unless the cache
measures the expansion's conversions. The reader context reuses its
`:refers`, `:excludes` and `:bindings` vectors while they are unchanged, so
that a new definition converts only the declarations part, and an import
adds its environment children to the cache after encoding instead of
copying the cache each time.

Per call with the probe (parent `cd4d0bb`, then this change, one run each):

| Macro | calls | parent | 427 | reads `&env` |
|---|---|---|---|---|
| `stl/css` | 5,131 | 173.5 us | 129.8 us | passes it to a function |
| `app.main.style/css*` | 5,131 | 124.0 us | 49.9 us | no |
| `dm/str` | 885 | 318.9 us | 80.0 us | no |
| `dm/get-prop` | 844 | 264.4 us | 112.9 us | `(:ns &env)` |
| all source macros | 20,126 | 10.37 s | 8.06 s | |

Parent and this change alternated, 3 rounds, load average 4.6 to 7.0,
medians: Penpot `app.main --optimize none` 15.45 s and 856 MB before, 13.08 s
(-15.3%) and 850 MB after. One run each: Penpot `main-advanced` 18.05 s to
15.24 s, the seed-42 100k project 3.19 s to 2.80 s at `none` and 4.10 s to
3.63 s advanced. Penpot `main-none` and `main-advanced` and the 100k project
at `none` and advanced are byte-identical to the parent's, `bb
benchmarks/generated-project/test.bb --light` passes, and the `--stats`
counters are the same but `trace.reads` on Penpot (73,777 to 73,437, -0.5%):
converting a locals map is one of the map constructions that can demand a
user protocol extension and so touch its unit; a macro given fewer entries
demands it later, at the next construction that needs it.

`stl/css` misses the halving the pellet aimed at: it hands `&env` to
`caller-prefix`, so it receives the whole environment, and since the macros
around it no longer convert the locals map, its own conversions find the
locals fresh more often. Of its 130 us, importing `&env` takes 65 (40 in the
engine decoding the fresh locals and reader context), the macro's own work
37 and the call form 9. Following `&env` into a helper function needs a
proof that holds while the helper can be redefined, which this change does
not attempt.

After review, the narrowed environment is given only while the compilation
has registered no protocol extension bundle (converting the entries left out
constructs maps, whose hashing and equality can call a user extension at the
declaration frontier) and only for keyword invocations, not core `get`,
which `with-redefs` can replace. Penpot registers extensions early, so its
macros now keep the whole environment; the table above measured the ungated
version. With the gate, 2 alternating rounds: 14.85 s and 820 MB before,
13.43 s (-9.6%) and 803 MB after, output byte-identical. Narrowing the gate to
the extensions that can reach canonical environment data (default, object and
core collection, symbol and keyword selectors) is left for a follow-up.

### The narrowed environment's gate (light-cljs-433)

light-cljs-433 replaced the blanket gate with two narrower ones. The
compilation catalog counts, next to `bundle_count`, the registered bundles
that converting environment data could demand or call
(`reaches_environment_data` in `src/compilation_session.rs`): those with an
index whose protocol is a core one and whose selector is `null` (a computed
or instance target), `_`, a native kind or a `cljs.core` type. Extensions of
source protocols, of user `deftype`s and `defrecord`s, and multimethods do not
count. And each compile-time engine notes the first extension its code
installs outside the declarations, from a function a macro calls or a
`defonce` initializer, that environment data could reach
(`noteEnvironmentExtension`, a `compiler_runtime` hook in `build.rs`): the
review's gate did not cover that case either, and a new test shows the cached
and uncached outputs differ without it.

Neither gate opens on Penpot. Its port registers 12 counted bundles, the first
at `app/common/time.cljc:497` (`IEncodeJS` on a computed target), before
nearly every macro call. All 12 have `null` selectors, because their targets
are values rather than type declarations: transit's `IEquiv`, `IHash` and
`IComparable` on `Long`, `TransitUUID` and `TaggedValue`, which are `def`s of
goog constructors, its `IEquiv` on `UUID`, rumext's `INamed` on
`cljs.core.UUID`, promesa's `IPrintWithWriter` and `IDeref` on `js/Promise`,
and beicon's `IFn` and `IDeref` on RxJS classes. The runtime demands a
`null`-selector bundle for any value that the protocol is dispatched on, and
building maps dispatches `IEquiv` and `IHash`, so converting the locals map
can initialize transit's bundles, and initializing them runs user code (the
`Long` root). Of the 14,171 calls to source macros that read only keyword
entries, 37 get the narrowed environment (none did at the parent). A sound
gate that opens there needs three more proofs: which core protocols converting
environment data dispatches, which constructors a ready computed-target bundle
patched, and that the left-out entries hold no UUID literal. That is a larger
design than a gate, so it is not attempted here.

Per call with the probe (`--jobs 1`, `--no-cache`, one run each, load 4.8 to
5.2): `cd4d0bb` is light-cljs-427's parent, `ff491d9` this change's parent.

| Macro | calls | `cd4d0bb` | `ff491d9` | 433 |
|---|---|---|---|---|
| `stl/css` | 5,131 | 196.5 us | 124.2 us | 118.5 us |
| `app.main.style/css*` | 5,131 | 129.0 us | 63.5 us | 60.9 us |
| `dm/str` | 885 | 368.5 us | 268.3 us | 279.4 us |
| `dm/get-prop` | 844 | 285.7 us | 218.4 us | 222.3 us |
| all source macros | 20,126 | 11.29 s | 9.88 s | 9.56 s |

Penpot `app.main --optimize none`, the three alternating in 3 rounds, load 4.3
to 2.6, medians: 15.49 s and 859 MB at `cd4d0bb`, 13.63 s and 833 MB at
`ff491d9`, 14.04 s and 836 MB with this change (-9.4% from `cd4d0bb`; the
rounds against `ff491d9` were -1.3%, +3.0% and -0.1%). light-cljs-427's
targets are still missed on Penpot: -9.4% against -10%, and of the four
macros only `css*`, which the lazy reader view cut, is halved. Penpot
`main-none` and `main-advanced` and the seed-42 100k project at `none` and
advanced are byte-identical to `ff491d9`'s and `cd4d0bb`'s. The 54
non-timing `--stats` counters, `trace.reads` (73,777) among them, equal
`ff491d9`'s. The extension report reuses the `light.build` service, since a
service of its own adds an engine entry to every compilation.
`bb benchmarks/generated-project/test.bb --light` passes.

## Default V8 regression check (October 27, 2026)

The October 27 slowdown reproduces with the original uncached command, without
statistics or source maps: **106.03 / 23.39 s = 4.53x slower**. Three quiet
runs per revision; independent medians, wall range in brackets:

| Revision | `release bench --no-cache`, wall | Peak RSS, GB | Pre-run load (1/5/15 min), rounds 1; 2; 3 |
| --- | ---: | ---: | --- |
| `ac88cb2237732ffdf6588fb0418ce5c7f86fe956` | 23.39 [23.08–23.43] s | 1.180 | 2.34/2.25/2.24; 2.42/2.33/2.27; 2.67/2.32/2.27 |
| `045c592656afc9d623ff73530847c782f6905099` | 106.03 [104.35–107.28] s | 1.067 | 2.12/2.20/2.22; 2.29/2.31/2.26; 2.65/2.34/2.27 |

Both release compilers were freshly built in an isolated worker worktree with
Homebrew Rust/Cargo 1.99.1, default features, thin LTO and one codegen unit.
macOS 15.7.3, Apple M3 Ultra (Virtual), 24 logical CPUs, 32 GiB RAM;
Babashka 1.12.218, Node 26.12.1. Builds and timing processes were sequential,
with no peer builders. `/usr/bin/time -l` includes process startup and writing;
OS file caches were retained. GB means 1e9 bytes. The generator is unchanged
since its original commit (`5ed076d3`), seed 42, requested 200k lines, producing
199,129 nonblank lines and 450 workers. Generated files stayed under the
worker's ignored `target/`; both outputs printed the oracle checksum `484264`.

```sh
LIGHT_CLJS_V8_CACHE=require CARGO_BUILD_JOBS=3 cargo build --locked --release
bb benchmarks/generated-project/generate.bb target/v8-regression-543/generated-200k --seed 42 --lines 200k
# In the generated project, with each revision's absolute binary path:
/usr/bin/time -l "$BIN" release bench --no-cache
```

Historical builds used detached worktrees under the worker's `target/`, each
with its own Cargo target directory. An empty workspace with exclusions for
`vendor/v8`, `vendor/rquickjs-sys` and `playground/crate` was appended to each
historical manifest solely to isolate it from the enclosing modern workspace;
source, lockfile, features and release profile were unchanged.

The flag comparison also uses the single-file command from light-cljs-522,
with the same generated roots and macro budgets, independently toggling
`--stats` and `--source-map "$MAP" --source-map-include-sources`:

```sh
# PROJECT and MAP are fixed absolute paths under the worker's target/.
/usr/bin/time -l "$BIN" --compile --esm --entry workload.main \
  --source-root "$PROJECT/src" --source-root "$PROJECT/macros" \
  --optimize advanced --no-cache \
  --macro-expansions 10000000 --macro-nodes 1000000000 \
  --macro-timeout-ms 3600000 --macro-memory-limit 2147483648 \
  --macro-poll-budget 100000000000
```

Each sample starts a fresh process. The project's `.light-cljs`,
`.compare-cache` and `out/light` are removed before every run. Revisions
alternate within each condition, baseline first in rounds one and three,
current first in round two. Generated conditions precede Penpot in each round.

### Independent statistics and source-map comparison

Three runs per cell, independent medians; wall range in brackets, CPU time in
seconds and RSS in decimal GB. `maps` includes embedded source content. Each
load cell gives the three pre-run 1/5/15-minute load averages. Both revisions
use the identical single-file command above, with only the named flags added.

| Revision | Flags | Wall, s | User CPU, s | Peak RSS, GB | Pre-run loads, rounds 1; 2; 3 |
| --- | --- | ---: | ---: | ---: | --- |
| `ac88cb2` | none | 23.10 [22.99–23.42] | 24.03 | 1.188 | 2.42/2.25/2.23; 2.20/2.25/2.24; 2.52/2.45/2.33 |
| `045c5926` | none | 104.55 [103.75–106.44] | 104.63 | 1.081 | 1.97/2.16/2.20; 2.02/2.24/2.24; 2.22/2.38/2.31 |
| `ac88cb2` | stats | 23.40 [23.16–23.46] | 24.30 | 1.202 | 2.49/2.29/2.24; 2.52/2.32/2.26; 2.34/2.37/2.31 |
| `045c5926` | stats | 105.01 [104.33–106.06] | 105.09 | 1.101 | 2.55/2.32/2.25; 2.10/2.21/2.23; 1.94/2.28/2.28 |
| `ac88cb2` | maps | 23.41 [23.27–23.67] | 24.31 | 1.244 | 2.33/2.28/2.24; 2.58/2.39/2.29; 2.18/2.24/2.25 |
| `045c5926` | maps | 105.02 [104.53–106.58] | 105.07 | 1.167 | 2.61/2.34/2.27; 2.35/2.29/2.25; 2.22/2.24/2.25 |
| `ac88cb2` | stats, maps | 23.50 [23.38–23.91] | 24.45 | 1.254 | 2.54/2.37/2.28; 1.95/2.29/2.27; 2.25/2.27/2.26 |
| `045c5926` | stats, maps | 105.73 [105.00–106.13] | 105.78 | 1.176 | 2.09/2.27/2.25; 2.67/2.42/2.30; 2.19/2.24/2.25 |

The same-command slowdown is 4.49–4.53x across these conditions. Neither the
flag differences nor the release/single-file command difference explains it.
One exploratory release+stats pair (excluded from these three-run cells) took
23.45 / 106.54 s. All 50 original-matrix processes, including that pair and
[Penpot](penpot-investigation.md#default-v8-regression-check-october-27-2026),
succeeded; every generated output printed the independent oracle's `484264`.
Each repeated revision/condition produced byte-identical JavaScript. All
single-file outputs passed Node syntax checking. These checks establish
execution/output repeatability, not source-map correctness.

### Culprit and profiles

The scripted bisect identifies **`b49764765e7f07cf7749ceed87e7fc48e37e5969`**
("Preserve method publication under conservative core-write tracking") as the
first bad commit. Its parent is `c92e5a557cccc85b891bffb06b3f8a04f1b04f5a`.
The matrix revision `045c5926` has identical `src`, `Cargo.toml`, `Cargo.lock`,
`build.rs` and `vendor` inputs to `b4976476`.

```sh
git bisect start --first-parent 045c5926 ac88cb2 -- \
  src Cargo.toml Cargo.lock build.rs vendor/v8 vendor/rquickjs-sys \
  ':(exclude)src/bbp*'
```

An inline Babashka command supplied to `git bisect run` built each point with
`cargo build --locked --release`, default features, `LIGHT_CLJS_V8_CACHE=require`
and `CARGO_BUILD_JOBS=3`, then timed `release bench --no-cache` in the same
project. Exit 1 means wall time **over 50 s**, 0 means at most 50 s, and 125
means build/compile failure; no points were skipped. The baseline worktree's
target directory was reused, with `cargo clean --release -p light-cljs` and
`cargo clean --release -p big-boy-pants-js` (where present) between revisions.
Engine-only history and `src/bbp*` changes were excluded. One timing per bisect
point, with three-run endpoint measurements above and below:

| Point, in test order | Wall, s | Pre-run load, 1/5/15 min | Threshold result |
| --- | ---: | --- | --- |
| `26ca6516` | 6.95 | 2.40/2.27/2.26 | good |
| `05007788` | 6.97 | 3.00/2.45/2.33 | good |
| `c0a9601e` | 7.14 | 3.14/2.58/2.38 | good |
| `a7958cb9` | 6.94 | 2.79/2.59/2.40 | good |
| `070eda86` | 7.02 | 2.68/2.61/2.42 | good |
| `509110a6` | 6.93 | 2.61/2.59/2.43 | good |
| `c92e5a55` | 7.02 | 2.69/2.61/2.45 | good |
| `b4976476` | 104.97 | 2.98/2.70/2.49 | bad |

`b4976476` added an unconditional `native_multimethod_arguments` index in
`Emitter::new`. Each prepared deferred unit scanned its whole expression arena,
including shared earlier units, although only native emission consumes the
index. This repeats growing-program traversal on the default path. In the
first unprofiled release+stats run, 93.382 of 106.503 s was `trace.release-ms`;
macro-host execution was only 1.309 s.

Separate direct+stats builds of the original binary, without maps, were sampled
from launch through exit with `/usr/bin/sample PID 600 1 -mayDie -file FILE`.
Both compilers and samplers exited 0 and their JavaScript matched the unprofiled
outputs. These instrumented runs are excluded from all timing medians:

| Profile | Instrumented wall, s | Main-thread samples | Samples in merge-front → release IR | Pre-run → post-run load, 1/5/15 min |
| --- | ---: | ---: | ---: | --- |
| generated 200k | 117.595 | 85,310 | 73,990 (86.7%) | 2.02/2.31/2.29 → 2.46/2.34/2.30 |
| Penpot main | 80.036 | 57,747 | 46,774 (81.0%) | 2.46/2.34/2.30 → 2.40/2.32/2.29 |

The common inclusive path is `NamespaceAnalysis::merge_front` →
`SharedCatalog::release_application_ir` → `emit_deferred_unit` → `emit_js_mode`
→ `Emitter::new`, dominated by arena iteration/indexing. Percentages describe
that main-thread branch, not whole-process CPU time; inclusive frames overlap.

### Repair and three-run verification

Fix `db78b7bbc0b9b2d7577ec907f70c5ed3806d8526` builds the multimethod argument
index inside the existing `native_forms` branch, preserving native indexing
while avoiding the unused scan during V8/QuickJS and application emission.
The same default release build and commands were rerun after all bisect builds
finished. Three runs per cell, independent medians, wall range in brackets:

| Revision | Command/flags | Wall, s | User CPU, s | Peak RSS, GB | Pre-run loads (1/5/15 min), rounds 1; 2; 3 |
| --- | --- | ---: | ---: | ---: | --- |
| parent `c92e5a55` | release | 6.94 [6.91–6.98] | 7.10 | 1.090 | 2.23/2.43/2.42; 1.94/2.35/2.39; 1.88/2.32/2.37 |
| fixed `db78b7bb` | release | 6.87 [6.85–6.88] | 7.03 | 1.087 | 2.41/2.32/2.36; 2.17/2.25/2.32; 2.25/2.23/2.31 |
| fixed `db78b7bb` | direct, none | 6.83 [6.82–6.87] | 7.02 | 1.102 | 2.36/2.31/2.35; 2.23/2.26/2.33; 2.15/2.21/2.30 |
| fixed `db78b7bb` | direct, stats | 6.86 [6.84–6.87] | 7.04 | 1.081 | 2.41/2.32/2.36; 2.04/2.22/2.31; 2.13/2.20/2.30 |
| fixed `db78b7bb` | direct, maps | 7.04 [7.03–7.09] | 7.24 | 1.176 | 2.27/2.30/2.35; 1.96/2.20/2.31; 2.12/2.20/2.29 |
| fixed `db78b7bb` | direct, stats, maps | 7.03 [7.00–7.05] | 7.20 | 1.173 | 2.25/2.29/2.34; 1.97/2.19/2.30; 2.03/2.18/2.28 |

The original release command falls from **106.03 to 6.87 s**, near the culprit's
parent (6.94 s), and below the remeasured October baseline (23.39 s). The matched
direct+stats runs' median `trace.release-ms` falls from 92.022 to **0.167 s**.
These phase times are included in wall time and must not be added to it.

All 27 repaired builds across both workloads and all six parent builds succeed.
Each JavaScript output is byte-identical to the corresponding original
`045c5926` output; every generated application prints `484264`, and all
single-file outputs pass Node syntax checks. This isolates the measured
slowdown to unused compiler bookkeeping. The scope is these two uncached
workloads at the default one job; native-backend and Shadow results remain
their separate dated measurements.

Verification of the repaired source at `db78b7bb`, after timing finished:

- `cargo fmt --check` passes.
- `cargo nextest run --locked --no-fail-fast` passes **2,132 tests**, 109 skipped,
  with default features and `LIGHT_CLJS_V8_CACHE=require`.
- Strict compiler Clippy passes for default features, QuickJS-only
  (`--all-targets --no-default-features --features quickjs`), and engine-free
  library/CLI (`--no-default-features --lib --bin light-cljs`), all with
  `-- -D warnings`.
- `cargo test --locked --no-default-features --features bbp-macros,quickjs
  --test bbp_macros multimethod -- --test-threads=3` passes all three tests,
  including conservative replacement, pool-size differential, capture rejection
  and once-only effects coverage.

The nested worker checkout exposed a Cargo workspace-discovery failure in the
Wasm playground test, also reproduced at the culprit's parent. The full suite
used a temporary empty `[workspace]` in the worktree-local playground manifest
and the installed Rust 1.99.0 Wasm toolchain's Cargo/rustc through
`LIGHT_CLJS_CARGO`, with the normal allowed Cargo cache. That temporary manifest
change was restored; native verification uses Homebrew Rust/Cargo 1.99.1.
Integration checks used the pinned read-only ClojureScript checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` through a temporary local symlink.

Two baseline limitations remain explicit. Engine-free `--all-targets` includes
ungated engine-dependent tests: the compiler target reports the same 501
QuickJS/API errors at the parent and repaired source; engine-free core lint
passes. Native+QuickJS all-target strict Clippy stops on eight unchanged engine
diagnostics, reproduced at untouched `045c5926` (deprecated `fetch_update`,
`chunks_exact_to_as_chunks`, `byte_char_slices`). The same native Clippy command
passes with only those three categories allowed after `-D warnings`; no source
suppressions were added. The engine's full test suite was outside this compiler
emission change.

```sh
cargo clippy --locked --all-targets --no-default-features \
  --features bbp-macros,quickjs -- -D warnings -A deprecated \
  -A clippy::chunks_exact_to_as_chunks -A clippy::byte_char_slices
```

## Re-measured at `0ad2514f`, cached and uncached (October 29, 2026)

**Summary.** At its default (`--jobs 1`, the project's 2 GiB macro budget)
light-cljs builds the 200k project uncached in 0.17x shadow-cljs's wall time
(5.8x faster) at 0.27x its peak memory, and the 800k project in 0.21x the time
(4.8x faster) at 0.54x the peak memory. With each compiler's cache populated
and nothing changed, light takes 0.03x shadow's time at both sizes (1.0 s
against 32 s, 4.8 s against 147 s) at 0.16x and 0.26x its peak memory; after
one leaf edit 0.16x (200k) and 0.18x (800k). Light's output is 0.86x
(200k) and 0.85x (800k) shadow's size after gzip. Since the October 23
section: every output is byte-for-byte the same size and gzip as since
`76c6366`; `--jobs 4` and `--jobs 8` are within 2% of October 23 in wall
time; the one-job default now builds 800k (36.7 s, 4.01 GB; on October 23 it
failed at the 64 MiB budget it was given, and at `76c6366` it took 67.9 s);
shadow measured within 2%. New and worse: a leaf edit at `--jobs 4` or `8` is
slower than at one job (37.1 and 44.5 s against 27.0 s at 800k), and the
native macro backend builds both projects but 4.9x (200k) and 12.3x (800k)
slower than the default backend at one job, with no gain from its cache.

Setup. light-cljs master `0ad2514f`, the same immutable release binaries as
the [Penpot section of this date](penpot-investigation.md#re-measured-against-shadow-cljs-cached-and-uncached-october-29-2026-at-0ad2514f)
(default features, and native-only `--no-default-features --features
bbp-macros`), shadow-cljs 3.5.3 with the generated `-Xmx8g`, Apple M3 Ultra
(Virtual), 24 logical CPUs, 32 GiB, macOS 15.7.3, OpenJDK 25.0.2, Clojure CLI
1.12.4.1618, Babashka 1.12.218, Node 24.22.1, Rust/Cargo 1.99.1. The machine
was reserved, one job at a time. Projects from `generate.bb --seed 42 --lines
200k|800k` (199,129 and 795,693 nonblank lines; 450 and 1,808 worker
namespaces), under the worktree's ignored `target/`.

```sh
bb benchmarks/generated-project/compare.bb DIR --light BIN --runs 3 --warm-jvm 4 --leaf src/workload/babomhol.cljs   # 200k
bb benchmarks/generated-project/compare.bb DIR --light BIN --runs 1 --leaf src/workload/babra.cljs                    # 800k
# other light configurations: the same with --light-only and BIN a wrapper
# script that appends --jobs 4 --macro-memory-limit 1073741824 (or --jobs 8
# --macro-memory-limit 2147483648), or the native-only binary
```

`compare.bb` gained two minimal additions for this run: it records user CPU
and the load average before every timed build, and `--leaf FILE` adds a
leaf-edit scenario for each compiler: on the cache its cached scenario left,
each run appends a different unused `defn` to `FILE` and rebuilds (FILE is
restored afterwards). `workload.babomhol` (200k) and `workload.babra` (800k)
are each required by one `workload.gN` namespace only. Uncached is light
`--no-cache` and shadow with `.shadow-cljs` removed; cached is one untimed
warm-up build into an empty cache, then the timed no-change rebuilds; shadow's
cached builds still run Closure. Medians of 3 runs (200k, range in brackets)
and 1 run (800k). The 1-minute load average before a timed build was 1.7-4.6
for light and 2.2-7.1 for shadow (the JVM raises it while it runs).

| Size | Build | light, default | shadow | light/shadow wall, peak memory |
| --- | --- | --- | --- | --- |
| 200k | uncached | 7.03 s (6.99-7.04), 7.1 s user, 1.071 GB | 40.70 s (38.79-40.84), 124.2 s user, 4.017 GB | 0.17, 0.27 |
| 200k | cached, no change | 1.02 s (1.02-1.10), 0.9 s user, 0.525 GB | 32.43 s (31.75-32.59), 100.6 s user, 3.318 GB | 0.03, 0.16 |
| 200k | cached, leaf edit | 5.14 s (5.13-5.34), 5.3 s user, 1.013 GB | 32.63 s (32.42-33.37), 103.1 s user, 3.128 GB | 0.16, 0.32 |
| 800k | uncached | 36.67 s, 36.9 s user, 4.013 GB | 176.78 s, 418.2 s user, 7.416 GB | 0.21, 0.54 |
| 800k | cached, no change | 4.76 s, 4.4 s user, 1.990 GB | 147.49 s, 312.9 s user, 7.591 GB | 0.03, 0.26 |
| 800k | cached, leaf edit | 27.00 s, 30.0 s user, 3.650 GB | 147.69 s, 302.9 s user, 8.032 GB | 0.18, 0.45 |

Shadow in one JVM at 200k (`--warm-jvm 4`, as a running shadow server):
30.66 s, then 23.94, 24.24 and 25.04 s; light's uncached build is 0.29x its
steady state. Uncached growth from 200k to 800k is 5.2x for light (5.5x at
`76c6366`) and 4.3x for shadow.

Output size, unchanged from `76c6366` for light and from the baseline for
shadow:

| Size | light bytes / gzip -9 | shadow bytes / gzip -9 | light/shadow raw, gzip |
| --- | --- | --- | --- |
| 200k | 3,456,905 / 860,671 | 4,067,637 / 1,002,909 | 0.85, 0.86 |
| 800k | 13,396,492 / 3,302,076 | 15,710,764 / 3,894,315 | 0.85, 0.85 |

Other light configurations, same scenarios (`--jobs N` with the October 23
budgets; native is the native-only binary at the project's budget):

| Size | Configuration | uncached | cached, no change | cached, leaf edit |
| --- | --- | --- | --- | --- |
| 200k | `--jobs 4`, 1 GiB | 5.07 s (4.99-5.09), 9.8 s user, 1.362 GB | 1.06 s, 0.568 GB | 5.54 s (5.54-5.66), 8.3 s user, 1.227 GB |
| 200k | `--jobs 8`, 2 GiB | 5.09 s (5.08-5.18), 13.2 s user, 1.600 GB | 1.07 s, 0.574 GB | 5.87 s (5.70-5.91), 12.3 s user, 1.598 GB |
| 200k | native, `--jobs 1` | 34.41 s (34.00-34.42), 32.6 s user, 1.291 GB | 35.76 s (35.62-35.79), 0.881 GB | 37.97 s (37.82-37.97), 35.6 s user, 1.299 GB |
| 200k | native, `--jobs 4` | 17.90 s (17.71-18.11), 24.0 s user, 1.463 GB | 20.59 s (20.49-22.90), 1.106 GB | 22.83 s (22.75-22.83), 25.9 s user, 1.511 GB |
| 800k | `--jobs 4`, 1 GiB | 27.68 s, 50.8 s user, 4.239 GB | 4.70 s, 1.996 GB | 37.14 s, 110.9 s user, 4.414 GB |
| 800k | `--jobs 8`, 2 GiB | 27.36 s, 65.7 s user, 4.832 GB | 4.68 s, 2.041 GB | 44.45 s, 190.0 s user, 5.423 GB |
| 800k | native, `--jobs 1` | 452.15 s, 412.5 s user, 4.799 GB | 477.49 s, 3.202 GB | 484.73 s, 438.5 s user, 5.038 GB |

Against shadow, uncached: 0.12x the wall time at 200k and 0.16x at 800k with
four or eight jobs. Four and eight jobs are equally fast at both sizes, so
these rows are not evidence of many-core scaling.

Correctness, coverage and performance, separately:

- *Correctness.* For both compilers, every configuration and both backends,
  the last uncached build's and the last leaf-edited build's output printed
  the oracle's `484264` for input 42 and 3 iterations (`compare.bb` stops
  otherwise) and passed `check-waste.bb --strict`; the no-change cached
  builds' outputs were not checked separately. Light's outputs have the same size and gzip size
  in every configuration; the last native 200k output (four jobs, leaf edit)
  has SHA-256 `1a75bc87b26f3121...`, the default backend's 200k hash recorded
  in docs/native-macros.md, so the native backend's output is byte-identical
  to the default's there. The leaf edit adds an unused function, so edited
  outputs equal the unedited ones.
- *Coverage.* Both projects build in every configuration, on both backends.
  On October 23 the one-job build failed at 800k only because that row was
  given a 64 MiB macro budget; at the project's own budget it builds.
- *Performance.* Light is faster than shadow, with less peak memory, in every
  row on the default backend. The native backend is not yet competitive: at
  one job it is 4.9x slower than the default backend at 200k (34.4 against
  7.0 s) and 12.3x slower at 800k (452.2 against 36.7 s); its cached rebuild with no change is no faster
  than an uncached build (35.8 against 34.4 s), whereas the default backend's
  takes 1.0 s. A `--stats` pair at 200k shows why: the warm build reports
  `cache.hits 0` and 569 misses, each "no artifact was published", although
  the populating build published 569 artifacts (221 MB); it analyzes the
  whole program again and only then reuses the stored output. Filed as
  light-cljs-571.

Against the October 23 section (`81f9837`) and, for the one-job default, the
October 27 repair (`db78b7bb`, `release bench --no-cache`):

| Build | before | October 29 | change |
| --- | --- | --- | --- |
| 200k light `--jobs 1`, uncached | 6.87 s, 1.087 GB (October 27) | 7.03 s, 1.071 GB | +2.3% wall |
| 200k light `--jobs 4`, uncached | 5.03 s, 1.34 GB | 5.07 s, 1.362 GB | +0.8% |
| 200k light `--jobs 8`, uncached | 5.18 s, 1.58 GB | 5.09 s, 1.600 GB | -1.7% |
| 200k light, cached, `--jobs 4` / `8` | 1.04 / 1.02 s | 1.06 / 1.07 s | +2% / +5% |
| 800k light `--jobs 1`, uncached | fails at 64 MiB; 67.9 s, 4.10 GB at `76c6366` | 36.67 s, 4.013 GB | -46% against `76c6366` |
| 800k light `--jobs 4`, uncached | 27.20 s, 4.43 GB | 27.68 s, 4.239 GB | +1.8% |
| 800k light `--jobs 8`, uncached | 27.57 s, 4.75 GB | 27.36 s, 4.832 GB | -0.8% |
| 800k light cached, `--jobs 4` / `8` | 4.82 / 4.69 s | 4.70 / 4.68 s | -2% / 0% |
| 200k shadow uncached / cached | 40.19 / 32.39 s | 40.70 / 32.43 s | +1.3% / +0.1% |
| 800k shadow uncached / cached | 179.15 / 146.72 s | 176.78 / 147.49 s | -1.3% / +0.5% |

Conclusions:

1. **No default-backend regression since October 23**: every light row is
   within about 2% of its earlier value (the 200k cached rebuild at eight jobs
   +5%, 0.05 s), and output is unchanged. The one-job 800k build's halving is
   relative to `76c6366` (October 22); it was not measured at its own budget
   on October 23.
2. **Cached rebuilds are where light leads most**: 0.03x shadow's time at both
   sizes, because light reuses its stored output and shadow runs Closure
   again. A leaf edit costs light 73% (200k) and 74% (800k) of an uncached
   build, because every namespace that runs a source macro depends on every
   earlier one (the coarse fallback noted under light-cljs-418).
3. **A leaf edit with several jobs is slower than with one** (800k: 37.1 s at
   four jobs and 44.5 s at eight against 27.0 s at one, with 3.7x and 6.3x the
   user CPU). This is the cache-writing cost recorded on October 23 and filed
   as light-cljs-461 (deferred): a build that records to the cache sends every
   unit through the macro host.
4. **The native backend builds both projects with output identical to the
   default backend's but is much slower**, and grows faster than the program
   (34.4 s to 452.2 s from 200k to 800k, 13.1x for 4x the code; one 800k
   sample, started at a 1-minute load average of 4.6). Four jobs halve its
   200k time (17.9 s). Its cache does not help yet (light-cljs-571), and
   Penpot's `app.main` does not build on it (light-cljs-563, in the Penpot
   section).

Pellets light-cljs-563, 564 and 565 were still open when this ran; the user
chose to measure master `0ad2514f` now rather than after them.

## Pants build cache reuse (October 29, 2026, light-cljs-571)

The October 29 comparison above found that the Pants build's rebuild with
nothing changed hit no namespace on the 200k project: all 569 missed with "no
artifact was published", ran all 94,592 macro calls again and republished
221 MB. The cause was not the store. Before analysis, the Pants build scans
every source for spelled writes to core functions, and skips every cache
lookup once the program may write one, because cached macro code may call a
core function directly. The scan read `set!`, `binding` and `with-redefs` as
calls only at the head of a list, so `#(binding [...] ...)`, which three
namespaces of the 200k project contain (none of the 5k or 50k ones), counted
as a mutation operator used as a value, which marks every core function
written. It now reads an anonymous function literal like a list; a build
that does skip lookups now reports each namespace as a `core-write` miss
and publishes nothing.

Setup: the commit that fixed it on `b1ed620d`, release binaries of the V8
build and the Pants build (`--no-default-features --features bbp-macros`),
same machine as above but not reserved: peer workers kept the 1-minute load
average at 11-17 (up to 30 during one Pants clean build) on 24 logical CPUs.
`compare.bb DIR --light BIN --runs 3 --light-only --leaf
src/workload/babomhol.cljs`; 4 threads through a wrapper adding `--jobs 4`
(and `--macro-memory-limit 1073741824` for the V8 build). Medians of 3, wall
time, peak memory.

| Build, threads | clean build | rebuild, nothing changed | rebuild after a one-file edit |
| --- | --- | --- | --- |
| V8, 1 | 8.49 s, 1.09 GB | 1.19 s, 0.53 GB | 6.23 s, 1.02 GB |
| V8, 4 | 5.83 s, 1.38 GB | 1.15 s, 0.57 GB | 6.24 s, 1.28 GB |
| Pants, 1 | 43.32 s (39.15-58.08), 1.21 GB | 1.49 s, 0.57 GB | 10.72 s, 1.11 GB |
| Pants, 4 | 19.61 s, 1.46 GB | 1.44 s, 0.57 GB | 10.20 s, 1.32 GB |

Before the fix the Pants build's rebuild with nothing changed took 35.8 s at
1 thread and 20.6 s at 4 (the October 29 table); it now takes 1.49 and 1.44 s
with 569 of 569 hits, and the rebuild after a one-file edit falls from 38.0
and 22.8 s to 10.7 and 10.2 s. That edit hits 463 and misses 106 namespaces
(the edited one and 105 whose coarse view covers it) with 20,020 macro
calls, exactly as the V8 build does. Every output, clean, rebuilt and
edited, on both builds, has SHA-256 `1a75bc87b26f3121...` and prints the
oracle's `484264`.

The V8 build is unchanged: alternating master and the fix three times under
the same load, clean builds took 8.40, 8.03, 8.16 s against 8.05, 8.14,
8.58 s, and rebuilds with nothing changed 1.20-1.31 s against 1.17 s, all with
569 of 569 hits and the same output. The 8.1-8.5 s against the 7.0 s of the
reserved October 29 run is the load.

## Five-round baseline at `627b9ed5`, unreserved machine (November 13, 2026)

**Summary.** The baseline the shadow-cljs comparison pellets cite (group
shadow-comparison-credibility): five alternating rounds per build, with user
and system CPU time. The machine was **not reserved**: two other workers were
building and timing re-frame and generated projects, shadow-cljs builds
included, and the 1-minute load average before a timed build ranged from 3.6
to 26.8 at 200k and from 2.8 to 17.3 at 800k, on 24 CPUs. The pellet asked for
a reserved machine; the user's decision of November 13 ("please dont stop
because of the machine not being free") supersedes that, so the load gate was
switched off (`--max-load 1000`), no round was repeated, and the load before
every build is in the tables. Medians: light-cljs takes 0.15x shadow-cljs's
wall time for a clean build at 200k and 0.17x at 800k, 0.030x and 0.037x for a
rebuild with nothing changed, and 0.13x and 0.17x for a rebuild after a
one-file edit, at 0.14x to 0.48x its peak memory and 0.009x to 0.089x its CPU
time (user + system). Output is 0.86x (200k) and 0.85x (800k) shadow's size
after gzip.

Setup. light-cljs master `627b9ed5` as the immutable release binary
`research/shadow-credibility/bin/light-cljs-627b9ed5` (V8 build), shadow-cljs
3.5.3 with the generated `-Xmx8g`. The projects (`generate.bb --seed 42
--lines 200k|800k`), raw EDN results and logs stay in the gitignored
`research/shadow-credibility/` (`projects/`, `baseline-627b9ed5/`). Each round
reverses the previous round's compiler order. A clean build is light
`--no-cache` and shadow with `.shadow-cljs` removed; the rebuild with nothing
changed follows one untimed build into an empty cache; the one-file edit
appends a different unused `defn` to a leaf namespace each round
(`workload.babomhol` at 200k, `workload.babra` at 800k). Times and peak memory
come from `/usr/bin/time -l`. 200k ran from 22:50 to 23:04 MST, 800k from
23:04 to 00:06.

```sh
R=research/shadow-credibility
bb benchmarks/generated-project/compare.bb $R/projects/workload-200k-seed42 --light $R/bin/light-cljs-627b9ed5 \
  --leaf src/workload/babomhol.cljs --warm-jvm 4 --max-load 1000 --out $R/baseline-627b9ed5/workload-200k
bb benchmarks/generated-project/compare.bb $R/projects/workload-800k-seed42 --light $R/bin/light-cljs-627b9ed5 \
  --leaf src/workload/babra.cljs --max-load 1000 --out $R/baseline-627b9ed5/workload-800k
```

Both runs record light-cljs `627b9ed5` (binary SHA-256 `11ea6bcc630b2cb5...`),
shadow-cljs 3.5.3, OpenJDK 25.0.2, Node v26.12.1, Clojure CLI 1.12.4.1618 and
Babashka 1.13.229 on an Apple M3 Ultra (Virtual), 24 CPUs, 32 GiB, macOS
15.7.3. The tables are `compare.bb`'s Markdown output without its gate
sentence (the gate was off).

### 200k

| Build | Compiler | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- |
| clean build | light-cljs | 6.35 s (6.17-6.62) | 6.54 s (6.29-6.82) | 0.38 s (0.35-0.39) | 1.039 GB (1.031-1.049) | 3.61-8.80 |
| clean build | shadow-cljs | 41.54 s (41.32-44.22) | 126.58 s (125.66-128.77) | 129.17 s (112.30-140.77) | 4.164 GB (3.017-4.271) | 3.88-9.49 |
| rebuild, nothing changed | light-cljs | 1.26 s (1.10-1.43) | 1.06 s (1.01-1.13) | 0.12 s (0.08-0.15) | 0.542 GB (0.527-0.546) | 8.58-20.44 |
| rebuild, nothing changed | shadow-cljs | 41.67 s (37.33-65.11) | 103.25 s (98.20-107.79) | 26.44 s (15.29-52.51) | 3.821 GB (3.488-4.930) | 8.58-26.82 |
| rebuild after a one-file edit | light-cljs | 5.19 s (4.94-9.20) | 5.25 s (4.78-6.10) | 0.44 s (0.38-1.21) | 0.996 GB (0.988-1.034) | 11.39-15.02 |
| rebuild after a one-file edit | shadow-cljs | 38.85 s (34.83-43.77) | 105.27 s (102.63-107.30) | 21.04 s (14.08-29.93) | 3.171 GB (2.797-3.620) | 9.11-16.85 |

| Build | light/shadow wall time | light/shadow CPU (user + system) | light/shadow peak memory |
| --- | --- | --- | --- |
| clean build | 0.15 | 0.027 | 0.25 |
| rebuild, nothing changed | 0.030 | 0.009 | 0.14 |
| rebuild after a one-file edit | 0.13 | 0.045 | 0.31 |

| Output | Compiler | Bytes | gzip -9 bytes | Checked |
| --- | --- | --- | --- | --- |
| clean build | light-cljs | 3,449,022 | 859,697 | oracle 484264, waste ok |
| clean build | shadow-cljs | 4,067,637 | 1,002,909 | oracle 484264, waste ok |
| rebuild after a one-file edit | light-cljs | 3,449,022 | 859,697 | oracle 484264, waste ok |
| rebuild after a one-file edit | shadow-cljs | 4,067,637 | 1,002,909 | oracle 484264, waste ok |

shadow-cljs in one JVM (4 release builds, as a running server): 44.20 s (113.5 s CPU, load 15.44), 28.06 s (52.8 s CPU, load 20.69), 32.58 s (45.0 s CPU, load 17.85), 29.22 s (43.6 s CPU, load 19.14); the JVM's peak memory 5.253 GB.

### 800k

| Build | Compiler | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- |
| clean build | light-cljs | 40.55 s (33.85-42.68) | 39.69 s (34.51-40.74) | 3.09 s (1.80-3.73) | 3.783 GB (3.755-3.792) | 3.43-15.79 |
| clean build | shadow-cljs | 243.48 s (199.44-344.61) | 465.20 s (447.87-484.36) | 582.78 s (284.15-605.37) | 7.813 GB (7.597-8.657) | 2.83-17.32 |
| rebuild, nothing changed | light-cljs | 6.45 s (5.22-7.49) | 4.98 s (4.70-5.23) | 0.56 s (0.37-0.82) | 1.985 GB (1.966-1.991) | 7.71-9.15 |
| rebuild, nothing changed | shadow-cljs | 172.28 s (163.35-191.85) | 333.59 s (323.41-354.13) | 36.06 s (32.64-40.23) | 7.948 GB (7.586-8.351) | 7.09-13.43 |
| rebuild after a one-file edit | light-cljs | 28.14 s (26.77-30.50) | 30.02 s (28.41-32.77) | 1.77 s (1.57-2.41) | 3.526 GB (3.454-3.561) | 4.43-17.29 |
| rebuild after a one-file edit | shadow-cljs | 165.38 s (154.69-198.21) | 328.13 s (319.31-344.85) | 28.20 s (24.54-71.45) | 7.501 GB (7.421-8.358) | 4.33-13.01 |

| Build | light/shadow wall time | light/shadow CPU (user + system) | light/shadow peak memory |
| --- | --- | --- | --- |
| clean build | 0.17 | 0.041 | 0.48 |
| rebuild, nothing changed | 0.037 | 0.015 | 0.25 |
| rebuild after a one-file edit | 0.17 | 0.089 | 0.47 |

| Output | Compiler | Bytes | gzip -9 bytes | Checked |
| --- | --- | --- | --- | --- |
| clean build | light-cljs | 13,367,898 | 3,299,312 | oracle 189716, waste ok |
| clean build | shadow-cljs | 15,710,764 | 3,894,315 | oracle 189716, waste ok |
| rebuild after a one-file edit | light-cljs | 13,367,898 | 3,299,312 | oracle 189716, waste ok |
| rebuild after a one-file edit | shadow-cljs | 15,710,764 | 3,894,315 | oracle 189716, waste ok |

Findings, recorded as found:

1. **shadow-cljs's 800k clean build varied most in these runs**, from 199.4
   to 344.6 s (median 243.5 s, against 176.8 s on the reserved machine on
   October 29); its rebuild with nothing changed took 172.3 s (147.5 s).
   light's 800k clean build ranged from 33.9 to 42.7 s; the two rounds that
   started at the lowest load (3.4 and 4.8) took 33.9 and 35.3 s, below
   October 29's 36.7 s, while its 800k rebuild with nothing changed took
   6.45 s (4.76 s). At 200k, against October 29: light's clean build 6.35 s
   (7.03 s), rebuild with nothing changed 1.26 s (1.02 s), after a one-file
   edit 5.19 s (5.14 s); shadow's 41.54 s (40.70 s), 41.67 s (32.43 s; its
   slowest round, 65.1 s, started at load 17.0) and 38.85 s (32.63 s). The
   load was not controlled, and Node and the light-cljs revision differ from
   October 29, so these runs do not isolate the effect of load on either
   compiler.
2. **shadow-cljs spends a large share of its clean builds in system CPU**:
   129 s system against 127 s user at 200k, and 583 s against 465 s at 800k
   (medians), but 21 to 36 s on its cached rebuilds. light's clean builds use
   0.4 s and 3.1 s. System CPU was not recorded before (October 29 has user
   CPU only), so whether this is the load or shadow's normal behaviour is
   open; the CPU ratios above include it.
3. **Growth from 200k to 800k under this load** (clean build medians): light
   6.4x (5.5x between its lowest-load rounds) and shadow 5.9x. These runs were
   not made to measure growth, and a loaded machine says little about it.
4. **Output.** For each compiler at both sizes, the last clean build's and
   the last one-file-edit build's output printed the project oracle's
   checksum (`484264` at 200k, `189716` at 800k; input 42, 3 iterations) and
   passed `check-waste.bb --strict`; outputs of earlier rounds, of the
   rebuilds with nothing changed and of the in-JVM builds were not checked
   separately. light's output is slightly smaller than on
   October 29 (3,449,022 against 3,456,905 bytes at 200k, 13,367,898 against
   13,396,492 at 800k); shadow's is the same size.
5. **A running shadow-cljs server** (`--warm-jvm 4`, 200k) took 28 to 33 s
   per release build after the first, at 5.25 GB peak memory; light's clean
   build (6.35 s) is 0.22x of that and its rebuild after a one-file edit
   (5.19 s) 0.18x.

Rerun on a reserved machine before quoting these figures as typical; until
then they are the dated baseline, labelled unreserved.

## shadow-cljs's maximum heap swept, unreserved machine (November 14, 2026)

**Summary.** The objection: shadow-cljs's peak memory in these comparisons
reflects the `-Xmx8g` the generated projects give its JVM, not what it needs.
So shadow's maximum heap was swept from 1 to 8 GiB over clean release builds,
three alternating rounds each, and light-cljs's peak memory set against
shadow's at its **smallest adequate heap**: the smallest heap at which no
build failed and the median wall time is within 10% of shadow's best. That
heap is 1.5 GiB for the generated 200k project and the re-frame-shaped 200k
app, and 3 GiB at 800k, where 1, 1.5 and 2 GiB all run out of memory. Against
it, light-cljs's peak memory is:

| Project | light-cljs peak memory | shadow-cljs's smallest adequate heap | its peak memory | light/shadow | light/shadow at `-Xmx8g` |
| --- | --- | --- | --- | --- | --- |
| generated 200k | 1.029 GB | 1.5 GiB (43.34 s, 1.02x its best) | 2.281 GB | **0.45** | 0.30 |
| re-frame-shaped 200k | 1.405 GB | 1.5 GiB (74.31 s, 1.01x) | 2.268 GB | **0.62** | 0.36 |
| generated 800k | 3.646 GB | 3 GiB (223.07 s, 1.05x) | 4.298 GB | **0.85** | 0.46 |

**light-cljs's memory advantage shrinks a lot once shadow is given only the
heap it needs.** Against `-Xmx8g` light used 0.30x to 0.46x shadow's peak
memory; against the smallest adequate heap it uses 0.45x to 0.85x. At 800k the
advantage nearly disappears: 3.65 GB against 4.30 GB. Any claim built on the
`-Xmx8g` ratios overstates it. Wall time is not affected: at its smallest
adequate heap shadow still takes about 6 to 8 times as long (light 0.16x, 0.12x and
0.17x of shadow's median there).

Setup. light-cljs master `6ea2540d` as the immutable release binary
`research/shadow-credibility/bin/light-cljs-6ea2540d` (V8 build, SHA-256
`8f1ada98afb9233c...`), shadow-cljs 3.5.3, OpenJDK 25.0.2 (G1 collector), Node
v26.12.1, Clojure CLI 1.12.4.1618, Babashka 1.13.229, Apple M3 Ultra
(Virtual), 24 CPUs, 32 GiB, macOS 15.7.3. Projects: `generate.bb --seed 42`
at `--lines 200k` and `800k`, and `--shape re-frame --lines 200k` (the re-frame-shaped app), in
`research/shadow-credibility/projects/`; raw EDN, Markdown and logs in
`research/shadow-credibility/heap-sweep-6ea2540d/` (both gitignored). 200k ran
00:48 to 01:03 MST, re-frame 01:03 to 01:30, 800k 01:30 to 02:13.

```sh
R=research/shadow-credibility
for p in workload-200k-seed42 re-frame-200k-seed42 workload-800k-seed42; do
  bb benchmarks/generated-project/compare.bb $R/projects/$p --light $R/bin/light-cljs-6ea2540d \
    --runs 3 --max-load 1000 --max-repeats 0 --shadow-heaps 1g,1536m,2g,3g,4g,8g \
    --light-memory-limits 67108864 --out $R/heap-sweep-6ea2540d/$p
done
```

Method (`compare.bb --shadow-heaps`, added for this run). Each round runs
light-cljs at its configured limit, light-cljs at `--macro-memory-limit
67108864`, then shadow at each heap, and the next round reverses that order.
So the shadow heaps run side by side, which suits comparing them with each
other, but light-cljs is not interleaved between them: its builds and a given
heap's ran minutes apart, under different load. Peak memory, the measure
compared here, hardly moves with load (light's ranged by at most 7% over its
three builds); the light/shadow wall-time ratios are rougher than that. Every build is a clean build (light `--no-cache`, shadow with `.shadow-cljs`
and its output removed). The heap reaches shadow's JVM as `clojure
-J-Xmx<size> -M:shadow release bench`: the Clojure CLI puts `-J` options after
the alias's `-Xmx8g`, and the last `-Xmx` wins (checked:
`Runtime.maxMemory` reported 1.5 GiB under `-J-Xmx1536m`), so the shared
projects were not edited. shadow also gets `-XX:+ExitOnOutOfMemoryError`, so a
heap that is too small ends the build instead of leaving it thrashing, and
`-Xlog:gc` to a file, from which "GC pauses" sums the stop-the-world pauses.
Every successful build's output was checked against the project oracle and
`check-waste.bb --strict`. Times and peak memory come from `/usr/bin/time -l`.

**The JVM default heap is the 8 GiB row.** On this 32 GiB machine the JVM's
default maximum heap is a quarter of physical memory, `MaxHeapSize =
8589934592` (`java -XX:+PrintFlagsFinal -version`, marked ergonomic), the same
as the projects' `-Xmx8g`, so it was not run twice.

**The machine was not reserved.** Two other workers were building and testing
(a TodoMVC check of the re-frame ports and a size breakdown of re-frame
output). Following the user's decision of November 13 ("please dont stop
because of the machine not being free"), the load gate was off (`--max-load
1000`, no repeats). The 1-minute load average before a build ranged from 5.8
to 24.8 at 200k, 6.6 to 26.0 for re-frame and 5.1 to 33.5 at 800k, on 24 CPUs;
the load before every build is in the tables and the EDN. Nothing was cut:
every project ran all six heaps and both light-cljs limits for three rounds.

### Generated 200k

| Compiler | Limit | Wall time | User CPU | System CPU | Peak memory | GC pauses | Failed | Wall time / shadow's best | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| light-cljs | light-cljs.edn's :macro-memory-limit 2147483648 (2 GiB) | 7.06 s (6.43-7.25) | 7.21 s (6.59-7.31) | 0.51 s (0.40-0.63) | 1.029 GB (1.017-1.032) | - | 0 of 3 | 0.17 | 6.39-17.64 |
| light-cljs | --macro-memory-limit 67108864 (64 MiB) | 7.19 s (6.12-8.25) | 7.40 s (6.47-8.21) | 0.64 s (0.37-1.43) | 1.041 GB (1.024-1.051) | - | 0 of 3 | 0.17 | 6.23-21.00 |
| shadow-cljs | -Xmx1g (1 GiB) | 48.18 s (45.49-51.09) | 170.80 s (166.45-177.07) | 85.95 s (66.20-90.01) | 1.735 GB (1.720-1.737) | 3.34 s (2.64-3.58), 2 full | 0 of 3 | 1.14 | 5.81-24.75 |
| shadow-cljs | -Xmx1536m (1.5 GiB) | 43.34 s (42.64-49.67) | 148.39 s (147.41-158.22) | 115.05 s (95.79-126.17) | 2.281 GB (2.272-2.299) | 1.70 s (1.66-2.36), 0 full | 0 of 3 | 1.02 | 11.64-19.12 |
| shadow-cljs | -Xmx2g (2 GiB) | 45.14 s (44.89-49.25) | 149.50 s (145.84-153.29) | 106.13 s (90.19-108.97) | 2.701 GB (2.540-2.730) | 1.78 s (1.59-2.25), 0 full | 0 of 3 | 1.07 | 11.87-15.82 |
| shadow-cljs | -Xmx3g (3 GiB) | 42.45 s (41.25-49.70) | 140.96 s (137.13-144.15) | 111.48 s (110.39-111.87) | 2.739 GB (2.694-2.927) | 1.32 s (1.26-1.94), 0 full | 0 of 3 | 1.00 | 13.24-16.01 |
| shadow-cljs | -Xmx4g (4 GiB) | 42.31 s (41.71-43.42) | 138.53 s (136.44-142.50) | 114.98 s (102.71-118.05) | 2.848 GB (2.518-2.924) | 1.30 s (1.21-1.39), 0 full | 0 of 3 | 1.00 | 11.98-16.64 |
| shadow-cljs | -Xmx8g (8 GiB) | 45.77 s (42.76-53.52) | 131.26 s (128.20-132.28) | 100.90 s (95.22-106.52) | 3.425 GB (3.371-3.717) | 1.15 s (0.94-1.47), 0 full | 0 of 3 | 1.08 | 9.33-14.15 |

### re-frame-shaped 200k

| Compiler | Limit | Wall time | User CPU | System CPU | Peak memory | GC pauses | Failed | Wall time / shadow's best | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| light-cljs | light-cljs.edn's :macro-memory-limit 4000000000 (3.73 GiB) | 8.91 s (8.59-9.34) | 8.98 s (8.68-9.01) | 0.61 s (0.57-0.87) | 1.405 GB (1.387-1.449) | - | 0 of 3 | 0.12 | 8.49-10.95 |
| light-cljs | --macro-memory-limit 67108864 (64 MiB) | 8.84 s (8.56-8.95) | 8.98 s (8.74-9.15) | 0.62 s (0.56-0.76) | 1.367 GB (1.359-1.378) | - | 0 of 3 | 0.12 | 7.34-11.32 |
| shadow-cljs | -Xmx1g (1 GiB) | 91.43 s (74.83-91.80) | 244.39 s (223.86-250.03) | 445.35 s (427.70-470.32) | 1.753 GB (1.729-1.764) | 3.48 s (2.76-3.67), 3 full | 0 of 3 | 1.24 | 6.60-13.17 |
| shadow-cljs | -Xmx1536m (1.5 GiB) | 74.31 s (68.63-79.40) | 185.19 s (178.50-194.38) | 431.48 s (279.08-434.36) | 2.268 GB (2.244-2.271) | 2.02 s (1.98-2.94), 0 full | 0 of 3 | 1.01 | 8.96-14.30 |
| shadow-cljs | -Xmx2g (2 GiB) | 73.65 s (73.10-77.90) | 180.48 s (176.21-183.71) | 416.77 s (398.89-433.80) | 2.444 GB (2.422-2.598) | 1.95 s (1.94-2.19), 0 full | 0 of 3 | 1.00 | 8.67-20.54 |
| shadow-cljs | -Xmx3g (3 GiB) | 78.95 s (73.38-99.35) | 180.66 s (179.55-182.74) | 443.88 s (327.92-453.63) | 2.680 GB (2.527-2.879) | 2.28 s (1.97-2.30), 0 full | 0 of 3 | 1.07 | 13.29-17.87 |
| shadow-cljs | -Xmx4g (4 GiB) | 73.81 s (72.55-103.25) | 174.21 s (171.67-179.30) | 315.25 s (302.83-418.07) | 3.077 GB (2.677-3.278) | 1.89 s (1.72-2.75), 0 full | 0 of 3 | 1.00 | 10.69-25.96 |
| shadow-cljs | -Xmx8g (8 GiB) | 77.80 s (75.29-79.51) | 155.40 s (154.53-167.94) | 314.65 s (296.34-440.41) | 3.932 GB (2.992-3.955) | 1.45 s (1.44-1.50), 0 full | 0 of 3 | 1.06 | 9.70-22.73 |

### Generated 800k

| Compiler | Limit | Wall time | User CPU | System CPU | Peak memory | GC pauses | Failed | Wall time / shadow's best | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| light-cljs | light-cljs.edn's :macro-memory-limit 2147483648 (2 GiB) | 37.83 s (37.36-38.59) | 37.59 s (36.67-38.37) | 2.56 s (2.50-2.82) | 3.646 GB (3.586-3.837) | - | 0 of 3 | 0.18 | 13.23-22.80 |
| light-cljs | --macro-memory-limit 67108864 (64 MiB) | - | - | - | - | - | 3 of 3 | - | 8.69-27.97 |
| shadow-cljs | -Xmx1g (1 GiB) | - | - | - | - | - | 3 of 3 (3 OutOfMemoryError) | - | 7.58-33.49 |
| shadow-cljs | -Xmx1536m (1.5 GiB) | - | - | - | - | - | 3 of 3 (3 OutOfMemoryError) | - | 15.03-17.50 |
| shadow-cljs | -Xmx2g (2 GiB) | - | - | - | - | - | 3 of 3 (3 OutOfMemoryError) | - | 10.19-24.45 |
| shadow-cljs | -Xmx3g (3 GiB) | 223.07 s (199.65-232.10) | 590.73 s (560.21-614.77) | 448.20 s (354.75-519.66) | 4.298 GB (4.036-4.306) | 8.10 s (7.20-11.01), 7 full | 0 of 3 | 1.05 | 5.25-27.08 |
| shadow-cljs | -Xmx4g (4 GiB) | 215.34 s (194.63-255.17) | 525.14 s (503.93-537.88) | 380.58 s (314.09-453.75) | 5.310 GB (5.258-5.325) | 5.75 s (5.07-8.40), 1 full | 0 of 3 | 1.01 | 5.12-13.54 |
| shadow-cljs | -Xmx8g (8 GiB) | 213.44 s (179.89-239.06) | 446.54 s (428.27-459.83) | 326.27 s (313.40-461.10) | 7.905 GB (7.665-8.011) | 5.52 s (4.03-7.48), 0 full | 0 of 3 | 1.00 | 5.85-18.35 |

The `-Xmx1g`, `-Xmx1536m` and `-Xmx2g` builds at 800k ended with
`OutOfMemoryError` after 46 to 79 s, at 1.85 to 3.06 GB peak memory, having
run 26 to 119 full collections; light-cljs at 64 MiB failed after 11 to 12 s
with "JavaScript session memory limit exhausted" while expanding a macro.
Every output that was checked printed the oracle's checksum (`484264`,
`714878 117718`, `189716`) and passed the waste check. Output sizes (200k / re-frame / 800k): light
3,449,139 / 6,223,347 / 13,368,015 bytes, shadow 4,067,637 / about 6,963,400
/ 15,710,764 bytes.

Findings, recorded as found:

1. **Too small a heap fails or slows; it never saves shadow time.** shadow
   fails outright below 3 GiB at 800k. At 200k, 1 GiB completes but is the
   slowest heap: 1.14x its best for the generated project and 1.24x for the
   re-frame one, with two to three full collections a build, about 2 to 3 times the
   pause time and 23 to 57% more user CPU than at 4 or 8 GiB. From 1.5 GiB up
   (3 GiB at 800k) wall time is flat within the load noise; the ranges of
   neighbouring heaps overlap.
2. **A bigger heap mostly buys lower CPU, not time.** shadow's user CPU falls
   steadily with heap (200k: 171 s at 1 GiB, 131 s at 8 GiB; 800k: 591 s at
   3 GiB, 447 s at 8 GiB) as collections get rarer, while peak memory grows
   with it. Its system CPU (86 to 448 s medians, large as in the November 13
   baseline) does not follow the heap.
3. **shadow's peak memory is its heap plus 0.55 to 1.1 GB.** At heaps it
   fills (1 to 2 GiB at 200k, 3 GiB at 800k) its peak memory exceeds the
   heap's size by that much: the JVM's code cache, class metadata, thread
   stacks and collector structures. From 3 GiB up at 200k it no longer fills
   the heap (2.7 to 3.4 GB peak), and how much it holds there depends on when
   the collector runs: 3.4 GB at 8 GiB today against 4.2 GB in the November 13
   baseline, which gave the earlier 0.25x ratio.
4. **light-cljs's peak memory does not depend on its limit.** Its
   `:macro-memory-limit` caps only the accounted JavaScript heap of macro
   expansion, not the process, so it is not the counterpart of `-Xmx`.
   Lowering it from the projects' 2 GiB (re-frame: 4 GB) to the 64 MiB
   default changed peak memory by under 3% at 200k (1.029 against 1.041 GB,
   1.405 against 1.367 GB), so the generous limit does not inflate light's
   numbers. At 800k the default 64 MiB is too small: the build fails, which is
   why the generator sets 2 GiB. Stated the same way as shadow's: light-cljs
   needs a macro limit above 64 MiB at 800k and then peaks at 3.65 GB; shadow
   needs a 3 GiB heap and then peaks at 4.30 GB.
5. **The 10% rule is close to the noise here.** With the load between 5 and
   34, a heap's three-build range is often wider than 10% (shadow's 8 GiB 800k
   builds: 179.9 to 239.1 s), so the adequate heap could move by one step on
   a reserved machine: down to 1 GiB at 200k if its 14% and 24% slowdowns were
   load (light/shadow 0.59 and 0.80), or up to 4 GiB at 800k (0.69). The 800k answer cannot
   move below 3 GiB, since every smaller heap ran out of memory.
6. **shadow's re-frame output is not byte-identical across builds**: 6,963,390
   to 6,963,436 bytes in the last build at each heap, all passing the oracle.
   The generated workload projects' shadow output was identical at every heap.

## Growth from 50k to 800k lines, both shapes, one binary, one day, unreserved machine (November 14, 2026)

**Summary.** The objection: growth from 200k to 800k "looked superlinear"
(5.6x for light-cljs against shadow-cljs's 4.0x), from runs on different
dates. So both compilers built both generated shapes at five sizes on one day
with one binary. **light-cljs's clean build does grow faster than the
program, and a little faster than shadow-cljs's**: from 200k to 800k (4.0x
the lines) its median wall time grows 5.4x for the plain shape (exponent 1.21)
and 5.8x for the re-frame shape (1.27), against shadow's 4.7x (1.12) and 5.7x
(1.25). Its clean build stays 5 to 12 times faster than shadow's at every size
(light/shadow wall time 0.084 to 0.19). Its peak memory grows more slowly than the program (3.6x and 3.5x,
exponents 0.93 and 0.89); shadow's is held down by its 8 GiB heap. The
superlinear part of light-cljs is in analysis: `--stats` names four terms,
filed as pellets, each growing 6x to 22x where the program grows 4x. **The
rebuild after a one-file edit is worse**: it grows 5.8x (plain) and 6.1x
(re-frame) and is slower than a clean build at 400k in both shapes and at
re-frame 800k (82.7 s against 51.1 s; plain 800k is faster, 27.5 s against
32.5 s), because the edit recompiles every later namespace
that ran a source macro and capturing each recompiled namespace into the
cache costs more the larger the program.

| Shape | Build | light-cljs 200k -> 800k | exponent | shadow-cljs 200k -> 800k | exponent | light/shadow at 800k |
| --- | --- | --- | --- | --- | --- | --- |
| plain | clean build, wall time | 6.04 -> 32.50 s, 5.38x | 1.21 | 40.07 -> 188.38 s, 4.70x | 1.12 | 0.17 |
| plain | clean build, peak memory | 1.049 -> 3.790 GB, 3.61x | 0.93 | 4.146 -> 7.788 GB, 1.88x | 0.46 | 0.49 |
| plain | one-file edit, wall time | 4.71 -> 27.54 s, 5.85x | 1.28 | 34.73 -> 160.47 s, 4.62x | 1.10 | 0.17 |
| re-frame | clean build, wall time | 8.82 -> 51.08 s, 5.79x | 1.27 | 73.17 -> 416.46 s, 5.69x | 1.25 | 0.12 |
| re-frame | clean build, peak memory | 1.405 -> 4.855 GB, 3.46x | 0.89 | 4.042 -> 8.097 GB, 2.00x | 0.50 | 0.60 |
| re-frame | one-file edit, wall time | 13.64 -> 82.74 s, 6.07x | 1.30 | 57.60 -> 346.46 s, 6.01x | 1.29 | 0.24 |

(Exponent: log(ratio of times) / log(ratio of lines), 1 for linear growth;
the lines grow 4.00x in both shapes.)

Setup. light-cljs master `fc31d229` as the immutable release binary
`research/shadow-credibility/bin/light-cljs-fc31d229` (V8 build, SHA-256
`45b2f5a1b76b4a4a...`), shadow-cljs 3.5.3 with the generated `-Xmx8g`, OpenJDK
25.0.2, Node v26.12.1, Clojure CLI 1.12.4.1618, Babashka 1.13.229, Apple M3
Ultra (Virtual), 24 CPUs, 32 GiB, macOS 15.7.3. Projects: `generate.bb --seed
42 --lines 50k|100k|200k|400k|800k`, and the same with `--shape re-frame`, in
`research/shadow-credibility/projects/`. The 200k and 800k plain projects and
the 200k re-frame one were generated again as `*-seed42-970` (identical
sources to the shared ones; manifests differ only in the generator's hash)
so that two other workers reading the shared projects' outputs were not
disturbed. Raw EDN, Markdown, logs and `--stats` output are in
`research/shadow-credibility/growth-fc31d229/` (gitignored); the fitted
points are [results/generated-growth-2026-11-14.edn](results/generated-growth-2026-11-14.edn),
one map per size, shape, compiler and build, with every measure's median and
range, and one per fitted series. Timings ran from 02:36 to 04:25 MST.

```sh
R=research/shadow-credibility
# each project, smallest first, the plain then the re-frame shape at each size:
bb benchmarks/generated-project/compare.bb $R/projects/PROJECT --light $R/bin/light-cljs-fc31d229 \
  --leaf src/workload/LEAF.cljs --runs 3 --max-load 1000 --max-repeats 0 --skip-unchanged \
  --out $R/growth-fc31d229/PROJECT          # --runs 1 for re-frame-800k-seed42
# one --stats build per size, and two more at 200k and 800k (--stats-file):
(cd $R/projects/PROJECT && $R/bin/light-cljs-fc31d229 release bench --no-cache --stats) > $R/growth-fc31d229/stats/PROJECT.txt 2>&1
bb benchmarks/generated-project/growth.bb --out $R/growth-fc31d229/growth \
  $R/growth-fc31d229/*-seed42*.edn $R/growth-fc31d229/stats/*.txt
```

Method. Each size is `compare.bb`'s clean build (light `--no-cache`, shadow
with `.shadow-cljs` removed) and rebuild after a one-file edit, three rounds
each, the compilers alternating; the rebuild with nothing changed was left
out (`--skip-unchanged`, added for this run) to save time, its untimed
cache-filling build still preceding the edit. The edited file is a leaf: the
first worker namespace, alphabetically, that only a group namespace requires
(`baklestei` at 50k and 100k, `babomhol` at 200k and 400k, `babra` at 800k;
re-frame shape `draijakvo`, `batulhok`, `besmeik`, `bouskoknu`, `badam`).
Every output printed the project oracle's checksum and passed
`check-waste.bb --strict`.

**What was cut.** The re-frame-shaped 800k project ran **one round**, not
three (its shadow builds take 6 to 7 minutes each); every other project ran
three. Nothing else was cut.

**The machine was not reserved.** Two other workers (light-cljs-972, the
watch loop, and light-cljs-975, runtime speed of outputs) were working
throughout. Following the user's decision of November 13 ("please dont stop
because of the machine not being free"), the gate was off (`--max-load 1000`,
no repeats). The 1-minute load average before a build ranged from 2.8 to 14.7
on 24 CPUs and is in the tables; the highest loads fell on the plain 400k
project (7.8 to 14.7 before light's clean builds), whose light step looks
steepest (2.67x from 200k to 400k, then 2.02x to 800k). The `--stats` builds
ran outside the timed runs (before them, and after them at 200k and 800k), at
load 4 to 14.

### Every size

`growth.bb`'s tables. Exponent: the slope of a least-squares fit of log(time)
against log(lines) over all five sizes; step ratios: each size's median over
the previous size's, with that step's exponent in brackets. shadow's fixed
cost (JVM start and Closure setup, about 10 to 15 s) makes its exponent over
the whole range small; the 400k to 800k step is the fairer comparison.

#### re-frame shape

| Nonblank lines | Build | Compiler | Rounds | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 50,650 | clean build | light-cljs | 3 | 1.89 s (1.88-1.90) | 1.99 s (1.99-2.01) | 0.14 s (0.13-0.14) | 0.486 GB (0.484-0.487) | 7.88-10.03 |
| 50,650 | clean build | shadow-cljs | 3 | 22.59 s (22.03-24.68) | 76.85 s (75.40-77.86) | 115.00 s (107.71-120.28) | 2.877 GB (1.964-3.166) | 7.88-10.03 |
| 50,650 | rebuild after a one-file edit | light-cljs | 3 | 1.87 s (1.87-2.04) | 1.83 s (1.82-1.94) | 0.22 s (0.21-0.27) | 0.456 GB (0.454-0.461) | 9.94-12.66 |
| 50,650 | rebuild after a one-file edit | shadow-cljs | 3 | 16.21 s (16.12-16.24) | 70.37 s (69.88-72.70) | 8.92 s (8.38-9.05) | 1.732 GB (1.671-1.793) | 9.94-12.66 |
| 101,130 | clean build | light-cljs | 3 | 3.85 s (3.78-4.15) | 3.90 s (3.83-4.23) | 0.29 s (0.24-0.33) | 0.820 GB (0.812-0.824) | 5.88-9.97 |
| 101,130 | clean build | shadow-cljs | 3 | 34.63 s (34.16-34.67) | 96.45 s (93.89-98.48) | 218.93 s (215.83-219.29) | 2.511 GB (2.369-2.713) | 5.57-9.89 |
| 101,130 | rebuild after a one-file edit | light-cljs | 3 | 4.28 s (3.93-4.34) | 4.13 s (3.79-4.16) | 0.47 s (0.42-0.52) | 0.772 GB (0.771-0.780) | 7.59-9.88 |
| 101,130 | rebuild after a one-file edit | shadow-cljs | 3 | 24.67 s (24.43-25.44) | 87.63 s (87.48-88.90) | 11.03 s (9.44-13.18) | 2.277 GB (2.096-2.312) | 7.22-9.88 |
| 202,053 | clean build | light-cljs | 3 | 8.82 s (8.34-9.63) | 8.77 s (8.45-9.71) | 0.56 s (0.52-0.70) | 1.405 GB (1.398-1.435) | 7.36-9.08 |
| 202,053 | clean build | shadow-cljs | 3 | 73.17 s (70.43-73.23) | 158.66 s (152.55-164.67) | 418.38 s (417.95-427.13) | 4.042 GB (3.018-4.051) | 6.62-8.68 |
| 202,053 | rebuild after a one-file edit | light-cljs | 3 | 13.64 s (10.69-15.55) | 10.84 s (10.36-10.93) | 2.22 s (1.13-2.70) | 1.400 GB (1.358-1.412) | 11.15-11.72 |
| 202,053 | rebuild after a one-file edit | shadow-cljs | 3 | 57.60 s (54.91-63.22) | 141.94 s (140.20-167.91) | 15.76 s (14.52-25.90) | 3.153 GB (2.834-3.542) | 8.75-11.99 |
| 404,134 | clean build | light-cljs | 3 | 19.49 s (19.09-19.52) | 19.42 s (19.41-19.90) | 1.00 s (0.97-1.11) | 2.513 GB (2.486-2.522) | 5.48-7.16 |
| 404,134 | clean build | shadow-cljs | 3 | 141.02 s (137.43-141.20) | 274.16 s (273.66-280.61) | 848.72 s (843.60-864.24) | 5.381 GB (4.877-5.405) | 5.08-7.46 |
| 404,134 | rebuild after a one-file edit | light-cljs | 3 | 30.00 s (27.85-30.53) | 27.51 s (26.15-28.85) | 2.54 s (2.44-3.03) | 2.552 GB (2.549-2.564) | 4.04-7.87 |
| 404,134 | rebuild after a one-file edit | shadow-cljs | 3 | 95.90 s (94.80-100.24) | 218.29 s (215.82-221.27) | 12.66 s (12.14-14.26) | 5.114 GB (5.082-5.293) | 4.05-6.16 |
| 808,055 | clean build | light-cljs | 1 | 51.08 s (51.08-51.08) | 51.14 s (51.14-51.14) | 2.47 s (2.47-2.47) | 4.855 GB (4.855-4.855) | 4.77-4.77 |
| 808,055 | clean build | shadow-cljs | 1 | 416.46 s (416.46-416.46) | 726.72 s (726.72-726.72) | 1698.64 s (1698.64-1698.64) | 8.097 GB (8.097-8.097) | 4.16-4.16 |
| 808,055 | rebuild after a one-file edit | light-cljs | 1 | 82.74 s (82.74-82.74) | 76.42 s (76.42-76.42) | 6.54 s (6.54-6.54) | 5.053 GB (5.053-5.053) | 4.36-4.36 |
| 808,055 | rebuild after a one-file edit | shadow-cljs | 1 | 346.46 s (346.46-346.46) | 556.01 s (556.01-556.01) | 106.01 s (106.01-106.01) | 8.621 GB (8.621-8.621) | 2.80-2.80 |

| Build | Compiler | Measure | Exponent (log-log fit) | Step ratios (exponent) |
| --- | --- | --- | --- | --- |
| clean build | light-cljs | wall time | 1.19 | 2.04x (1.03), 2.29x (1.20), 2.21x (1.14), 2.62x (1.39) |
| clean build | light-cljs | CPU (user + system) | 1.16 | 1.94x (0.96), 2.25x (1.17), 2.20x (1.14), 2.61x (1.39) |
| clean build | light-cljs | peak memory | 0.83 | 1.69x (0.76), 1.71x (0.78), 1.79x (0.84), 1.93x (0.95) |
| clean build | shadow-cljs | wall time | 1.04 | 1.53x (0.62), 2.11x (1.08), 1.93x (0.95), 2.95x (1.56) |
| clean build | shadow-cljs | CPU (user + system) | 0.92 | 1.63x (0.71), 1.84x (0.88), 1.96x (0.97), 2.15x (1.10) |
| clean build | shadow-cljs | peak memory | 0.41 | 0.87x (-0.20), 1.61x (0.69), 1.33x (0.41), 1.50x (0.59) |
| rebuild after a one-file edit | light-cljs | wall time | 1.38 | 2.29x (1.20), 3.19x (1.67), 2.20x (1.14), 2.76x (1.46) |
| rebuild after a one-file edit | light-cljs | CPU (user + system) | 1.34 | 2.27x (1.19), 2.84x (1.51), 2.29x (1.19), 2.76x (1.47) |
| rebuild after a one-file edit | light-cljs | peak memory | 0.87 | 1.69x (0.76), 1.81x (0.86), 1.82x (0.87), 1.98x (0.99) |
| rebuild after a one-file edit | shadow-cljs | wall time | 1.08 | 1.52x (0.61), 2.33x (1.23), 1.66x (0.74), 3.61x (1.85) |
| rebuild after a one-file edit | shadow-cljs | CPU (user + system) | 0.74 | 1.25x (0.32), 1.68x (0.75), 1.39x (0.47), 2.87x (1.52) |
| rebuild after a one-file edit | shadow-cljs | peak memory | 0.58 | 1.31x (0.40), 1.38x (0.47), 1.62x (0.70), 1.69x (0.75) |

#### Plain shape

| Nonblank lines | Build | Compiler | Rounds | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 50,074 | clean build | light-cljs | 3 | 1.62 s (1.59-1.63) | 1.69 s (1.64-1.70) | 0.10 s (0.10-0.11) | 0.380 GB (0.378-0.390) | 5.79-7.73 |
| 50,074 | clean build | shadow-cljs | 3 | 16.87 s (16.86-17.07) | 71.92 s (71.82-72.88) | 27.73 s (27.40-29.81) | 2.774 GB (2.661-2.873) | 5.79-8.06 |
| 50,074 | rebuild after a one-file edit | light-cljs | 3 | 0.83 s (0.82-0.83) | 0.81 s (0.81-0.82) | 0.05 s (0.05-0.05) | 0.310 GB (0.307-0.317) | 7.69-8.30 |
| 50,074 | rebuild after a one-file edit | shadow-cljs | 3 | 13.85 s (13.77-14.22) | 67.17 s (65.51-67.81) | 8.31 s (8.18-8.78) | 1.852 GB (1.783-2.013) | 7.69-7.93 |
| 99,626 | clean build | light-cljs | 3 | 2.94 s (2.82-3.09) | 3.04 s (2.92-3.16) | 0.19 s (0.16-0.21) | 0.622 GB (0.620-0.633) | 9.19-9.90 |
| 99,626 | clean build | shadow-cljs | 3 | 24.16 s (24.11-24.51) | 89.12 s (89.10-90.44) | 59.31 s (56.08-59.87) | 2.732 GB (2.518-2.899) | 8.69-9.96 |
| 99,626 | rebuild after a one-file edit | light-cljs | 3 | 3.03 s (2.89-3.17) | 3.14 s (2.93-3.17) | 0.22 s (0.21-0.24) | 0.587 GB (0.578-0.595) | 6.08-8.92 |
| 99,626 | rebuild after a one-file edit | shadow-cljs | 3 | 19.65 s (19.42-19.79) | 78.86 s (78.68-79.91) | 8.96 s (8.32-10.68) | 2.665 GB (2.290-2.896) | 5.67-8.37 |
| 199,129 | clean build | light-cljs | 3 | 6.04 s (5.99-6.42) | 6.23 s (6.20-6.58) | 0.32 s (0.31-0.36) | 1.049 GB (1.019-1.059) | 7.30-7.85 |
| 199,129 | clean build | shadow-cljs | 3 | 40.07 s (39.79-40.48) | 124.55 s (123.02-124.78) | 117.96 s (114.42-118.41) | 4.146 GB (3.522-4.177) | 6.69-7.00 |
| 199,129 | rebuild after a one-file edit | light-cljs | 3 | 4.71 s (4.49-4.73) | 4.71 s (4.61-4.84) | 0.31 s (0.30-0.33) | 0.981 GB (0.958-1.000) | 6.03-10.55 |
| 199,129 | rebuild after a one-file edit | shadow-cljs | 3 | 34.73 s (32.76-40.26) | 101.38 s (100.20-107.38) | 12.63 s (12.50-14.49) | 3.881 GB (3.334-4.128) | 6.03-9.16 |
| 397,920 | clean build | light-cljs | 3 | 16.11 s (14.22-16.95) | 16.31 s (14.46-17.19) | 0.89 s (0.88-0.98) | 1.939 GB (1.925-1.943) | 7.82-14.72 |
| 397,920 | clean build | shadow-cljs | 3 | 86.26 s (83.05-88.70) | 216.95 s (210.56-223.28) | 236.79 s (217.85-245.34) | 5.067 GB (4.494-5.638) | 6.75-12.98 |
| 397,920 | rebuild after a one-file edit | light-cljs | 3 | 18.94 s (18.80-19.26) | 20.53 s (20.31-20.82) | 1.29 s (1.21-1.35) | 1.932 GB (1.911-1.963) | 4.76-13.10 |
| 397,920 | rebuild after a one-file edit | shadow-cljs | 3 | 66.04 s (64.78-69.36) | 162.60 s (154.63-166.84) | 18.89 s (16.88-19.08) | 4.900 GB (4.727-5.433) | 4.27-10.66 |
| 795,693 | clean build | light-cljs | 3 | 32.50 s (32.34-35.75) | 33.06 s (32.93-36.02) | 1.57 s (1.56-1.83) | 3.790 GB (3.789-3.896) | 3.94-5.45 |
| 795,693 | clean build | shadow-cljs | 3 | 188.38 s (183.69-205.64) | 438.72 s (424.74-454.00) | 504.54 s (490.29-525.42) | 7.788 GB (7.630-7.874) | 2.89-7.36 |
| 795,693 | rebuild after a one-file edit | light-cljs | 3 | 27.54 s (27.17-27.75) | 29.78 s (29.60-30.48) | 1.57 s (1.54-1.60) | 3.502 GB (3.494-3.529) | 4.20-4.82 |
| 795,693 | rebuild after a one-file edit | shadow-cljs | 3 | 160.47 s (160.06-167.17) | 319.05 s (311.76-326.25) | 28.13 s (21.49-28.63) | 7.966 GB (7.850-8.705) | 4.02-4.32 |

| Build | Compiler | Measure | Exponent (log-log fit) | Step ratios (exponent) |
| --- | --- | --- | --- | --- |
| clean build | light-cljs | wall time | 1.11 | 1.81x (0.87), 2.05x (1.04), 2.67x (1.42), 2.02x (1.01) |
| clean build | light-cljs | CPU (user + system) | 1.10 | 1.80x (0.86), 2.02x (1.02), 2.63x (1.40), 2.01x (1.01) |
| clean build | light-cljs | peak memory | 0.83 | 1.64x (0.72), 1.69x (0.75), 1.85x (0.89), 1.95x (0.97) |
| clean build | shadow-cljs | wall time | 0.88 | 1.43x (0.52), 1.66x (0.73), 2.15x (1.11), 2.18x (1.13) |
| clean build | shadow-cljs | CPU (user + system) | 0.81 | 1.48x (0.57), 1.63x (0.70), 1.88x (0.91), 2.08x (1.06) |
| clean build | shadow-cljs | peak memory | 0.39 | 0.98x (-0.02), 1.52x (0.60), 1.22x (0.29), 1.54x (0.62) |
| rebuild after a one-file edit | light-cljs | wall time | 1.28 | 3.65x (1.88), 1.55x (0.64), 4.02x (2.01), 1.45x (0.54) |
| rebuild after a one-file edit | light-cljs | CPU (user + system) | 1.31 | 3.91x (1.98), 1.49x (0.58), 4.36x (2.13), 1.43x (0.52) |
| rebuild after a one-file edit | light-cljs | peak memory | 0.87 | 1.89x (0.93), 1.67x (0.74), 1.97x (0.98), 1.81x (0.86) |
| rebuild after a one-file edit | shadow-cljs | wall time | 0.88 | 1.42x (0.51), 1.77x (0.82), 1.90x (0.93), 2.43x (1.28) |
| rebuild after a one-file edit | shadow-cljs | CPU (user + system) | 0.55 | 1.18x (0.24), 1.28x (0.36), 1.59x (0.67), 1.91x (0.94) |
| rebuild after a one-file edit | shadow-cljs | peak memory | 0.51 | 1.44x (0.53), 1.46x (0.54), 1.26x (0.34), 1.63x (0.70) |

### Which light-cljs phases grow faster than the program

`--stats` builds (one per size, three at 200k and 800k, medians), 200k to
800k, against the lines' 4.0x:

| Counter | plain 200k -> 800k | growth | re-frame 200k -> 800k | growth |
| --- | --- | --- | --- | --- |
| time.wall-ms | 6,588 -> 34,508 | 5.2x | 8,389 -> 53,404 | 6.4x |
| time.analysis-ms | 2,211 -> 14,427 | **6.5x** | 2,319 -> 24,751 | **10.7x** |
| top-level pass outside the engine (trace.top-level-ms less its engine-ms) | 1,393 -> 9,513 | **6.8x** | 1,664 -> 16,765 | **10.1x** |
| trace.top-level.engine-ms | 519 -> 2,201 | 4.2x | 233 -> 944 | 4.1x |
| trace.other-ms | 157 -> 1,565 | **10.0x** | 314 -> 6,358 | **20.2x** |
| trace.append-ms | 102 -> 976 | **9.6x** | 52 -> 350 | **6.8x** |
| time.deferred-units-ms | 2,664 -> 12,129 | 4.6x | 2,900 -> 13,664 | 4.7x |
| time.prepare-application-ms | 1,018 -> 5,014 | 4.9x | 1,761 -> 7,996 | 4.5x |
| time.emission-ms | 675 -> 2,913 | 4.3x | 1,223 -> 5,423 | 4.4x |
| peak memory (memory.peak-rss-bytes) | 1.05 -> 3.78 GB | 3.6x | 1.40 -> 4.88 GB | 3.5x |
| macro calls, namespaces, deferred units | | 4.0x | | 3.9x |

Deferred units, preparation and emission grow within 8 to 23% of the
program; the engine's share of every stage grows with it. The excess is in
analysis outside the engine. At re-frame 800k the build is 19.8 s longer than
4x its 200k time, and the top-level pass (10.1 s over 4x) and the
unattributed loop time (5.1 s) account for 15 s of it.

- **The top-level pass's cost per byte grows with the namespace's position.**
  From the `--stats-file` namespace rows, its time outside the engine per KB
  of source rises from about 510 µs in the second tenth of the re-frame 800k
  namespaces to 990 to 1,260 µs in the last tenth (plain 800k: 145 to
  344 µs; at 200k: 236 to 334 and 111 to 190), so each namespace pays for
  the namespaces before it. A 30-second `sample` of the re-frame 800k build
  found one cause: after each pass `NamespaceTop::analyze` scans every
  program namespace to check membership before adding its own (about 3 s at
  800k). Filed as **light-cljs-994**.
- **Analysis-loop time outside every trace frame** (`trace.other-ms`) is
  close to quadratic: step exponents 2.2 and 2.1 for the re-frame shape from
  200k to 400k to 800k. With `--no-cache` it holds no cache work.
  **light-cljs-995**.
- **Appending fragments** grows 9.6x in the plain shape (fit exponent 1.55),
  under 1 s at 800k. **light-cljs-996**.
- **Peak memory grows more slowly than the program** in both shapes
  (exponents 0.81 and 0.82 over all sizes), so it is not a growth problem.

### The rebuild after a one-file edit

light-cljs's rebuild after a one-file edit grows irregularly with size (plain:
3.65x, 1.55x, 4.02x, 1.45x per doubling) and is **slower than its clean
build** at plain 400k (18.94 s against 16.11 s) and re-frame 400k and 800k
(30.00 s against 19.49 s, 82.74 s against 51.08 s); at plain 800k it is
faster (27.54 s against 32.50 s), since that edit recompiled only 17% of the
namespaces. shadow's is 0.68x to 0.87x of
its clean build at every size. A `--stats` rebuild per size on the caches the
runs left (one edit each, not timed for the tables; the 50k to 400k ones
ran, one light-cljs build at a time, while the plain 800k project was being
timed, so that project's clean builds carry one more busy core) shows two
causes:

- **How much an edit recompiles depends on where the edited namespace falls
  in compile order, not on the program's size.** Every later namespace that
  ran a source macro misses with "earlier namespaces changed": a source
  macro sees the whole compilation's declaration view, so it is the cache's
  coarse fallback (docs/compiler-cache.md), and every generated worker
  expands its group's source macros. The same one-`defn` edit recompiled 9 of
  144 namespaces at 50k, 130 of 288 at 100k, 106 of 569 at 200k, 587 of 1,134
  at 400k and 375 of 2,264 at 800k (plain); in the re-frame shape 22% to 38%
  of them (4,613 of 12,143 at 800k). This is a design property, not a phase
  that grows; changing it would be a design change and is left to the user.
- **Capturing each recompiled namespace into the cache costs more the larger
  the program**: `cache.capture-ms` per captured namespace is 0.36, 0.66,
  1.22, 2.66 and 5.22 ms from 50k to 800k in the re-frame shape (1.35 to
  4.36 ms from 200k to 800k, plain). At re-frame 800k capture took 24.1 s,
  validation 15.6 s and writing 7.9 s of a 97 s rebuild. Filed as
  **light-cljs-997**.

Findings, recorded as found:

1. **The 200k to 800k growth is superlinear for both compilers on both
   shapes**, and light-cljs's is the steeper: clean build 5.4x and 5.8x
   against shadow's 4.7x and 5.7x for 4.0x the lines. The earlier 5.6x
   against 4.0x came from runs on different dates and loads; on one day the
   gap is smaller, but it is real in the plain shape and it is light-cljs's.
2. **light-cljs remains far faster at every size**: 0.084x to 0.19x of
   shadow's clean-build wall time, 0.060x to 0.31x of its rebuild after a
   one-file edit (the worst, 0.31x, at re-frame 400k, where light's edit
   rebuild is slower than its clean build).
3. **The plain shape's 400k step (2.67x from 200k) is steeper than its
   neighbours (2.05x, 2.02x)**, and ran at the day's highest load; the
   `--stats` builds, run at a steadier load, show the same step (2.73x), so
   it is not only load. The fitted exponents use all five sizes.
4. **shadow's peak memory is set by its heap.** It grows 0.87x to 1.61x per
   doubling and reaches 7.8 to 8.6 GB at 800k, the `-Xmx8g` ceiling; the
   November 14 heap sweep above gives what it needs. Its fitted exponents
   (0.39, 0.41) say nothing about its requirements.
5. **shadow's re-frame 800k clean build used 1,699 s of system CPU** against
   727 s of user CPU in 416 s of wall time (one round), the largest system
   share recorded in these comparisons (see the November 13 baseline's
   finding 2).

## Why a one-file edit rebuilds slower than a clean build at 400k, unreserved machine (November 14, 2026, light-cljs-1000)

**Summary.** The growth section above found light-cljs's rebuild after a
one-file edit slower than its clean build from 400k up. On the 400k
projects, one job, three alternated rounds, the same edit (an unused `defn`
appended to a leaf worker) recompiles **587 of 1,134 namespaces (plain) and
2,127 of 6,092 (re-frame shape)**, and the rebuild takes **19.46 s against a
14.76 s clean build (plain) and 30.82 s against 18.77 s (re-frame)**. Three
things add up to more than a clean build:

1. **The coarse fallback** (docs/compiler-cache.md, "Conservative keys and
   invalidation") recompiles every later namespace that ran a source macro
   (504 and 1,550 misses for "earlier namespaces changed"). A probe binary
   whose view left out the earlier-namespace chain recompiled only the leaf
   and its reverse require closure (3 and 4 namespaces) in **6.66 s and
   11.72 s**, with byte-identical output: the fallback costs 13 s and 19 s
   of this edit. Narrowing it changes the cache's dependency model, so it is
   proposed, not built (docs/compiler-cache.md, "A narrower view for
   source-macro users").
2. **Recompiling with the cache on costs 1.4x to 2.0x per namespace** what a
   clean build pays, because every recompiled namespace is captured and its
   observations recorded (the cache-writing cost of light-cljs-461): a
   cache-writing build with nothing cached takes 25.5 s and 48.8 s against
   14.8 s and 18.8 s.
3. **Cache work grows with the edit's reach and with past edits**: 4.8 s
   (plain) and 13.7 s (re-frame) of validation, decoding, installing,
   capturing and writing. Validation reads and hashes every retained
   candidate of a namespace that misses in full before checking its key, so
   it grew by about 2 s per earlier edit at re-frame 400k (3.3, 5.6, 7.5 s);
   filed as **light-cljs-1003**.

More jobs do not help: at `--jobs 8` the edit rebuild takes the same wall
time as at one job (plain 26.8 and 29.1 s against 28.1 s, re-frame 26.5 and
29.8 s against 28.3 and 30.2 s, loaded machine) for about 2.8x and 1.3x the
user CPU, since with capture on every deferred unit enters the macro host
(16,123 of 16,123 against 3,053 at one job, plain) and the coordinator's
analysis grows from 9.4 s to 17 to 19 s (plain). The clean build does gain
(plain 14.6 to 10.2 s).

One local fix keeps the model and was built: a manifest no longer lists a
source-macro user's coarse edges by name, one per earlier namespace, only
its flag (`0a56a693`). At re-frame 400k that halves the bytes an edit
rebuild writes (181.9 to 92.8 MB) and cuts what it reads; the wall-time
gain at 400k is inside the spread, and at re-frame 800k, alternated, the
edit rebuild takes 68.8 and 66.0 s instead of 75.3 and 84.4 s (below).

Setup. light-cljs master `8eefcf09` as the immutable release binary
`research/shadow-credibility/bin/light-cljs-8eefcf09` (V8 build, SHA-256
`3fc36ffd45831688...`); the fix `0a56a693` as `bin/light-cljs-0a56a693`
(`b87dc83abfd4a7bc...`). The probe binary is `8eefcf09` plus two
environment switches used only for this measurement and not committed:
`PROBE_1000_NO_COARSE_VIEW` (the view leaves out the earlier-namespace chain)
and `PROBE_1000_IMPLICIT_COARSE` (the fix's manifest; the lookup side of the
fix is always on in it and changes nothing for a listed manifest). Projects:
`research/shadow-credibility/projects/{workload,re-frame}-400k-seed42-1000`,
copies of the growth run's projects without outputs or caches; leaves
`src/workload/babomhol.cljs` and `src/workload/bouskoknu.cljs` as there.
Raw `--stats` and `/usr/bin/time -l` output, the load before each build and
an output hash after each build are in
`research/shadow-credibility/edit-1000-8eefcf09/` (gitignored). Each build
was `light-cljs release bench --jobs 1 --stats`, with `--no-cache` or its
mode's own `--cache-dir`, filled by one untimed cache-writing build per mode;
each round wrote one new `defn` used by all its builds, then ran the clean
build and one edit rebuild per mode, the modes' order rotating each round.
All 30 builds of the alternated runs printed one output hash per project.

**The machine was not reserved.** Two other workers (light-cljs-989 and
light-cljs-992) built and ran test suites throughout. The 1-minute load
before the alternated builds was 3.7 to 5.1 (plain) and 4.2 to 7.8
(re-frame; up to 11.0 before the fills). The first, unalternated probe (one
round per job count, the `--jobs 8` numbers above) ran at load 6 to 50; its
re-frame clean builds (90 and 61 s at load 27 to 47) are not used.

### Alternated rounds, one job

| Project | Build | Wall time | User CPU | System CPU | Peak memory | Recompiled |
| --- | --- | --- | --- | --- | --- | --- |
| plain 400k | clean build | 14.76 s (14.51-16.40) | 15.20 s (15.00-16.85) | 0.96 s (0.83-1.20) | 1.932 GB (1.920-1.937) | 1,134 |
| plain 400k | one-file edit | 19.46 s (17.88-20.00) | 20.72 s (19.88-21.51) | 1.40 s (1.21-1.72) | 1.969 GB (1.936-2.012) | 587 |
| plain 400k | one-file edit, fix's manifest | 19.64 s (19.25-19.75) | 21.06 s (20.92-21.20) | 1.45 s (1.39-1.56) | 1.972 GB (1.955-1.997) | 587 |
| plain 400k | one-file edit, probe without the earlier-namespace view | 6.66 s (6.43-6.78) | 6.20 s (6.09-6.25) | 0.39 s (0.38-0.43) | 1.755 GB (1.713-1.761) | 3 |
| re-frame 400k | clean build | 18.77 s (18.23-19.66) | 18.88 s (18.36-19.75) | 1.15 s (1.11-1.24) | 2.467 GB (2.436-2.482) | 6,092 |
| re-frame 400k | one-file edit | 30.82 s (26.81-32.35) | 27.73 s (24.81-27.87) | 2.91 s (2.59-3.44) | 2.502 GB (2.493-2.512) | 2,127 |
| re-frame 400k | one-file edit, fix's manifest | 29.15 s (26.61-29.27) | 24.79 s (23.87-26.00) | 3.18 s (2.78-3.58) | 2.531 GB (2.531-2.545) | 2,127 |
| re-frame 400k | one-file edit, probe without the earlier-namespace view | 11.72 s (11.58-11.80) | 9.61 s (9.60-9.74) | 1.14 s (1.10-1.16) | 2.350 GB (2.336-2.367) | 4 |

Where the time goes (`--stats` medians in ms; the cache-writing build with
nothing cached ran once per mode, the base mode's shown):

| Project | Build | Analysis | Deferred units | Preparation | Emission | Validation | Decoding + installing | Capture | Writing | Read / written |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| plain | clean build | 5,491 | 5,650 | 2,227 | 1,371 | | | | | |
| plain | cache-writing, nothing cached | 8,525 | 8,351 | 2,295 | 1,371 | 13 | 0 | 1,601 | 2,628 | 0 / 198 MB |
| plain | one-file edit | 5,558 | 4,849 | 2,041 | 1,332 | 1,714 (875-1,899) | 519 | 1,017 | 1,536 | 318 / 120 MB |
| plain | probe without the earlier-namespace view | 460 | 20 | 1,849 | 1,311 | 1,250 | 1,521 | 5 | 4 | 198 / 0.3 MB |
| re-frame | clean build | 6,905 | 5,413 | 3,658 | 2,416 | | | | | |
| re-frame | cache-writing, nothing cached | 9,808 | 9,273 | 3,378 | 2,316 | 64 | 0 | 12,330 | 5,598 | 0 / 264 MB |
| re-frame | one-file edit | 4,245 | 5,230 | 3,242 | 2,450 | 5,604 (3,310-7,495) | 782 | 4,582 | 2,766 | 446 / 182 MB |
| re-frame | one-file edit, fix's manifest | 4,430 | 5,459 | 3,370 | 2,428 | 3,233 (2,784-4,392) | 810 | 4,813 | 2,340 | 267 / 93 MB |
| re-frame | probe without the earlier-namespace view | 324 | 215 | 2,982 | 2,346 | 3,111 | 1,940 | 16 | 22 | 266 / 1.7 MB |

- **What a leaf edit invalidates.** Plain: the leaf (its source), 82
  namespaces for a changed dependency and 504 for their view; re-frame: the
  leaf, 1,550 for their view and 576 for a dependency. The leaf's reverse
  require closure is 2 and 3 namespaces (`workload.g22`, `workload.main`;
  `workload.g12`, `app.core`, `workload.main`), exactly what the probe
  recompiled. Everything else was recompiled only because it ran a source
  macro after the leaf in source order, or requires a namespace that did.
- **Why that exceeds a clean build.** The re-frame edit recompiles 35% of
  the namespaces but spends 61% of the clean build's analysis time and 97% of
  its deferred-unit time on them: per namespace, analysis costs 2.0 ms
  against 1.13 ms and a deferred unit 0.30 ms against 0.16 ms (plain: 9.5
  against 4.8 ms, 0.30 against 0.21 ms). The cache-writing build with
  nothing cached shows the same factor with no installed namespace (analysis
  1.42x and 1.55x, deferred units 1.71x and 1.48x the clean build's), so it
  is capture and observation recording. On top come 4.8 s and 13.7 s of
  cache work, and the whole-program preparation and emission (3.4 s and
  5.7 s) that every advanced build pays.
- **The floor.** With only the closure to recompile, the rebuild is
  preparation, emission, validation, decoding and installing: 6.7 s and
  11.7 s, 45% and 62% of a clean build. Validation (1.25 s and 3.1 s) reads
  and hashes every installed artifact.
- **Validation grows with past edits.** A namespace that misses reads every
  candidate its index retains (`--cache-retention`, three by default) in full
  and checks its SHA-256 before comparing its key, and each earlier edit
  leaves one more stale candidate: bytes read per edit rebuild 264, 446 and
  628 MB in rounds one to three at re-frame 400k, validation 3.3, 5.6 and
  7.5 s; the probe's all-hit rebuilds stay at 266 MB and 3.1 s. Comparing
  the key first and hashing only the candidate that matches is local:
  **light-cljs-1003**.
- **The fix's manifest.** At re-frame 400k the coarse edges' names were half
  of what each recompiled namespace wrote: bytes written per edit rebuild
  181.9 to 92.8 MB, read 446 to 267 MB (medians), writing 2.77 to 2.34 s,
  validation 5.60 to 3.23 s; the cache-writing build with nothing cached
  48.8 to 36.6 s (one each). The edit rebuild's median moves from 30.82 to
  29.15 s, inside the base's spread. Plain 400k has a fifth as many
  namespaces and smaller lists: 119.9 to 111.9 MB written, no change in wall
  time. The saving grows with the program, since each list grew with its
  namespace's position.

### The fix, with compare.bb and at 800k

`compare.bb --light-only --leaf ... --runs 3 --skip-unchanged --max-load 1000
--max-repeats 0` on the re-frame 400k copy, then `--runs 2` on a copy of the
re-frame 800k project (`re-frame-800k-seed42-1000`, leaf
`src/workload/badam.cljs`), one binary after the other (so not alternated
between binaries); every output passed the oracle and the waste check. Raw
results in `research/shadow-credibility/edit-1000-8eefcf09/kit/`.

| Project | Binary | Clean build | Rebuild after a one-file edit | Load before the edits |
| --- | --- | --- | --- | --- |
| re-frame 400k | `8eefcf09` | 18.95 s (18.12-20.06), 2.436 GB | 26.60 s (26.40-28.98), 25.61 s user, 2.523 GB | 4.2-5.2 |
| re-frame 400k | `0a56a693` | 19.74 s (17.87-19.78), 2.429 GB | 24.91 s (23.41-33.34), 23.68 s user, 2.518 GB | 4.6-4.9 |
| re-frame 800k | `0a56a693` | 53.19 s (52.76-53.62), 4.750 GB | 91.25 s (89.33-93.17), 81.84 s user, 4.949 GB | **14.7-22.7** |
| re-frame 800k | `8eefcf09` | 55.30 s (53.23-57.37), 4.769 GB | 76.03 s (72.18-79.87), 72.15 s user, 4.951 GB | 3.5-3.7 |

The fix's 800k edit rebuilds ran while another worker's load peaked (1-minute
load 14.7 to 22.7, against 3.5 to 3.7 for the base's), and are recorded as
found. To separate the binaries from the load, the two ran again alternated
on the same 800k copy, each with its own cache and one untimed fill, two
edit rounds, `--jobs 1 --stats`, load 2.3 to 2.6, the same output hash for
all six builds (`research/shadow-credibility/edit-1000-8eefcf09/probe800/`):

| Binary | Cache-writing build, nothing cached | Edit round 1 | Edit round 2 | Bytes written per edit | Bytes read, rounds 1 and 2 | Validation | Writing | Capture |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `8eefcf09` | 113.30 s, 4.960 GB | 75.33 s | 84.44 s | 560.0 MB | 703, 1,074 MB | 8.3, 12.6 s | 6.7, 7.2 s | 19.0, 19.6 s |
| `0a56a693` | 105.08 s, 4.898 GB | 68.78 s | 66.04 s | 205.0 MB | 347, 552 MB | 4.3, 6.1 s | 4.7, 4.5 s | 19.4, 17.7 s |

At 800k the coarse edges' names were 63% of what an edit rebuild wrote; the
fix saves 6.6 to 18.4 s of the edit rebuild there. The edit rebuild stays
slower than the clean build (about 67 s against 53 s): capture (18 to 19 s,
light-cljs-997), validation that grows with past edits (light-cljs-1003) and
the coarse fallback remain.


Findings, recorded as found:

1. **The rebuild after a one-file edit is slower than a clean build at 400k
   because of the coarse fallback, recompiling with capture on, and cache
   work, in that order of size.** Only the first is a dependency-model
   question; it is put to the user with three remedies
   (docs/compiler-cache.md, "A narrower view for source-macro users"). The
   measured bound for an edit that registers no declaration: 19.46 to 6.66 s
   (plain) and 30.82 to 11.72 s (re-frame), both well under the clean build.
2. **The require-closure rule rules out two of the three views the fallback
   covers, and only in the Pants build.** The V8 build does not enforce it
   (light-cljs-446 is deferred), and neither build restricts the `&env`
   macro map, so narrowing the view to the require closure is not sound in
   the build that ships without a language change there.
3. **`--jobs 8` does not speed up an edit rebuild** while capturing, and
   costs up to about 2.8x the user CPU (light-cljs-461's cause: with capture
   on, every unit enters the macro host).
4. **The coarse edges no longer weigh on artifacts** (`0a56a693`): half the
   bytes written at re-frame 400k and 63% fewer at 800k, where the edit
   rebuild gains 6.6 to 18.4 s, with keys, hits and output unchanged. It is
   still slower than the clean build there (about 67 s against 53 s).
