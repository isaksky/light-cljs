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
The [October 29 comparison at `0ad2514f`](#re-measured-at-0ad2514f-cached-and-uncached-october-29-2026),
at the end, is the latest, with cached builds and leaf edits for both compilers.

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
