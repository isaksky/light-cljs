# Compiler cache measurements

Numbers for the persistent compiler cache
([design](../docs/compiler-cache.md), "Settled decisions" for the rollout
gates). Every row is Penpot `app.main --optimize none` built by
[penpot.bb](penpot.bb) (`bb benchmarks/penpot.bb OUT main-none`) in a fresh
process, on the Apple M3 Ultra virtual machine (24 vCPUs, 32 GiB) other workers
share. Baseline and candidate runs alternate, with `uptime` load recorded next
to each, and the binaries are immutable copies selected with
`LIGHT_CLJS_BINARY`. `LIGHT_CLJS_EXTRA_ARGS=--stats` adds the report, saved as
`main-none.stats.txt`, with its counters in `results.edn` under `:stats`.

## Cold-build baseline (light-cljs-310, October 18, 2026)

Parent `f68c767` against the `--stats` change, both without `--stats`, four
alternating pairs at 1-minute load 3.5 to 10.6 (a fifth of the machine). Output
is 33,588,607 bytes in every run.

| Build | Wall (s), run 1-4 | Mean wall | Peak RSS (GB), run 1-4 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `f68c767` | 22.74, 23.48, 22.22, 24.06 | 23.13 | 0.994, 0.952, 0.951, 0.969 | 0.967 |
| `--stats` change, flag off | 22.73, 24.10, 23.43, 24.01 | 23.57 | 0.964, 0.992, 0.996, 0.983 | 0.984 |

The difference, +1.9% wall and +1.8% RSS on the means, is within the +3% and
+5% gate and within the run-to-run spread of either binary (1.8 s): without
`--stats` the change reads no clock and adds one counter read per namespace.
Two runs with `--stats` took 22.37 s and 22.17 s at 0.977 and 0.963 GB. An
earlier round under load 40 (another worker's V8 build) gave 36.6 s for both
binaries and is not used.

### Phase split with `--stats`

The second `--stats` run, which is representative of both:

| Counter | Value |
| --- | ---: |
| `time.wall-ms` | 22,125 |
| `time.resolve-parse-ms` | 97 |
| `time.analysis-ms` | 5,753 |
| `time.deferred-units-ms` | 15,471 |
| `time.prepare-application-ms` | 99 |
| `time.emission-ms` | 636 |
| `time.write-ms` | 39 |
| `namespaces` | 922 |
| `macro-calls` | 93,997 |
| `macro-nodes.peak` | 5,234,662 |
| `deferred-units.registered` / `analyzed` / `prepared` | 13,367 each |
| `deferred-units.executed` | 277 |

Analysis is 26% of the wall time and draining deferred units 70%; resolving,
reading and parsing 922 sources is under half a percent, and preparing,
emitting and writing the 33.6 MB output 3.5%. The cache's saving is therefore
bounded by the analysis and drain phases it can skip, about 96% of a cold
build. Every registered deferred unit is analyzed and prepared during its
namespace's drain; 277 of them ran at compile time, for the macros that called
them. The namespaces with the most macro calls are `malli.core` (1,905),
`app.plugins.shape` (1,465), `app.render-wasm.api` (1,315),
`app.plugins.library` (1,206) and `app.common.logic.libraries` (880). The
macro-call count is the same in every run, so a cache hit will show as a
lower count.

## Dependency manifest, cache off (light-cljs-311, October 18, 2026)

Without `--cache-dir` nothing records a manifest. On the cold path what remains
is a `RefCell` check per source macro call and per name resolved outside the
`ns` form, plus a few small vectors per namespace in the loader. The runs were
three alternating pairs of the parent `d6439d4` against the change, both
release builds, at 1-minute load 7.6 to 11.2, with other workers active. Output
is 33,588,607 bytes in every run.

| Build | Wall (s), run 1-3 | Mean wall | Peak RSS (GB), run 1-3 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `d6439d4` | 25.40, 24.48, 25.03 | 24.97 | 1.042, 1.031, 1.034 | 1.036 |
| Manifest change | 24.96, 22.02, 24.22 | 23.73 | 1.051, 0.960, 1.027 | 1.013 |

On the means, wall is -5.0% and RSS -2.3%, within the +3% and +5% gate.
Both differences are inside the run-to-run spread. The manifest's own size and
cost on Penpot are in [docs/compiler-cache.md](../docs/compiler-cache.md),
"Conservative keys and invalidation".

## Watch startup (light-cljs-326, October 18, 2026)

`--watch` starts in three parts: the startup build analyzes the program with
V8 (`time.resolve-parse-ms`, `time.analysis-ms`), writes the dev build
(`time.emission-ms`, `time.write-ms`), then a QuickJS `DevSession` compiles
the same program again as `(require 'entry)` (`time.session-load-ms`), the
program later steps compile against. [watch_startup.bb](watch_startup.bb)
(`bb benchmarks/watch_startup.bb OUT [RUNS] [NAMESPACES]`) starts the release
binary with `--stats` in a fresh process, stops it at its "watching" line, and
prints the counters, the startup time the watch reports and a digest of the
dev build.

Penpot `app.main` does not start under `--watch`: the watch takes none of the
`--macro-*` flags, and at the default limits (1,024 expansions per
compilation, against Penpot's 93,997 macro calls) its startup build stops with
`unavailable definition: cuerdas.core/repeat` while expanding
`cuerdas.core/unsafe-concat`, as `--compile --reloadable` does without the
flags (light-cljs-349, which found that warning mode causes the error, with
or without the flags: "Macro limits under watch"). The large fixture is therefore the script's generated
program: NAMESPACES namespaces, every tenth a macro namespace with three
`defmacro`s, the others each a 20-map data table, `fn*` functions and a `defn`
calling a macro, each requiring up to two earlier namespaces. At 300 (301 with
the entry) it makes 962 macro calls, the most that fit under the watch's
1,024 (sources 1.2 MB).

Release binary at `7929289`, three runs each (five for the example) at
1-minute load 2.1 to 2.3; times in ms, means.

| Program | Namespaces | Macro calls | V8 build (parse + analysis) | Emission + write | Session load | Startup | Session share |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `examples/dev` | 4 | 16 | 42 | 1 | 32 | 75 | 42% |
| generated 75 | 76 | 243 | 85 | 27 | 329 | 442 | 75% |
| generated 150 | 151 | 482 | 128 | 77 | 1,042 | 1,250 | 83% |
| generated 300 | 301 | 962 | 239 | 254 | 4,053 | 4,550 | 89% |

The session load grows quadratically (3.2x and 3.9x per doubling) while the
V8 build stays near linear; the dev build's emission is also superlinear
(19, 58 and 210 ms). A `sample` profile of the session load attributes it to
per-form work over the whole retained program: each form's analysis clones
the previous program and its resolution snapshot retains every global's
metadata again (light-cljs-348), with QuickJS macro execution a smaller
share. An earlier round at load 44 gave 6.7 s startup for the 301-namespace
program, of which 6.0 s was the session load.

### The single startup compilation evaluated

- **The session's units write the dev build** (startup compiles once, in
  QuickJS). It would save the V8 build, at most 11% of startup on the large
  fixture (493 of 4,550 ms), but the output is not today's: after a save that
  changes nothing in `app.state` and `app.view` of `examples/dev`, the units
  the step writes from the session differ from the startup build's
  (`app.state.js` 2,394 against 2,400 bytes, `app.view.js` 3,492 against
  3,551, its map 590 against 530). The session's units set the namespace
  with `createNs` and run each form in a scope of its own, where the build
  emits the namespace as one unit with `theNs`. Rejected: the dev build must
  stay byte-identical.
- **The build's results seed the session** (startup compiles once, and the
  session starts from it). This is where the time is: at most 89% of startup
  on the large fixture. Analysis does not depend on the engine: the
  QuickJS-only binary's `--compile --reloadable` build of the 301-namespace
  program is byte-identical to the V8 binary's and takes 0.68 to 0.73 s
  (analysis 0.43 s) against 0.48 to 0.51 s (two rounds, at load 2 and 9), so
  one whole-program build in QuickJS, the session's own engine, could write
  the dev build and keep its macro heap for the session without crossing
  engines, for a startup near 0.7 s instead of 4.55 s. What the session
  needs from it is not in the build's results, however: its per-namespace
  directive contexts, the macro host's interactive imports, renames and
  namespace metadata, the namespaces
  it registered with the runtime and how many definitions each had, the
  loaded set and the source provider's record of each namespace's file. The
  session's own load builds these, and the whole-program analysis takes a
  different path through the analyzer (`NamespaceAnalysis` rather than
  `analyze_interactive_form` against the retained program). Installing them
  for a namespace compiled elsewhere is the work of light-cljs-327 (with
  light-cljs-315 to 318 underneath), which validates first edits after such
  an install. Not adopted here: no version could be shown to keep first
  edits the same within this pellet.

The startup path is unchanged. The cost the measurements point at is the
quadratic session load, which light-cljs-348 addresses directly.

## Watch startup from the cache (light-cljs-327, October 18, 2026)

`--watch --cache-dir DIR` compiles its startup build once, in the session's
QuickJS host, with the persistent cache, and starts the session from it
(docs/compiler-cache.md, "Development sessions and engine boundaries").
`bb benchmarks/watch_startup.bb [--cache] OUT RUNS 300` on the generated
301-namespace program, release binary of this change, at 1-minute load 4.6
to 5.0 (two other workers active); with `--cache` the first run fills an
empty cache and the others start warm. Times in ms.

| Start | Runs | Cache hits | Macro calls | Analysis | Emission + write | Session | Startup |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| without `--cache-dir` (V8 build, then the session's own load) | 3 | | 962 | 232 | 242 | 4,113 | 4,602 |
| `--cache-dir`, filling an empty cache | 1 | 0 | 962 | 461 | 234 | 52 | 954 |
| `--cache-dir`, warm | 3 | 301 | 0 | 1.6 | 180 | 48 | 320 |

The dev build's digest is the same in all seven runs. The populating start
is 21% of the uncached one: the session no longer loads the program form
by form (the quadratic part, light-cljs-348), and analysis in QuickJS costs
twice V8's. A warm start is 7% of an uncached one; what is left is writing
the dev build, installing the session's state (taking the program and
replaying 301 `ns` forms, the session column) and reading, validating and
installing the 301 artifacts. Steps are unchanged: saving a newline into
`bench.n155` ten times takes 24.0 to 35.2 ms (median 28.5) after an
uncached start and 23.9 to 34.1 ms (median 30.0) after a warm one, whose
first step is 133 ms because the session's macro engine starts with it;
`bench.macros150` recompiles in 271 against 347 ms the first time (a
second save fails in `bench.n159` after either start alike). Without
`--cache-dir` the startup path is the one
light-cljs-326 measured; `--compile` is unchanged, so no Penpot rerun.
Replaying every top-level directive rather than the `ns` form alone (found
by syntax, after review) leaves the session column at 51 to 54 ms: at load
8 to 9, filling took 1,097 ms and warm starts 334 to 372 ms.

## Macro limits under watch (light-cljs-349, October 18, 2026)

`--watch` now takes the `--macro-*` flags, and its macro hosts take
`--compile`'s stub packages. Penpot `app.main` with the limits
[penpot.bb](penpot.bb) passes still does not start, unmodified, under
`--watch` or `--compile --reloadable`. The first failure is warning mode,
not a limit: `--warnings --compile --esm` fails the same way in 0.13 s, while
`--compile --esm --optimize none` builds in 24.2 s (0.92 GB):

    vendor/cuerdas/core.cljc:293:15: macro cuerdas.core/unsafe-concat: ...
    unavailable definition: cuerdas.core/repeat

Warning mode drains deferred units after every top-level form, so the macro
runs before `repeat`, declared at line 53 and defined at line 375, has a unit
(light-cljs-360). With `repeat` moved above the macros in an uncommitted
overlay of `vendor/` (symbolic links and one edited file), warning mode
builds the whole program (`--warnings --compile --esm`, 28.3 s, 0.96 GB).
Next, `abnf.cljc` in instaparse requires `[goog.string.format]`, which no
root provides: `--compile` takes it as a host namespace, but the session
reports `missing namespace goog.string.format`, both when it loads the
program itself and when it replays the `ns` forms of a cache-seeded start
(light-cljs-361). Stubs for packages were the third failure: a macro of
`app.util.object` demands the transit extensions, whose type comes from the
`transit-js` package, and the watch's hosts had no stubs; this change fixes
that. With an empty `(ns goog.string.format)` added to the overlay, `--watch`
starts. These are one run each of the release binary of this change, with
`--stats`, stopped at the watching line; the peak RSS was sampled every
0.5 s. Load was 5.6 to 8.9, with two other workers active. Times in s.

| Start | Analysis | Emission + write | Session | Startup | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| without `--cache-dir` (V8 build, then the session's own load) | 22.5 | 18.3 | 707.4 | 748.4 | 15.5 GB |
| `--cache-dir`, filling an empty cache (923 misses) | 103.1 | 19.7 | 2.2 | 128.1 | 0.86 GB |
| `--cache-dir`, warm (923 hits, 0 macro calls) | 0.02 | 6.9 | 2.2 | 10.6 | 0.75 GB |

- **Without the cache**, the session's form-by-form load of 923 namespaces
  takes 94% of startup, and its peak memory is 16 times the build's. This
  is the quadratic per-form copy of the retained program (light-cljs-348).
- **Filling** runs the 93,997 macro calls in QuickJS, the session's engine:
  analysis takes 103 s, against 22.5 s in V8. Capture adds 1.4 s and the
  store writes 105 MB.
- **Warm** decoding takes 0.49 s and installing 0.22 s. The rest is the dev
  build's emission of 923 units with source maps, and the session's replay
  of the directives. Emission takes 6.6 s here and 18 to 20 s in the two
  cold runs, at load 8.9 against 5.6.

## Host `goog.*` namespaces under watch (light-cljs-361, October 18, 2026)

The session now binds a `goog.*` require that no source root provides as a
host namespace, as `--compile` does, so Penpot `app.main` needs no
`goog.string.format` stand-in: 922 namespaces where the stand-in made 923.
light-cljs-360 is not done yet, so these runs keep the one-file overlay of
`vendor/` that moves `cuerdas.core/repeat` above its macros. Nothing else
is changed. One run each of the release binary at `663aa49`, with the
penpot.bb limits and `--stats`, stopped at the watching line. Peak RSS was
sampled every 0.5 s. Load was 3.9 to 7.4, with two other workers active.
Times in s.

| Start | Analysis | Emission + write | Session | Startup | Peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| without `--cache-dir` (V8 build, then the session's own load) | 24.5 | 21.3 | 742.2 | 788.1 | 14.0 GB |
| `--cache-dir`, filling an empty cache (922 misses) | 104.2 | 21.3 | 2.2 | 131.0 | 0.78 GB |
| `--cache-dir`, warm (922 hits, 0 macro calls) | 0.02 | 7.2 | 2.1 | 10.8 | 0.65 GB |

These match light-cljs-349's runs with the stand-in, within load noise. The
session's own load is still the quadratic per-form copy of light-cljs-348.

## Warning mode in Penpot (light-cljs-360, October 18, 2026)

In warning mode a namespace that defines macros of its own now drains its
deferred units at its end, as without warnings, so `cuerdas.core/unsafe-concat`
reaches `repeat`. Penpot `app.main` builds unmodified with the limits
[penpot.bb](penpot.bb) passes. Release binary of this change, load 3 to 6
(15 for the third reloadable run) with two other workers active; one run each
unless noted:

| Build | Wall | Peak RSS | Warnings |
| --- | ---: | ---: | ---: |
| `--compile --reloadable --out-dir` (three runs) | 46.3, 46.8, 47.4 s | 0.90, 0.92, 0.95 GB | 18 |
| `--warnings --compile --esm --optimize none` | 24.8 s | 0.94 GB | 18 |

The dev build writes 1,848 files, a unit and a source map for each of 923
namespaces plus the runtime, client, manifest and index. An earlier run of the
same build at load 3.2 took 39.1 s (0.90 GB). Without warnings nothing changes:
`main-none` output is byte-identical to the parent commit's, and alternating
three runs each with the parent's binary at load 3.6 to 5.8 gave medians of
25.68 s and 0.947 GB against 24.89 s and 0.942 GB (+3.2% wall, +0.5% RSS; the
means are 25.35 s against 25.03 s, +1.3%). With light-cljs-361 rebased in,
`--watch --cache-dir` filling an empty cache now starts unmodified, with no
overlay: 922 namespaces built and watching after 130.6 s, 0.81 GB peak RSS
(one run, load about 10).

## Session load (light-cljs-348, October 18, 2026)

The session's form-by-form load of the program (`time.session-load-ms`,
without `--cache-dir`) was quadratic. Instrumenting it on the generated
301-namespace program (4.6 s) found five terms that grew with the retained
program at every form, where light-cljs-326's profile named the first two:

- every form compacted the whole retained program again
  (`interactive_resolution_snapshot`, 1.4 s), and copied it, with its global
  names, into the analysis (0.2 s);
- every macro call's `&env` listed every namespace the session had entered
  as a require, so every macro call encoded, decoded and
  walked a map as large as the session (1.0 s importing `&env`, 0.8 s in the
  macro calls, 0.3 s building the list);
- reading each form gathered every global into the reader's bindings (0.15
  s);
- each form scanned every retained expression for arities and source-map
  names, and every global and namespace for the unit's registration (0.3 s).

Each now costs what the form adds (docs/dev-workflow.md, "As built
(light-cljs-348)"). Release binaries of master `b04e99a` (before) and this
change (after), alternating, three runs each with
`bb benchmarks/watch_startup.bb OUT 1 N`, at 1-minute load 8.8 to 9.3 (two
other workers active); means in ms:

| Program | Session load before | after | Startup before | after |
| --- | ---: | ---: | ---: | ---: |
| generated 75 (76 namespaces) | 360 | 136 | 492 | 269 |
| generated 150 (151) | 1,173 | 234 | 1,408 | 473 |
| generated 300 (301) | 4,618 | 464 | 5,192 | 1,039 |

Per doubling the session load grew 3.3x and 3.9x before and 1.7x and 2.0x
after. The dev build's digest is the same before and after at each size,
and so are the session's units: the 1,472 units of the 301-namespace load,
written out during the change, are byte-identical. What grows faster than
the program now is the V8 dev build's emission and write (30, 85 and 304
ms), which light-cljs-326 noted.

Penpot `app.main` still stops in the uncached session load at
`cuerdas.core/unsafe-concat` (`unavailable definition: cuerdas.core/repeat`),
before and after this change, so these runs use light-cljs-361's one-file
overlay of `vendor/` that defines `repeat` above the macros, with the
penpot.bb limits and `--stats`: one run of this change at load 8 to 9,
against light-cljs-361's run of the same overlay at `663aa49` (load 3.9 to
7.4).

| Penpot `app.main`, 922 namespaces | Session load | Startup | Peak RSS |
| --- | ---: | ---: | ---: |
| before (light-cljs-361) | 742.2 s | 788.1 s | 14.0 GB |
| after | 115.4 s | 163.6 s | 1.69 GB |

The session load is still 71% of startup (the V8 build and the dev build's
write take 48 s), 1.2 ms per macro call on average; it was not profiled on
Penpot.

The cold build is unchanged: Penpot `main-none` through
[penpot.bb](penpot.bb), three alternating runs each of the parent `b04e99a`
and this change at load 5.8 to 10, gave the same output (same SHA-256) in
26.25, 31.04, 27.05 s against 26.81, 27.54, 26.83 s (medians 27.05 and
26.83 s, -0.8%) and 0.906, 0.919, 0.923 GB against 0.922, 0.954, 0.924 GB
peak RSS (medians 0.919 and 0.924 GB, +0.5%).

## Uncached watch of unmodified Penpot (light-cljs-364, October 18, 2026)

The session's form-by-form load now lets a macro call a definition its file
makes after the macro's use: it registers the compile-time definitions of
the rest of the file ahead, before a macro use while the file has pending
declarations, or after such a call fails, and then analyzes the form again
(docs/dev-workflow.md, "As built (light-cljs-364)"). Penpot `app.main`, with
no overlay of `vendor/`, starts under `--watch` without `--cache-dir`.
Release binary of this change, with the penpot.bb limits and `--stats`,
stopped at the watching line, with peak RSS sampled every 0.5 s and two
other workers active:

| Run | Analysis | Emission + write | Session | Startup | Peak RSS | Load |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| final | 22.3 s | 20.3 s | 111.4 s | 157.7 s | 1.70 GB | 4.3 to 5.4 |
| draft, only after a failure | 24.1 s | 22.7 s | 116.7 s | 167.6 s | 1.70 GB | 8.7 to 9.4 |
| draft, analyzing the rest of the file ahead | 23.6 s | 22.4 s | 113.6 s | 163.4 s | 1.65 GB | 5.4 to 10.2 |
| draft, the same with strict analysis | 23.3 s | 23.3 s | 125.4 s | 175.9 s | 1.69 GB | 5.3 to 14.2 |

The 922 namespaces match light-cljs-348's run with the one-file overlay
(115.4 s session load, 163.6 s startup, 1.69 GB): defining ahead costs
nothing measurable. The last two rows analyzed every later form of the
file ahead and completed its units, which made later macros visible early
and expanded macro uses twice. The final change registers only the later
definitions, leaves their units unanalyzed until a macro calls one, reads
each form once, and registers them before the macro use when the file
declared the names, so the macro runs once; the second row registered
them only after the use failed.

## Watch invalidation (light-cljs-325, October 18, 2026)

`--watch` now records the startup build's dependency manifest and revises it
after each step (`DependencyManifest::revise`), and a step asks the policy
evaluator for the always-compile set. Release binaries of master `f9016ff`
(baseline) and this change (candidate), alternating, on the generated
301-namespace program of [watch_startup.bb](watch_startup.bb), at 1-minute
load 7.5 to 13.6 (other workers were testing); times in ms.

| | Baseline | Candidate |
| --- | ---: | ---: |
| Startup, mean of 6 (`bb benchmarks/watch_startup.bb OUT 3 300`, twice) | 5,167 | 5,151 |
| Session load, mean of 6 | 4,568 | 4,542 |
| Step saving a newline into `bench.n155` (1 namespace), median of 10 | 25.5 | 26.1 |
| Step saving `bench.macros150`, first (10 namespaces), 2 runs | 291.1, 292.3 | 294.2, 295.0 |
| Step saving `bench.macros150`, third to fifth, median of 6 | 2.2 | 2.9 |

The dev build is byte-identical (same digest). Recording the manifest in
the startup build costs nothing measurable at this size; revising it costs
about 0.6 ms a step on 301 namespaces, most of them coarse (each ran a
source macro, so each has an edge to every earlier namespace). The steps
recompile the same namespaces as before on this program, which has no
helper edges, resources or `^:dev/always` namespaces.

## Namespace artifact capture (light-cljs-315, October 18, 2026)

Capture runs only when a compilation asks for artifacts
(`analyze_namespace_entry_with_artifacts`); until the store exists,
`LIGHT_CLJS_CAPTURE_ARTIFACTS` turns it on for `--compile --entry`. `1`
captures each namespace and drops the artifact once it is handed over, as a
store that writes it and lets it go would; `keep` holds every artifact until
the process exits, as an in-memory map would. Three alternating rounds of the
parent `603c1a8`, the change with capture off, `1` and `keep`, all release
builds, at 1-minute load 1.4 to 2.3. Output is 33,588,607 bytes in every run.

| Build | Wall (s), run 1-3 | Mean wall | Peak RSS (GB), run 1-3 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `603c1a8` | 21.14, 21.13, 21.16 | 21.14 | 0.955, 0.963, 0.973 | 0.964 |
| Change, capture off | 21.14, 21.18, 21.11 | 21.14 | 0.958, 0.970, 0.952 | 0.960 |
| Capture, `1` | 22.14, 22.11, 22.12 | 22.12 | 0.966, 0.965, 0.983 | 0.972 |
| Capture, `keep` | 22.18, 22.46, 22.19 | 22.28 | 1.185, 1.236, 1.221 | 1.214 |

With capture off the change is 0.0% wall and -0.4% RSS against its parent,
within the +3% and +5% gate. With capture on, peak RSS is +1.2% against
capture off, within the +10% the pellet allows, and wall is +4.6%. Keeping
every artifact costs +26.4% RSS: the 922 artifacts are about 183 MB by
`cache.capture.bytes`, so the store must write and release each one rather
than hold them for the build. All 922 namespaces capture, none refused. Three
`--stats` runs with capture (22.11, 22.11, 22.06 s) report
`cache.capture-ms` 1,029, 1,037 and 1,026, 4.7% of the wall time;
`time.analysis-ms` (about 5,390) and `time.deferred-units-ms` (about 14,780)
exclude it.

## Installing artifacts, cache off (light-cljs-316, October 18, 2026)

Penpot `app.main --optimize none`, without installing anything: the entry
loop asks no lookup and a macro's function is still evaluated where it is
declared. Three alternating rounds of the parent `44f0ee4` and the change,
release builds, at 1-minute load 2.0 to 3.4. Output is 33,588,607 bytes and
byte-identical in every run.

| Build | Wall (s), run 1-3 | Median wall | Peak RSS (GB), run 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `44f0ee4` | 21.99, 21.74, 21.61 | 21.74 | 0.978, 0.962, 0.960 | 0.962 |
| Change | 21.62, 21.64, 21.74 | 21.64 | 0.955, 0.956, 0.961 | 0.956 |

The change is -0.5% wall and -0.6% RSS against its parent, within the +3%
and +5% gate. There is no installing build of Penpot yet: the store and its
lookup are light-cljs-319 to 322.

## Generated names, cache off (light-cljs-317, October 18, 2026)

Penpot `app.main --optimize none`, without installing anything. Three
alternating rounds of the parent `eda504b` and the change, release builds,
at 1-minute load 1.4 to 3.7. Output is 33,588,607 bytes and byte-identical
in every run: the `gensym` hooks are only in the compile-time evaluators'
runtime.

| Build | Wall (s), run 1-3 | Median wall | Peak RSS (GB), run 1-3 | Median RSS |
| --- | --- | ---: | --- | ---: |
| Parent `eda504b` | 21.42, 21.28, 21.27 | 21.28 | 0.948, 0.945, 0.957 | 0.948 |
| Change | 21.31, 21.39, 21.28 | 21.31 | 0.949, 0.981, 0.966 | 0.966 |

The change is +0.1% wall and +1.9% RSS against its parent, within the +3%
and +5% gate. With capture on (`LIGHT_CLJS_CAPTURE_ARTIFACTS=1 --stats`, one
run each), the generated names add 581,458 bytes to `cache.capture.bytes`
(184,742,484 to 185,323,942 for the 922 namespaces), and reading the gensym
counter and log once per namespace moves `cache.capture-ms` from 1,058 to
1,103 (wall 22.18 and 22.29 s).

## light.build services, cache off (light-cljs-312, October 18, 2026)

Penpot `app.main --optimize none`, which calls no `light.build` service. On
the cold path the change adds one site update per namespace (its name and
file) and a JavaScript wrapper around each `__lightCompileUnit` call and unit
initializer. Three alternating rounds of the parent `ebeb14e` and the change,
release builds, at 1-minute load 6.1 to 10.5 with other workers active.
Output is 33,588,607 bytes and byte-identical in every run.

| Build | Wall (s), run 1-3 | Mean wall | Peak RSS (GB), run 1-3 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `ebeb14e` | 26.54, 25.39, 26.20 | 26.04 | 1.002, 0.993, 1.039 | 1.011 |
| Change | 26.51, 25.48, 26.28 | 26.09 | 1.004, 0.969, 1.038 | 1.004 |

On the means, wall is +0.2% and RSS -0.8%, within the +3% and +5% gate and
inside the run-to-run spread of either binary.

## Binary artifacts (light-cljs-320, October 18, 2026)

`LIGHT_CLJS_CAPTURE_ARTIFACTS=encode` captures each namespace as `1` does,
then encodes it with `artifact_codec::encode` (zero keys, no dependency
fields) and decodes it again with the default limits; `--stats` adds the
`cache.codec.*` counters, `cache.encode-ms` and `cache.decode-ms`. Decoding
includes checking every section's SHA-256, charging the decoded-allocation
budget and `NamespaceArtifact::validate`, which includes the IR cycle and
depth check. Release build of the change, three runs at 1-minute load 3.0
to 3.9 (an earlier round at load 12.6 to 15.0 gave encode 493 to 525 ms and
decode 434 to 466 ms before the budget and cycle check were added). All 922
artifacts encode and decode again (none failed), with the same sizes in
every run. The longest expression chain of any artifact
(`cache.codec.ir-depth`) is 88, against the default limit of 4,096.

| Measure | Value |
| --- | ---: |
| Artifacts | 922 |
| Encoded bytes, total | 93,801,445 |
| Source bytes of those namespaces | 11,343,454 |
| Encoded / source | 8.3x |
| Largest artifact (`app.common.fonts-data`, source 1,558,113 bytes) | 8,991,968 |
| Mean artifact | 101,737 |
| `cache.capture.bytes` (in-memory estimate, for comparison) | 185,323,942 |

Bytes by section, summed over the 922 artifacts:

| Section | Bytes | Share |
| --- | ---: | ---: |
| Header and manifest | 706,033 | 0.8% |
| Strings | 75,296,204 | 80.3% |
| Declarations | 397,664 | 0.4% |
| Semantic IR | 16,157,912 | 17.2% |
| Deferred units | 886,673 | 0.9% |
| Diagnostics | 308,066 | 0.3% |
| Names and accounting | 48,893 | 0.1% |

The artifacts exceed five times the source. The string table is the cause.
The 13,367 deferred units' prepared compile-time JavaScript is 31.3 MB of
ASCII text, which is 62.5 MB in UTF-16, and the IR's own strings are 5.8 M
code units. A temporary count, not committed, found only 74,548 bytes of
that code duplicated between units of the same namespace, so interning does
not reduce it. `app.common.fonts-data` is one data `def` whose prepared
code alone is 2.7 MB. Compression is out of scope here. The open question
is recorded in docs/compiler-cache.md.

| Run | `cache.encode-ms` | `cache.decode-ms` | Wall (s) | Peak RSS (GB) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 464.9 | 443.0 | 26.17 | 0.938 |
| 2 | 468.1 | 448.7 | 26.15 | 0.956 |
| 3 | 524.0 | 453.9 | 26.15 | 0.975 |

Encoding takes about 0.47 s and decoding 0.45 s for the 93.8 MB, about
200 MB/s each way. A cache-off run of the same binary right after, at load
3.1, took 22.66 s (`time.wall-ms` 22,616, peak RSS 0.982 GB). Of that,
analysis (5,878 ms) and draining deferred units (15,853 ms) are what a warm
build with every namespace a hit skips, which leaves about 0.89 s:
resolving and parsing, preparing, emitting, writing and process start. A
warm build is therefore expected to cost about 0.89 s plus decoding
(0.45 s) plus installing, which is not measured on Penpot yet. Decoding is
34% of that expected cost without installation. If installing costs what
capture does (about 1.1 s), decoding is about 18%. Either way the expected
warm build is under 11% of the cold build. The cold build without the
variable runs no codec code.

## Warnings and logical limits, cache off (light-cljs-318, October 18, 2026)

Penpot `app.main --optimize none`, without installing anything. Three
alternating rounds of the parent `9949f63` and the change, release builds,
at 1-minute load 1.4 to 2.9. Output is 33,588,607 bytes and byte-identical:
the `encode` hooks that record what each encoding charged are only in the
compile-time evaluators' runtime, and the accounting is measured only while
capturing.

| Build | Wall (s), run 1-3 | Mean wall | Peak RSS (GB), run 1-3 | Mean RSS |
| --- | --- | ---: | --- | ---: |
| Parent `9949f63` | 23.53, 23.33, 23.55 | 23.47 | 0.985, 0.960, 0.972 | 0.972 |
| Change | 22.62, 22.79, 22.73 | 22.71 | 0.942, 0.961, 0.947 | 0.950 |

The change is -3.2% wall and -2.3% RSS against its parent, within the +3%
and +5% gate; it does no more work cache-off, so the difference is noise.
An earlier round, before the review fixes and against `ebeb14e` at load 3.1
to 8.7, gave 23.34 and 23.44 s (+0.4%) and 0.973 and 0.962 GB. With capture on (`LIGHT_CLJS_CAPTURE_ARTIFACTS=1 --stats`, two
alternating runs each at load 2.4 to 3.2, in that earlier round), reading the encode peak after
each of the 93,997 expansions moves `cache.capture-ms` from 1,203 and 1,257
to 1,216 and 1,388 and the wall from 23.75 and 25.12 s to 24.20 and
26.76 s; the accounting and policy add 44,256 bytes to `cache.capture.bytes`
(185,323,942 to 185,368,198). All 922 namespaces are still captured.

## Namespace keys (light-cljs-319, October 18, 2026)

Three alternating pairs of the same release binary: cache off, then
`LIGHT_CLJS_CAPTURE_ARTIFACTS=1 --stats`, which also records the dependency
manifest, computes all 922 front-end keys from the 11,343,454 bytes of source
and the resolution records, and re-validates every namespace's inputs by
repeating its lookups and re-reading its source and resources, as a store
does before publishing. Load was 9.7 to 13.2. Output is 33,588,607 bytes in
every run, and no run found an input changed (`cache.keys.changed` 0).

| Run | Cache off: wall (s), RSS (GB) | `cache.keys-ms` | `cache.revalidate-ms` | Keys run: wall (s), RSS (GB) |
| --- | --- | ---: | ---: | --- |
| 1 | 25.18, 1.053 | 57.2 | 106.2 | 26.20, 1.039 |
| 2 | 25.03, 0.989 | 56.7 | 105.9 | 26.25, 1.017 |
| 3 | 24.93, 1.068 | 53.4 | 102.4 | 25.73, 1.015 |

Keying takes 56 ms on average, 0.22% of the 25.05 s cache-off wall time, and
re-validation 105 ms, 0.42%; together 0.64%, under the 2% budget. The rest of
the keys run's extra second is capture (`cache.capture-ms` about 1.34 s),
measured under light-cljs-315. Without the variable nothing computes a key:
the cold path is unchanged apart from `light.build` resource reads hashing
their text with SHA-256 instead of FNV-1a.

## `--compile --cache-dir` (light-cljs-322, October 18, 2026)

Penpot `app.main --optimize none` through `benchmarks/penpot.bb`, release
binaries: the parent `f9016ff` (baseline) and this change, alternating in
rounds of baseline cache-off, change cache-off, change populating an empty
`--cache-dir`, then change warm on that directory with nothing edited. Every
run had `--stats`. Rounds 1 and 2 ran at load 1.6 to 2.2; round 3 started at
load 1.7 and rose to 11 when another worker began a build, so it is shown
but not used.

| Run | Round 1 | Round 2 | Round 3 (load 11) |
| --- | --- | --- | --- |
| Baseline, cache off: wall (s), RSS (GB) | 21.84, 0.958 | 21.79, 0.937 | 22.31, 0.942 |
| Change, cache off | 21.80, 0.952 | 21.84, 0.975 | 33.99, 0.998 |
| Change, populating | 24.52, 0.960 | 24.76, 0.982 | 28.57, 1.023 |
| Change, warm | 1.83, 0.804 | 1.75, 0.798 | 1.96, 0.805 |

- **Cache off** is +0.0% wall (21.82 against 21.82 s) and +1.7% RSS
  (0.964 against 0.948 GB) on the means of rounds 1 and 2, within the +3%
  and +5% gate. Without `--cache-dir` the compilation takes the same path
  as before: nothing records a manifest, captures or reads a store.
- **Populating** is +12.9% wall over cache off (24.64 s) and +0.8% RSS:
  every namespace misses (`cache.miss.no-artifact` 922) and is published
  (`cache.published` 922, `cache.bytes-written` 104,922,801). Capture costs
  1.12 s (`cache.capture-ms`) and encoding and writing 1.32 to 1.37 s
  (`cache.write-ms`), so the populating gate of +10% for the default flip
  is missed by about 3 points; that is light-cljs-328's to weigh, and the
  write time is the place to look.
- **Warm, unchanged**: `cache.hits` 922, `cache.misses` 0, `macro-calls` 0,
  output byte for byte the cache-off build's (33,588,607 bytes), 1.79 s on
  average, 8.2% of the cache-off wall time (the gate is 50%), and 0.80 GB
  peak RSS. Of that, reading and checking manifests and keys takes about
  0.50 s (`cache.validate-ms`), decoding 0.48 s and installing 0.18 s.
- **Disk**: 922 artifacts, 105 MB of objects, 108 MB with the indexes.

## Reader macros after the end drain, cache off (light-cljs-362, October 18, 2026)

Without warnings a namespace's reader context now records the macros it
defines after its last deferred unit, as warning mode did since light-cljs-360.
Penpot `main-none` output changes only in the `bindings` of 18
`registerReaderNamespace` calls (36 changed lines, +2,493 bytes to 33,627,083),
each only adding names: `cuerdas.core` gains `ffmt`, `app.common.logging` its
level macros, `clojure.spec.alpha` and `cljs.spec.alpha` `and assert coll-of def
fdef keys map-of nilable or`, and so on. Alternating three runs each with the
parent's binary at load 9 to 12 with two other workers active: medians 26.77 s
and 0.959 GB against 25.92 s and 0.964 GB (+3.3% wall, -0.5% RSS; the means are
26.70 s against 26.67 s, +0.1%). The change adds one macro-name lookup per
namespace that defines macros.

## Special forms in the runtime syntax quote, cache off (light-cljs-368, October 18, 2026)

The runtime reader's syntax quote now leaves the compiler reader's special forms
unqualified before the namespace's bindings, excludes and refers. Penpot
`main-none` output changes only in the runtime's `syntaxQuote` (three hunks,
+182 bytes to 33,627,652). A probe compiled from the Penpot sources reads "`def"
and "`catch" in `clojure.spec.alpha` and `cljs.spec.alpha` as `(quote def)` and
`(quote catch)` (before: `(quote clojure.spec.alpha/def)` and so on), while
"`fdef" still reads `(quote clojure.spec.alpha/fdef)`. Alternating three runs each
with the parent's binary at load 12 to 16 with two other workers active: medians
25.96 s and 0.973 GB against 28.23 s and 0.964 GB (-8.0% wall, +0.9% RSS), which
is load noise for a change outside compilation.

## Penpot benchmark and the default flip (light-cljs-328, October 19, 2026)

[penpot_cache.bb](penpot_cache.bb) runs the scenarios below in one process
each, with penpot.bb's limits, `--optimize none` (or the advanced job's
flags) and `--stats`, on the release binary of master `c3f33ad` (an
immutable copy), three rounds in a row with no other worker on the machine
(1-minute load 1.9 to 3.9). Every edit scenario starts from an APFS clone of
the round's warm directory, and the port is restored after each; it is
unchanged at the end. Wall and peak RSS are the three runs, then their mean
and its ratio to the cache-off mean (the advanced rows to the advanced
cache-off mean); CPU is user plus system, mean.

| Scenario | Wall (s), runs | Mean | Of off | CPU (s) | Peak RSS (GB), mean | Hits / misses | Macro calls | Disk |
| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| Cache off | 23.61, 23.17, 23.36 | 23.38 | 100% | 28.6 | 0.887 | | 93,997 | |
| Populating an empty directory | 25.93, 25.49, 25.59 | 25.67 | 109.8% | 30.9 | 0.900 | 0 / 922 | 93,997 | 112 MB |
| Warm, nothing edited | 1.75, 1.74, 1.74 | 1.74 | 7.5% | 1.7 | 0.816 | 922 / 0 | 0 | 112 MB |
| Leaf edit: `app.main.ui.workspace.palette` (one requirer) | 7.43, 7.40, 7.33 | 7.39 | 31.6% | 8.4 | 0.885 | 769 / 153 | 14,218 | 130 MB |
| Macro and helper edit: `app.common.exceptions` (41 requirers) | 25.59, 25.33, 25.27 | 25.40 | 108.6% | 30.2 | 0.897 | 163 / 759 | 83,691 | 201 MB |
| Resource edit (fixture, below) | 12.84, 12.88, 12.74 | 12.82 | 54.8% | 15.7 | 0.919 | 632 / 293 | 27,083 | 253 MB |
| A new `--config` value | 27.07, 26.69, 26.78 | 26.85 | 114.8% | 31.8 | 0.897 | 0 / 922 | 93,997 | 221 MB |
| Source resolution: a new `app/common/geom/line.cljs` shadowing `line.cljc` | 21.17, 21.11, 21.10 | 21.13 | 90.4% | 25.0 | 0.934 | 433 / 489 | 56,257 | 173 MB |
| `--optimize advanced`, cache off | 29.42, 29.70, 29.61 | 29.58 | 100% | 34.8 | 1.084 | | 93,532 | |
| `--optimize advanced`, populating | 32.24, 32.33, 31.88 | 32.15 | 108.7% | 37.5 | 1.078 | 0 / 922 | 93,532 | 111 MB |
| `--optimize advanced`, warm | 4.07, 4.01, 4.03 | 4.04 | 13.6% | 4.0 | 0.955 | 922 / 0 | 0 | 111 MB |

The advanced rows come from three earlier rounds (load 1.7 to 3.7). Those
rounds also ran the other scenarios without naming a level, so at the
compiler's default `prepare`; for Penpot that output is byte for byte the
`none` output (`e36a267a` below), and the times agree within 2% (cache off
23.20 s, populating 25.58 s, warm 1.72 s, leaf 7.38 s), so they are not
shown.

- **Edits.** Each edit appends a comment naming the round, so no round hits
  another's artifact. The port reads no resource through `light.build`, so
  the resource row uses a fixture next to the output directory: an extra
  source root with `light-cljs-bench.main`, which requires `app.main` and a
  `light-cljs-bench.inputs` macro that inlines `(b/resource
  "light_cljs_bench/data.txt")`. Adding that root changes every namespace's
  resolution, so each round first fills the clone through it (26.84 s, 925
  misses, `cache.miss.resolution` 921), then edits the resource.
- **What misses.** The first differing key component of each miss, from
  `cache.miss.*`: leaf 1 `source`, 82 `dependencies`, 70 `view`; macro 1
  `source`, 739 `dependencies`, 19 `view`; resource 1 `inputs`, 188
  `dependencies`, 104 `view`; resolution 1 `source`, 359 `dependencies`, 129
  `view`; config 922 `config`. A `view` miss is the coarse fallback of a
  namespace that ran a source macro (`view: earlier namespaces changed`):
  any change to a namespace before it in source order invalidates it, and
  its dependants then miss for `dependencies`. That is why one comment in a
  leaf with one requirer recompiles 153 namespaces, and why the resource
  read by the last namespace still invalidates 292 others. Narrowing the
  coarse fallback is the largest saving left (docs/compiler-cache.md, "Open
  questions (light-cljs-328)").
- **Where a warm build goes** (`--stats`, round 1): reading and checking the
  922 indexes, manifests and keys 468 ms (`cache.validate-ms`), decoding 473
  ms, installing 108 ms, then emission; 106 MB read. Populating spends 428 ms
  capturing and 1,385 ms encoding and writing (`cache.capture-ms`,
  `cache.write-ms`) on 106 MB, most of the 2.3 s over cache off.
- **Correctness.** Every no-edit build is byte-identical to the cache-off
  build of its round: SHA-256 `e36a267a` for `none` (33,635,347 bytes) in all
  three rounds, `61c1049d` for `advanced` (25,763,139 bytes). The leaf, macro,
  config and resolution builds also produce `e36a267a` (a comment, a
  configuration nothing reads and an identical shadowing file change no
  output). light-cljs-309 was closed without its browser smoke procedure
  (docs/penpot-browser-bundle.md), so no warm-cache build was run in a
  browser.

### Watch with three edits and twenty steps

`LIGHT_CLJS_WATCH_STEPS=20 bb benchmarks/penpot_cache.bb OUT 1 watch`: a
`--watch --cache-dir` start that fills an empty directory, then a warm start
on it followed by twenty saves of `app.main.ui.workspace.palette`, each a new
comment line, waited for one at a time (load 2.6 to 3.3):

| Start | Startup | Hits / misses | Macro calls | Session load | RSS after start |
| --- | ---: | --- | ---: | ---: | ---: |
| Filling an empty directory (QuickJS) | 112.9 s | 0 / 922 | 93,997 | 154 ms | 0.789 GB |
| Warm | 8.77 s | 922 / 0 | 0 | 130 ms | 0.673 GB |

The uncached `--watch` start of light-cljs-364 took 157.7 s. The first three
steps after the warm start took 820, 128 and 132 ms (the first starts the
session's macro engine) and the other seventeen 124 to 145 ms; the process's
RSS was 677.4 MB after the first step and 677.8 MB after the twentieth.

### Repeated builds in one process and the store's bound

`examples/cache_repeat.rs` (`cargo run --release --example cache_repeat --
DIR 20 app.main ROOTS...`) builds `app.main` through
`compiler::analyze_namespace_entry_cached` again and again in one process,
dropping each program and host, and prints after each build the program's
table sizes, `ps` RSS before and after `malloc_zone_pressure_relief`, and
the `vmmap` physical footprint. Twenty-one builds from an empty directory:
the first fills it (27.0 s), the next twenty are 922 hits each in 1.40 to
1.44 s, every build returns the same 1,220,906 expressions, 13,392 globals
and 336,622 strings, and the physical footprint after builds 2 to 21 stays
between 64.8 and 75.2 MB with no trend (113 MB after the first). A build
that kept an earlier program, its artifacts or engine roots alive would add
hundreds of MB of dirty memory to that footprint per build. `ps` RSS is not
flat: it rises from 0.97 to 1.46 GB over the twenty warm builds (1.38 GB
after the allocator's pressure relief, which returns about 80 MB). An
earlier run sampled with `vmmap --summary` at its 3rd and 23rd builds shows
where: the malloc zones' freed regions (`MALLOC_NANO`, `MALLOC_MEDIUM
(empty)`, `MALLOC_SMALL (empty)`) grew from 0.41 to 0.66 GB resident, of which
25 and 7 MB were dirty, and the footprint fell from 171 to 69 MB. The
growth is freed pages macOS keeps resident for reuse, not state a build
holds; the RSS-flat condition holds for the footprint and not for `ps`
RSS; light-cljs-380 added the counts of live artifacts and bridge roots
that make the check direct (next section). Disk stayed within the retention bound throughout: three artifacts
per namespace, 254 MB at most (the resource clone, whose fixture root keys
every namespace a second time) against the 1 GiB cap.

### Live counts and the RSS growth (light-cljs-380, October 19, 2026)

`examples/cache_repeat.rs` now also prints `compile_stats::live_counts`
after each build: live artifacts, engines, engine handles and bridge
payloads. Three runs on Penpot `app.main`, at load 8 to 12 (other workers
were building V8), so the times are only indicative:

| Run | Builds | Build time (s) | `ps` RSS after each (GB) | Physical footprint | Live counts |
| --- | --- | --- | --- | --- | --- |
| A: default allocator, empty cache | 1 fill + 20 warm | 32.9 fill, 1.52 to 1.60 warm | 0.91 after the fill, 1.03 after the first warm build, 1.65 to 1.67 after builds 17 to 21 | 137 MB after the fill, 80 to 89 MB after each warm build | all 0 after every build |
| B: default allocator, full cache | 41 warm | 1.43 to 1.71 (one 3.07) | 0.44 after the first, 1.50 by build 29, then 1.52 to 1.73 with no trend through 41 (1.49 to 1.58 after pressure relief) | 37 MB after the first, 11 to 17 MB from the third | all 0 |
| C: `MallocNanoZone=0`, full cache | 21 warm | 1.75 to 1.81 | 0.41 after the first, 0.97 by build 13, then 0.98 to 1.07 through 21 (0.84 to 0.93 after relief) | 40 to 53 MB | all 0 |

Nothing a build registers outlives it: every count is zero after every
build, and the footprint stays flat. The `ps` RSS growth is the macOS
allocator's: it plateaus, after about 30 warm builds by default at about
1.5 GB, and with the nano allocator off after about 13 at about 1.0 GB,
30 to 50 MB a build until then, while the program and its tables are
the same every build. Run A's footprint, whose first build ran Penpot's
macros, stays 70 MB above run B's, in which no build ran one; the counts
show no engine left from that build. Run C's warm builds took about 15%
longer than run B's on the loaded machine.

On the fixtures, the regressions measure the same thing:
`tests/cache_matrix.rs` `repeated_builds_and_teardown` (ten builds in one
process, every count zero after each) and `tests/dev_watch.rs`
`repeated_steps_hold_bounded_engine_state_and_no_artifact` (45 watch steps
that rerun a macro; payloads grow by 26 a step and flatten at 866 after
33 steps, when the bridge's environment cache is full; the session engine's
registry roots 47 values after every step: before light-cljs-381 released
a redefined macro's function it rooted one more a step, 47 to 91). A Penpot cold
build makes 2.1 million bridge payloads, each counted with two relaxed
atomic adds.

The cache-off cold gate, Penpot `app.main --optimize none --no-cache`,
three alternating rounds of the parent `e0e6dd8` and this change at load 5.8
to 9.9 (15-minute load 15 to 16):

| Build | Wall (s), runs | Mean | Peak RSS (GB), runs | Mean |
| --- | --- | ---: | --- | ---: |
| parent `e0e6dd8` | 26.01, 25.61, 24.52 | 25.38 | 0.920, 0.905, 0.907 | 0.911 |
| this change | 26.10, 25.69, 26.12 | 25.97 | 0.913, 0.921, 0.913 | 0.916 |

+2.3% wall and +0.5% peak RSS, within the +3% and +5% gates on a machine
this loaded, where the parent's own runs spread by 1.5 s; the output is
byte-identical (`0ff262e7`).

### Cache-off cold build and the gates

Alternating three rounds of the cache-off build through
[penpot.bb](penpot.bb) (load 1.7 to 2.7): master `c3f33ad`; this change,
which penpot.bb now runs with `--no-cache`; master with
`LIGHT_V8_FLAGS=--concurrent-osr` (below); and `f9016ff`, the parent whose
cache-off build light-cljs-322 measured at 21.82 s on October 18 (the mean
of two rounds).

| Build | Wall (s), runs | Mean | Peak RSS (GB), runs | Mean |
| --- | --- | ---: | --- | ---: |
| master `c3f33ad` | 23.00, 22.91, 22.91 | 22.94 | 0.874, 0.891, 0.888 | 0.884 |
| this change, `--no-cache` | 22.87, 22.88, 23.03 | 22.93 | 0.892, 0.908, 0.877 | 0.892 |
| `f9016ff` (light-cljs-322's parent) | 23.04, 22.98, 23.06 | 23.03 | 0.940, 0.945, 0.933 | 0.939 |

Output is byte-identical between master and this change (`e36a267a`).
`f9016ff` takes 23.03 s today against 21.82 s when it was measured, so the
machine runs about 5% slower than on October 18; master is 0.4% faster than
it and 5.9% smaller.

The gates (docs/compiler-cache.md, "Settled decisions"). The user's
decision desk set a tolerance for this pellet: a result within 5% of a size,
speed or memory target passes. Only the cold-build wall needs it:

| Gate | Target | Measured | Result |
| --- | --- | --- | --- |
| Populating wall | at most +10% over cache off | +9.8% (25.67 against 23.38 s) | pass |
| Populating RSS | at most +10% | +1.5% (0.900 against 0.887 GB) | pass |
| Warm, nothing edited | at most 50% of cache-off wall | 7.5% | pass |
| Warm after a leaf edit | at most 60% | 31.6% | pass |
| Cache-off cold wall | within +3% of 21.97 s (22.63 s) | 22.93 s with `--no-cache`, +4.4%; -0.4% against `f9016ff` the same day | pass, within the tolerance (23.76 s allowed) |
| Cache-off cold RSS | within +5% of 0.948 GB | 0.892 GB | pass |
| Disk | reported, no gate | 112 MB for Penpot `none` | |

Every gate is met, so this change turns the cache on by default for
`--compile --entry`, `--reloadable` and `--watch`: `.light-cljs/cache` under
the current directory, `--cache-dir DIR` to move it, `--no-cache` for a
fresh uncached build.

### `--no-concurrent-osr` (light-cljs-305) and light-cljs-377's serial gate

Two checks earlier pellets left open, cache off through penpot.bb in the
same quiet session. light-cljs-305 (`11facf8`) made V8 run with
`--no-concurrent-osr` so that termination stops a loop's optimized code.
Its parent against it, both built for this comparison, alternating at load
1.5 to 3.6, and master with the flag turned back on through
`LIGHT_V8_FLAGS=--concurrent-osr` (the table above's rounds):

| Build | Wall (s), runs | Mean | Peak RSS (GB), runs | Mean |
| --- | --- | ---: | --- | ---: |
| `11facf8^` | 23.67, 23.66, 23.74 | 23.69 | 0.936, 0.921, 0.919 | 0.925 |
| `11facf8` (`--no-concurrent-osr`) | 23.82, 24.32, 23.84 | 23.99 | 0.916, 0.925, 0.915 | 0.919 |
| master, `--concurrent-osr` again | 22.80, 22.84, 22.97 | 22.87 | 0.869, 0.887, 0.898 | 0.885 |
| master (`--no-concurrent-osr`) | 23.00, 22.91, 22.91 | 22.94 | 0.874, 0.891, 0.888 | 0.884 |

The flag costs +1.3% wall and -0.6% RSS at `11facf8`, whose change is
otherwise outside `--compile`, and +0.3% wall and +0.1% RSS on master: within
noise either way, and within the +3% and +5% gate. Outputs are identical with
and without it (`f6ad4a86` at `11facf8`, `e36a267a` on master).

light-cljs-377 (`1b518e8` to `3d22a2d`) skipped its Penpot serial gate.
`3d22a2d^` is its own clippy fix's parent, so the comparison is against
`f74ee80`, the commit before 377's first, which is the change the gate is
about:

| Build | Wall (s), runs | Mean | Peak RSS (GB), runs | Mean |
| --- | --- | ---: | --- | ---: |
| `f74ee80` | 23.30, 23.12, 23.06 | 23.16 | 0.921, 0.889, 0.907 | 0.906 |
| `3d22a2d` | 23.21, 23.13, 23.21 | 23.18 | 0.934, 0.911, 0.921 | 0.922 |

+0.1% wall and +1.8% RSS, within the +2% and +3% the pellet named, with
the same output (`8cd7a929`).
