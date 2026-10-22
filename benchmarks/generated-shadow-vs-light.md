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
