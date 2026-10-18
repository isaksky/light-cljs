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
flags (light-cljs-349). The large fixture is therefore the script's generated
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
