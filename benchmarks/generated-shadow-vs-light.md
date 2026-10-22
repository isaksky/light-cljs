# Generated projects: light-cljs vs shadow-cljs, advanced builds (October 22, 2026)

Projects from [the generator](generated-project/README.md), seed 42, at 5k, 200k
and 800k nonblank lines. Both compilers build at advanced: `light-cljs release
bench` (default `:advanced`) and `clojure -M:shadow release bench` (Closure
advanced). Every output printed the oracle's checksum for input 42 and 3
iterations, and `check-waste.bb --strict` passed for both.

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
3. **gzip is 26-27% worse than shadow at the same raw size.** At 200k light's
   output has 98,232 distinct identifiers against shadow's 26,279: locals get
   program-unique names (`tVY`, `tVZ`, ...) where Closure reuses short names in
   each function scope. Light also emits 9,410 `(function(){...})()` expression
   wrappers and 3,177 calls to the truthiness helper.
4. **Light keeps more unused arities** (86 vs 51 at 200k).
   Since light-cljs-421 light keeps 49 at 200k and 202 of 779 at 800k (shadow
   51 and 215): methods that no call selects of functions without a
   dispatcher, and dispatchers kept only for an unread alias, are gone
   ([details](compiler.md#unused-arities-without-a-dispatcher-and-unread-aliases--october-22-2026)).

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
