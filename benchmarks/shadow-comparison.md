# light-cljs against shadow-cljs: the answer to the critique (November 14, 2026)

On November 13, 2026 a reviewer who knew nothing about light-cljs read our
comparison with shadow-cljs and raised seven objections (group
shadow-comparison-credibility). This document answers them one by one with what
the group's pellets measured since, says where light-cljs loses, what is still
unmeasured and what is a non-goal, and lists the features shadow-cljs has that
light-cljs lacks. A second reviewer, also new to light-cljs, then read only
this document's claims; its objections and our answers are at the end.

Every figure names the light-cljs commit it was measured on. Several figures
were superseded within the day; this document quotes the latest and says so.
Every run was on the same machine (Apple M3 Ultra virtual machine, 24 CPUs,
32 GiB, macOS 15.7.3), with shadow-cljs 3.5.3, OpenJDK 25.0.2 and Node 26.12.1.
**None of it ran on a reserved machine**: other workers were building and
timing throughout, by the user's decision of November 13 ("please dont stop
because of the machine not being free"). The load average before every run is
in the source sections.

## Summary

In short: light-cljs compiles large programs several times faster than
shadow-cljs and reloads edits far sooner, but **the JavaScript it produces runs
3.5 to 7 times slower**, its memory advantage is small once shadow-cljs's heap
is sized to need, and small apps come out somewhat larger. The scale figures
come from projects we generate ourselves, and none comes from a quiet machine.

**Where light-cljs is ahead**

- **Compile time on large generated projects.** A clean release build takes
  about 0.15x shadow-cljs's wall time at 200k lines and 0.17x at 800k; a
  rebuild with nothing changed 0.03x; and 0.009 to 0.089x of its CPU time
  (`627b9ed5`). Since then light-cljs's own 800k clean build has fallen to
  28 s (plain) and 33 s (re-frame shape) in `--stats` builds on `809302a4`,
  from 37 s and 52 s for `fc31d229` in the same alternated runs.
- **The edit-to-reload loop.** On a generated 200k-line re-frame app,
  `light-cljs watch` has new code running 77 to 83 ms after a save to a view or
  a widely required namespace, and 255 ms after a macro edit; `shadow-cljs
  watch` takes 2.4 s, 6.5 s and 2.3 s (`fa9856b4`). Its startup is about half
  shadow-cljs's (19 to 20 s against 37 to 46 s).
- **Real apps work.** re-frame's TodoMVC, simple and flow, built by light-cljs,
  pass the same behaviour checks in headless Chromium as shadow-cljs's builds,
  and re-frame's own test suite passes 118 of 118 tests in light-cljs's dev
  and release builds, as in shadow-cljs's.

**Where light-cljs loses** (recorded as found)

- **Its output runs slower.** In steady state about 3.5 to 3.75 times
  shadow-cljs's time on the generated programs in Node, and 6.95 times on
  TodoMVC's "add 1,000 todos" in Chromium (`cdf39aae`, `181dc839`). Startup is
  1.5 to 2.7 times. This is the largest loss.
- **Its memory advantage shrinks once shadow-cljs gets only the heap it
  needs.** At shadow-cljs's smallest adequate heap light-cljs uses 0.45x,
  0.62x and 0.85x its peak memory (generated 200k, re-frame-shaped 200k,
  generated 800k), not the 0.30 to 0.46x it uses against `-Xmx8g` in the same
  runs (`6ea2540d`).
- **The dev page of a large app loads slower**: 4.9 to 5.3 s against 2.0 s for
  the generated 200k re-frame app (`fa9856b4`), mostly the app's first run,
  which is the runtime-speed gap again.
- **Release size of small apps.** After gzip, re-frame's simple is 6.9% larger
  than shadow-cljs's and TodoMVC 0.2% larger; after brotli 10.1% and 3.9%
  (`8b11492b`). flow is 2.0% smaller after gzip and 1.6% larger after brotli
  (`5dbf5eda`). On the generated projects light-cljs's output is 0.85 to 0.95x
  shadow-cljs's after gzip.
- **A rebuild after a one-file edit still grows faster than the program**: 3.95
  to 22.5 s from 200k to 800k lines in the re-frame shape, 5.7x for 4.0x the
  code (`c5dad571`). Set against shadow-cljs's figures from the growth run,
  it is 0.07 to 0.09x of shadow-cljs's in the re-frame shape at 200k, 400k
  and 800k, and 0.10 to 0.15x in the plain shape.

**What the evidence is and is not.** Compile-speed and memory claims at scale
come from **generated projects** (`benchmarks/generated-project`), which we
wrote, in two shapes: a plain one and a re-frame-shaped one that uses reagent,
re-frame and React. re-frame's three example apps show **correctness, output
size, output speed and the watch loop** on code nobody here wrote; they are too
small to say anything about compile speed or memory, because shadow-cljs's
time there is mostly JVM startup.

**Non-goals.** Compiling unmodified third-party shadow-cljs projects is not a
light-cljs goal: light-cljs builds a port made by a porter tool, and every port
edit is listed by cause. Closure Library support is out of scope; where an app
uses `goog.*`, its port carries a small shim.

### Headline figures

| Question | light-cljs | shadow-cljs | light/shadow | Commit, source |
| --- | --- | --- | --- | --- |
| Clean build, generated 200k | 6.35 s, 1.04 GB | 41.54 s, 4.16 GB | 0.15 time | `627b9ed5`, [baseline][g968] |
| Clean build, generated 800k | 40.55 s, 3.78 GB | 243.48 s, 7.81 GB | 0.17 time | `627b9ed5`, [baseline][g968] |
| Rebuild, nothing changed, 200k / 800k | 1.26 s / 6.45 s | 41.67 s / 172.28 s | 0.030 / 0.037 | `627b9ed5`, [baseline][g968] |
| Rebuild after a one-file edit, 200k / 800k | 5.19 s / 28.14 s | 38.85 s / 165.38 s | 0.13 / 0.17 | `627b9ed5`, [baseline][g968] |
| Peak memory against shadow's smallest adequate heap, 200k / re-frame 200k / 800k | 1.03 / 1.41 / 3.65 GB | 2.28 / 2.27 / 4.30 GB | 0.45 / 0.62 / 0.85 | `6ea2540d`, [heap sweep][g969] |
| Clean-build growth, 200k to 800k (4.0x the lines), plain / re-frame shape | 4.37x / 4.29x | 4.70x / 5.69x | | `809302a4` [append][g996]; shadow [growth][g970] |
| Output after gzip, generated 200k / 800k | 859,697 / 3,299,312 B | 1,002,909 / 3,894,315 B | 0.86 / 0.85 | `627b9ed5`, [baseline][g968] |
| Output speed, steady state per iteration, workload 200k / 800k / re-frame-shaped 200k | | | 3.75 / 3.50 / 3.69 slower | `cdf39aae`, `181dc839`, [runtime][r998] |
| TodoMVC, add 1,000 todos in Chromium | 6,473 ms | about 930 ms | 6.95 slower | `cdf39aae`, [runtime][r998] |
| TodoMVC release after gzip / brotli, React included | 137,348 / 119,062 B | 137,088 / 114,615 B | 1.002 / 1.039 | `8b11492b`, [size][f984] |
| Watch, generated re-frame 200k: save to new code running, view / widely required / macro | 83 / 77 / 255 ms | 2,388 / 6,463 / 2,281 ms | | `fa9856b4`, [watch][w1001] |
| Watch startup / dev page load, same app | 18.6-20.0 s / 4.9-5.3 s | 37.0-45.7 s / 2.0 s | | `fa9856b4`, [watch][w1001], [page][w993] |

[g968]: generated-shadow-vs-light.md#five-round-baseline-at-627b9ed5-unreserved-machine-november-13-2026
[g969]: generated-shadow-vs-light.md#shadow-cljss-maximum-heap-swept-unreserved-machine-november-14-2026
[g970]: generated-shadow-vs-light.md#growth-from-50k-to-800k-lines-both-shapes-one-binary-one-day-unreserved-machine-november-14-2026
[g1000]: generated-shadow-vs-light.md#why-a-one-file-edit-rebuilds-slower-than-a-clean-build-at-400k-unreserved-machine-november-14-2026-light-cljs-1000
[g347]: generated-shadow-vs-light.md#declaration-views-for-source-macro-users-unreserved-machine-november-14-2026-light-cljs-347
[g995]: generated-shadow-vs-light.md#untraced-analysis-loop-time-at-200k-and-800k-unreserved-machine-november-14-2026-light-cljs-995
[g994]: generated-shadow-vs-light.md#the-top-level-passs-own-time-at-200k-and-800k-unreserved-machine-november-14-2026-light-cljs-994
[g994b]: generated-shadow-vs-light.md#three-more-position-dependent-terms-in-the-analysis-loop-unreserved-machine-november-14-2026-light-cljs-994-second-attempt
[g996]: generated-shadow-vs-light.md#the-append-stages-growth-on-master-unreserved-machine-november-14-2026-light-cljs-996
[g997]: generated-shadow-vs-light.md#capturing-a-recompiled-namespace-into-the-cache-200k-and-800k-unreserved-machine-november-14-2026-light-cljs-997
[g1003]: generated-shadow-vs-light.md#a-stale-candidate-costs-its-manifest-unreserved-machine-november-14-2026-light-cljs-1003
[gshape]: generated-project/README.md#the-re-frame-shape
[fsize]: re-frame-vs-shadow.md#release-size
[f979]: re-frame-vs-shadow.md#2026-11-14-what-the-gzip-gap-consists-of-light-cljs-979
[f999]: re-frame-vs-shadow.md#2026-11-14-release-elides-asserts-by-default-light-cljs-999
[f985]: re-frame-vs-shadow.md#2026-11-14-edn-data-reading-is-not-reflection-light-cljs-985
[f984]: re-frame-vs-shadow.md#2026-11-14-constant-defs-fold-through-other-defs-and-functions-light-cljs-984
[ftime]: re-frame-vs-shadow.md#build-time
[fedits]: re-frame-vs-shadow.md#port-edits-by-cause
[ftests]: re-frame-vs-shadow.md#2026-11-14-118-of-118-tests-pass-on-both-compilers
[fbrowser]: re-frame-vs-shadow.md#2026-11-14-running-in-a-browser
[fspec]: re-frame-vs-shadow.md#2026-11-14-the-real-cljsspecalpha-in-todomvc-and-flow
[r975]: runtime-vs-shadow.md#summary
[r988]: runtime-vs-shadow.md#2026-11-14-protocol-lookup-on-the-runtimes-own-values-light-cljs-988
[r989]: runtime-vs-shadow.md#2026-11-14-small-maps-and-map-entries-light-cljs-989
[r990]: runtime-vs-shadow.md#2026-11-14-one-kind-brand-for-the-runtimes-stores-light-cljs-990
[r998]: runtime-vs-shadow.md#2026-11-14-the-protocol-hooks-on-the-runtimes-own-values-light-cljs-998
[w972]: watch-vs-shadow.md#2026-11-14-todomvc-and-the-generated-200k-re-frame-app
[w991]: watch-vs-shadow.md#2026-11-14-later-watch-steps-after-light-cljs-991
[w992]: watch-vs-shadow.md#2026-11-14-later-watch-startup-after-light-cljs-992
[w993]: watch-vs-shadow.md#2026-11-14-later-dev-page-load-after-light-cljs-993
[w1001]: watch-vs-shadow.md#2026-11-14-later-watch-steps-after-light-cljs-1001

TodoMVC's shadow-cljs figure is computed from the light-cljs median and the
ratio, both from the same alternated runs. The growth row's two sides come from
different runs on the same day: light-cljs's from `809302a4` with `--stats`,
shadow-cljs's from the growth section's alternated runs with `fc31d229`.

**What a compile figure includes.** Neither side bundles in the generated
projects: both outputs run in Node and take React from `node_modules` at run
time. In re-frame's examples, shadow-cljs bundles React itself, and
light-cljs's esbuild step is timed as its own row (0.03 to 0.11 s,
[build time][ftime]). shadow-cljs 3.5.3 does not lower the language level by
default (`:output-feature-set :es-unsupported`), so neither side spends time
lowering.

## The objections, one by one

### 1. "The real-world comparison is not the same program on both sides"

*The objection:* Penpot was a hand port with shims and patches, shadow-cljs
built modules ours skipped, and nobody had ever run light-cljs's output of a
real app.

**What was done.** The real-world apps are now re-frame's examples, TodoMVC,
simple and flow, by the user's decision; Penpot is no longer evidence (see
"Penpot" below).

- shadow-cljs builds the **unmodified upstream examples**; only its pin moved
  from 3.2.0 to 3.5.3.
- light-cljs builds the **porter's port** of each, regenerated by
  `bb benchmarks/re-frame.bb port`. [Every port edit is listed by
  cause][fedits]: compiler gaps (all now fixed on master, each with a
  regression test), JVM-only macros whose expansion the porter takes from the
  JVM, Closure Library shims (a non-goal), and shadow-cljs build options with
  no light-cljs counterpart (`:init-fn`, `:target :browser`, preloads).
- **The ports are the same program where it matters.** The porter's stub of
  cljs.spec.alpha flattered light-cljs (it made `s/valid?` always true) and
  was replaced with the real library ([light-cljs-980][fspec]); the
  `goog-define` flags became `^:const` defs that fold the same way.
- **light-cljs's output of the apps runs.** In headless Chromium, every
  TodoMVC specification check (add, complete, toggle all, edit, delete,
  filter routes, clear completed, persistence), simple's clock checks and
  flow's eleven-step page-text comparison with shadow-cljs's run pass 3 of 3
  on both builds, with no console errors ([browser][fbrowser]). None of the
  three apps started before this group fixed two shim bugs and one compiler
  defect.
- **re-frame's own test suite** (118 tests, 446 assertions) passes 118 of 118
  in light-cljs's dev build and its advanced release build, against shadow-cljs's
  118 ([tests][ftests]). Caveat: re-frame.core-instrumented's macros read
  `&env` and `*file*`, so the porter splices shadow-cljs's expansions at 24
  call sites and 40 `defalias` forms; the three source-meta test namespaces
  test light-cljs running shadow-cljs's expansion there. The suite's
  `cljs.test` is upstream's, vendored with 12 edits.

**What remains.** It is still a port. A skeptic should read the port-edit
inventory and judge whether a user would accept making those edits; compiling
the unmodified project is a non-goal, so light-cljs will not close this gap
by compiling shadow-cljs projects as they are.

### 2. "Developers use `shadow-cljs watch`, not cold command-line builds"

**What was done.** `benchmarks/watch-vs-shadow.bb` runs each tool's own watch,
dev server and reload client, saves the same edits into the same sources, and
times from the file's modification time to the output written, the reload
message, and a probe in headless Chromium that turns true only once the new
code runs ([watch][w972]).

| App, edit | light-cljs | shadow-cljs | Commit |
| --- | --- | --- | --- |
| TodoMVC, a view | 64 ms | 979 ms | `e5b7832e` |
| TodoMVC, re-frame.loggers (required by 14 namespaces) | 173 ms | 988 ms | `e5b7832e` |
| generated re-frame 200k, a view | 83 ms (47-139) | 2,388 ms | `fa9856b4` |
| generated re-frame 200k, `workload.common` (required by 217 namespaces) | 77 ms (27-117) | 6,463 ms | `fa9856b4` |
| generated re-frame 200k, a project macro used by 8 namespaces | 255 ms (196-302) | 2,281 ms (957 with a fixed pause) | `fa9856b4` |

Medians of 10 to 20 edits; the 200k rows use a random pause between edits
([light-cljs-1001][w1001]), since a fixed pause had favoured shadow-cljs's
macro edit. TodoMVC's light-cljs times were rechecked on `951119a3` (63 and
170 ms).

**Where light-cljs lost, and what happened.** The first run found three losses
on the 200k app; all were filed and two are gone:

- **The macro edit** was slower (1.43 s against 0.96 s); it is now 255 ms
  ([light-cljs-991][w991], [light-cljs-1001][w1001]).
- **Startup** took 169 to 175 s against 38 to 40 s; it is now 18.6 to 20.0 s
  against shadow-cljs's 37.0 to 45.7 s, with one shadow-cljs session at
  102.3 s under load ([light-cljs-992][w992], [light-cljs-993][w993]).
- **Dev page load is still slower: 4.9 to 5.3 s against 2.0 s**
  ([light-cljs-993][w993]). Loading the 3,077 files in parallel cut it from
  about 8 s; what is left is mostly the app's first run (mounting it and
  dispatching 2,100 events), which is the runtime-speed loss of objection 4.

**What the numbers do not say.** Most of shadow-cljs's time is noticing the
save: its default macOS watcher (the JVM watch service, 500 ms loop) against
light-cljs's 100 ms poll. Only the default watcher was measured; a faster
watcher setting for shadow-cljs was not tried. On compile step alone the
widely required TodoMVC edit is a tie (150 against 144 ms). Its watch was
measured on macOS only. docs/dev-workflow.md's target of under 100 ms from
save to updated page now holds on the 200k app at the median, but its slowest
edits (139 and 117 ms) miss it.

### 3. "shadow-cljs's peak memory reflects the JVM default heap"

**The objection was right.** shadow-cljs's maximum heap was swept from 1 to
8 GiB over clean release builds, three alternated rounds each, and light-cljs
compared with shadow-cljs at its **smallest adequate heap**: the smallest heap
at which no build failed and the median wall time is within 10% of its best
([heap sweep][g969], `6ea2540d`).

| Project | light-cljs peak memory | shadow-cljs's smallest adequate heap | its peak memory | light/shadow | at `-Xmx8g` |
| --- | --- | --- | --- | --- | --- |
| generated 200k | 1.029 GB | 1.5 GiB | 2.281 GB | **0.45** | 0.30 |
| re-frame-shaped 200k | 1.405 GB | 1.5 GiB | 2.268 GB | **0.62** | 0.36 |
| generated 800k | 3.646 GB | 3 GiB | 4.298 GB | **0.85** | 0.46 |

- **At 800k the memory advantage nearly disappears**: 3.65 GB against
  4.30 GB. Any claim built on the `-Xmx8g` ratios overstates it.
- The JVM's default maximum heap on this 32 GiB machine is 8 GiB, the same as
  the projects' `-Xmx8g`.
- Wall time does not change the picture: at its smallest adequate heap
  shadow-cljs still takes 6 to 8 times as long. A smaller heap never made it
  faster; below 3 GiB at 800k it runs out of memory.
- light-cljs's `:macro-memory-limit` is not the counterpart of `-Xmx`: it caps
  only macro expansion's JavaScript heap, and lowering it to 64 MiB changed
  peak memory by under 3% at 200k. At 800k 64 MiB is too small and the build
  fails, which is why the generated projects set 2 GiB.
- The 10% rule is close to the noise of this unreserved machine; the adequate
  heap could move by one step on a quiet one (1 GiB at 200k, 4 GiB at 800k).

### 4. "Output was judged by size only"

**How fast the output runs** ([runtime][r975]). Both compilers' release
builds were run 10 times each, alternated: the generated programs in Node, the
re-frame-shaped app mounted in jsdom, TodoMVC driven through its own DOM events
in headless Chromium. **light-cljs's output is slower everywhere measured.**
Latest ratios, light/shadow:

| Workload | First output (startup and one iteration) | Steady state per iteration | Peak memory | Commit |
| --- | --- | --- | --- | --- |
| generated workload 200k | 1.50 | 3.75 | 1.20 | `67b1bbec`, `cdf39aae`, `c755984d` |
| generated workload 800k | 1.51 | 3.50 | 1.17 | `67b1bbec`, `181dc839`, `c755984d` |
| re-frame-shaped 200k app | 2.72 | 3.69 | 1.05 | `32cb0373`, `cdf39aae`, `c755984d` |

Peak memory is at the largest iteration count (40, 16 and 10). Each column's
commit is the latest run that moved it; later runs left first output and peak
memory within their ranges. In TodoMVC: page load to interactive 1.41, add
1,000 todos 6.95, toggle all 2.68, edit one of 1,000 todos 1.21, JavaScript
heap with 1,000 todos 1.24 times shadow-cljs's.

Four runtime changes in this group ([988][r988], [989][r989], [990][r990],
[998][r998]) cut the steady-state ratios from 4.73 to 5.67 (`67b1bbec`) to
3.50 to 3.75, and TodoMVC's add step from 10.9 to 6.95. **It is still the largest loss**, in cljs.core's place:
the runtime's protocol lookup, map entries and value-kind tests, and closures.
light-cljs-1002, building a map seq's entries only when they are read, is
finished but **not on master**: it is held for the user because it showed no
measured gain.

**Property renaming.** Closure's `:advanced` renames properties and leaves no
`cljs$core$...` names; light-cljs never renames host-facing names, by design,
so it needs no externs, and its TodoMVC output keeps 34 arity property names
and the protocol method names. Shortening those would save 0.3 to 0.4 KB after
gzip ([breakdown][f979]).

**JavaScript language level.** shadow-cljs 3.5.3's output is ES2015-level.
light-cljs writes modern JavaScript (ES2020 `?.` and `??`, and newer) and does
not lower it; lowering its bundle to ES2015 with esbuild costs 1.0 to 2.3%
after gzip in the latest size tables (flow 1.0%, TodoMVC 2.1%, simple 2.3%;
[size][fsize]). A user who needs older browsers pays that.

**brotli, and size.** The re-frame examples report raw, gzip and brotli sizes,
React counted apart ([size][fsize]). Latest, whole file with React, light-cljs
against shadow-cljs:

| App | gzip | brotli | Commit |
| --- | --- | --- | --- |
| simple | +6.9% | +10.1% | `8b11492b` |
| TodoMVC | +0.2% | +3.9% | `8b11492b` |
| flow | -2.0% | +1.6% | `5dbf5eda` (not rerun after the last two size changes) |

- These losses shrank in this group: asserts are elided in release by
  default, as in shadow-cljs ([999][f999]); EDN reading no longer registers
  every namespace ([985][f985]); constant defs fold through other defs and
  functions, so re-frame's tracing code goes ([984][f984]); clojure.set and
  clojure.walk keep only what is called. TodoMVC was 11.5% larger after gzip
  before them.
- What is left in simple is mostly light-cljs's runtime, which has fewer
  characters than cljs.core but compresses worse (0.31 against 0.20 gzip bytes
  per character); the smaller the app, the larger that share.
- The generated projects report gzip only, not brotli (0.85 to 0.95x).

### 5. "The synthetic project is our own and uses no real libraries"

**What was done.** The generator gained a re-frame shape ([shape][gshape]):
reagent views (keyed lists, form-2 and form-3 components, `with-let`),
re-frame events with interceptors and effects, layered subscriptions, specs as
predicates, React 18 through `react-dom/client`, and reagent's and re-frame's
real sources. Both compilers expand the libraries' macros. An independent
oracle replays the event sequence on a model and computes the expected
checksums of `app-db` and the rendered markup; both outputs must print them.

- It is still our generator. Its library code is real; its app code is
  generated. That is why re-frame's examples are kept beside it.
- light-cljs's side uses the porter's port of the libraries, with four edits
  listed by cause in the generator's README (two macro-alias fixes and two
  Closure Library shims); a few of reagent's macros are spliced JVM expansions,
  as in the examples.
- Results on that shape: clean build 0.12x shadow-cljs's wall time at 200k and
  800k (`fc31d229`), rebuild after a one-file edit 0.07 to 0.09x (the latest
  light-cljs figures against the growth run's shadow-cljs), peak memory 0.62x at
  shadow-cljs's smallest adequate heap, output 0.95x after gzip (`4d2669bc`
  smoke row) and running 3.69x slower in steady state.

### 6. "The measurements are sloppy"

*The objection:* single runs, a loaded virtual machine, dates mixed, CPU time
missing.

- **One kit.** `benchmarks/generated-project/compare.bb` now runs every
  head-to-head: alternating compiler order each round, medians and ranges of
  wall time, user and system CPU time and peak memory, the load average
  before every build, the binary's commit and hash, and tool versions. After
  its rounds it checks each compiler's last clean build and last one-file-edit
  build against the oracle and for leftover unused code; earlier rounds, the
  rebuilds with nothing changed and the warm-JVM builds are not checked one by
  one ([baseline][g968]). The heap sweep checked every successful build
  ([heap sweep][g969]).
- **Medians**, of 2 to 10 runs, with ranges; the one single run is the
  re-frame-shaped 800k project in the growth run, said there.
- **CPU time** is reported everywhere, user and system separately. It showed
  something new: shadow-cljs spends as much or more system CPU than user CPU on
  its clean builds (418 s system against 159 s user at re-frame 200k), which
  may be the loaded machine and is recorded as found.
- **Still loaded.** No figure here comes from a reserved machine. Load average
  before a build ranged from about 2 to 41 across the sections. Ratios between
  alternated runs are the fairer comparison; absolute times move with load
  (shadow-cljs's 800k clean build ranged from 199 to 345 s in one run).
- **Still several binaries.** The figures span about twenty light-cljs commits
  over two days, because each pellet measured its own change. Each figure names
  its commit. No single binary was rerun against every comparison at the end.

### 7. "Growth from 200k to 800k looked superlinear"

**It was, for light-cljs, and it is now close to linear for clean builds.**
On one day with one binary (`fc31d229`, [growth][g970]), light-cljs's clean
build grew 5.4x (plain) and 5.8x (re-frame shape) for 4.0x the lines, against
shadow-cljs's 4.7x and 5.7x. `--stats` named four terms that grew 6x to 22x;
each was filed and fixed with byte-identical output: the top-level pass
scanning earlier namespaces ([994][g994], [994 again][g994b]), untraced
analysis-loop time ([995][g995]), appending fragments ([996][g996]) and cache
capture ([997][g997]).

Re-measured on `809302a4`, five alternated rounds against `fc31d229`,
`--no-cache --stats` builds ([append][g996]); shadow-cljs's column is the
growth run's:

| Shape | `fc31d229` clean build 200k -> 800k | `809302a4` | shadow-cljs (`fc31d229` day) |
| --- | --- | --- | --- |
| plain | 6.6 -> 37.2 s, 5.61x | 6.4 -> 28.1 s, **4.37x** | 40.1 -> 188.4 s, 4.70x |
| re-frame shape | 9.6 -> 51.9 s, 5.41x | 7.8 -> 33.3 s, **4.29x** | 73.2 -> 416.5 s, 5.69x |

**The rebuild after a one-file edit was worse** and is better but still
superlinear:

- It was slower than a clean build from 400k up, because an edit recompiled
  every later namespace that ran a source macro ([1000][g1000]). Declaration
  views ([347][g347]) cut the same edit at 400k from 587 and 2,127 recompiled
  namespaces to 3 and 4, and the rebuild from 18.8 and 25.1 s to 6.4 and
  8.5 s (`a0383188`).
- At re-frame 200k and 800k it now takes 3.95 s and 22.5 s (`c5dad571`), 5.7x
  for 4.0x the code. What is left is preparing and emitting the whole program
  (advanced optimization is whole-program), and cache validation that varies
  by 2 to 8 s with load ([1003][g1003]). shadow-cljs's took 57.6 and 346.5 s.
- An edit that adds or changes a protocol, type, extension, multimethod or
  macro still recompiles every later namespace that ran a source macro; this
  is the price of the cache's soundness rule and is tested.

## Feature parity with shadow-cljs

Checked against the code at master `680f1ec2`: `src/config_file.rs` (the keys
the configuration file accepts and refuses, with the reason it gives),
`src/main.rs` (the command line), `src/dev_nrepl.rs`, `src/dev_client.js` and
the tests named. "Refused" means light-cljs reports an error naming the key and
the reason, it does not ignore it.

| Feature | shadow-cljs | light-cljs | Where checked |
| --- | --- | --- | --- |
| Release builds | `release`, Closure `:advanced` (also `:simple`) | `release ID`, its own optimizer at `:none`, `:prepare` or `:advanced` (default); no Closure Compiler | `config_file.rs` `Setting::Optimizations`; `main.rs` usage |
| Dev builds | `compile`, `watch` | `compile ID`, `watch ID`: one ES unit per namespace with a source map, a manifest and a loader | `main.rs` usage (`--reloadable`), `dev_build.rs` |
| Watch | file watcher (500 ms loop on macOS) | polling, 100 ms by default (`:fs-watch {:loop-wait N}`); `:polling` is the one watcher | `config_file.rs` `FsWatchImpl` |
| Hot reload | changed namespace and its dependents; `:after-load` hooks | changed namespace only (callers look definitions up in a registry), or dependents with `:devtools {:reload-strategy :full}`; `^:dev/before-load`/`^:dev/after-load` metadata; error overlay in the page | `main.rs` usage, `dev_client.js` |
| Watch of several builds in one process | yes | yes, one server, each build under `/ID/` | `main.rs` usage |
| REPL | `cljs-repl ID` (starts a server if needed), Clojure REPL | `cljs-repl ID` into a running watch only; standalone `--repl` in the embedded engine | `main.rs` usage, `tests/dev_repl.rs` |
| nREPL | own port, starts in Clojure, middleware (CIDER) | on the dev server's port or `:nrepl {:port N}`, sessions in ClojureScript; operations `clone close completions describe eval interrupt load-file lookup ls-sessions`; no middleware | `dev_nrepl.rs` `OPS`, `tests/dev_nrepl.rs` |
| npm packages | resolves `node_modules`, bundles them | resolves nothing: `--esm`/release output imports packages statically for a bundler (esbuild in our runs); otherwise `globalThis.lightCljsPackages` supplied by the host. The dev page needs packages pre-bundled | `config_file.rs` `deps`/`js-options` refused; `tests/compiler/npm_packages.rs` |
| Dependencies (`:dependencies`, `deps.edn`) | resolved from Maven and Clojars | none: library sources are source paths (the porter vendors them) | `config_file.rs` `dependencies`/`deps` refused |
| Source maps, dev | yes | yes, one per namespace unit | `dev_build.rs` (`sourceMappingURL`) |
| Source maps, release | `release --source-maps` | only for one-file output from the flag command line (`--compile ... --source-map PATH`); the `release` command and chunked output refuse them | `main.rs` (`--out-dir` excludes the source-map flags), `config_file.rs` `source-map` refused |
| Code splitting | explicit `:modules` graph with `:depends-on` | inferred chunks from `light.lazy/loadable` call sites, loaded with `light.lazy/load`; one `:modules` entry only | `config_file.rs` `depends-on` refused; `tests/compiler/chunks.rs` |
| Named modules | yes | no: named chunk groups are designed (docs/code-splitting.md, Phase 4) but not built | no `--chunk` flag in `main.rs` |
| Web workers | `:web-worker true` module | no worker module or worker chunks (docs/code-splitting.md, Phase 5); loading an ES module build as a module worker is untested | no worker option in `config_file.rs` |
| `:prepend` / `:prepend-js` | yes | refused: "put other scripts in the page" | `config_file.rs` `NO_EQUIVALENT` |
| `:exports` | named exports | refused: the startup chunk's default export is the entry namespace's value; no `:init-fn` either | `config_file.rs` `NO_EQUIVALENT` |
| Build hooks | `:build-hooks` (Clojure functions) | refused: no user code runs outside macros | `config_file.rs` `NO_EQUIVALENT` |
| Externs | needed for interop under `:advanced`, inferred | none needed and none accepted: host-facing property names are never renamed | `config_file.rs` `externs`/`infer-externs` refused; docs/advanced-optimization.md principle 2 |
| Property renaming | Closure renames eligible unquoted properties, keeping externed and quoted names | host-facing names never; the compiler's own arity and protocol property names are kept in output today | [breakdown][f979] |
| `:closure-defines` / `goog-define` | dev and release | release only; an error in dev, permanently | `config_file.rs` `release_only` |
| Test targets (`:browser-test`, `:node-test`, `:karma`) | yes, with `cljs.test` | none, and `cljs.test` is not bundled; re-frame's suite ran by vendoring upstream `cljs.test` with 12 edits and a generated runner | `config_file.rs` `:target` (`:esm` only); `src/compiler_libraries` |
| Node targets (`:node-script`, `:node-library`, `:npm-module`, `:esm`) | yes | `:target :esm` only; the ES module output runs in Node (the generated projects run there), and the dev build and REPL run in Node; no CommonJS or library output with named exports | `config_file.rs` `Setting::Target`; `tests/dev_repl.rs` |
| Closure Library (`goog.*`) | bundled | none, a non-goal; ports carry small shims | README "Status" |
| Persistent compiler cache | per build and mode | one content-addressed store shared by builds, with declaration views for source-macro users | docs/compiler-cache.md |

## What is unmeasured

- **Anything on a reserved machine**, and anything on Linux or Windows.
- **The re-frame examples' build times with the real cljs.spec.alpha.** The
  timed ports had the stub ([build time][ftime]); light-cljs then took 0.17 to
  1.2 s against shadow-cljs's 11.6 to 17.4 s, mostly its JVM startup.
- **Output speed of simple and flow**, and of TodoMVC on any browser but
  Chromium.
- **shadow-cljs's watch with a faster file watcher** (a lower `:fs-watch
  {:loop-wait N}`), light-cljs's watch with `:reload-strategy :full`, and
  either watch on a plain generated project or at 800k.
- **Upstream ClojureScript's test suites run whole** against light-cljs's
  runtime; only adapted cases and an execution comparison exist.
- **Output speed or compile speed on a real app larger than flow.**
- **brotli and language level on the generated projects**, and debugging
  quality of source maps.
- **flow's size after light-cljs-985 and -984**, which moved the other two
  apps.
- **One binary against every comparison.** The growth run is the only one
  that held the binary fixed across sizes and shapes.
- **A rebuild after a one-file edit against shadow-cljs since light-cljs-347
  and -997**: the latest light-cljs edit figures were measured light-only and
  set here against shadow-cljs's figures from the growth run.

## Non-goals

- **Compiling unmodified third-party shadow-cljs projects.** light-cljs reads
  no `deps.edn`, resolves no dependencies, and refuses shadow-cljs build keys it
  has no counterpart for. The supported route is the porter and its shims, and
  the port inventory lists every edit. This was not measured and no work is
  filed toward it.
- **Closure Library.** light-cljs will not support `goog.*`. A gap only Closure
  Library would close is recorded as a non-goal; ports shim what they use.

## Penpot

Penpot was the real-world comparison the critique attacked. By the user's
decision it is now optional, and this group did not re-measure it against
shadow-cljs, so no claim here rests on it. One cache figure was taken on it:
the declaration views of light-cljs-347 cut a leaf edit's recompiled namespaces
from 153 to 2 (docs/compiler-cache.md).

## Independent check

A reviewer with no knowledge of light-cljs was given this document up to this
section, without the link targets, and asked for counter-arguments and what
would convince it (November 14, 2026). Its verdict: the document is "unusually
candid about where light-cljs loses", the heap sweep is "sound method", and it
would believe that release compiles are much faster and the edit loop snappier
at default settings, but would not switch while the output runs 3.5 to 7 times
slower and a project needs a porter. Its main objections, most damaging first,
with our answers:

1. **"The output is 3.5 to 7 times slower, but compile speed leads the
   summary"; and is the replacement runtime as correct as it is slow?**
   Agreed on emphasis: the summary now opens with the runtime loss. On
   correctness, beyond the three apps and re-frame's 118 tests, the test suite
   carries several hundred cases adapted from upstream ClojureScript's own test
   files (tests/reference/fixture-origins.tsv), and an execution comparison
   runs programs through ClojureScript 1.12.134 and light-cljs in Node and
   compares values, effects and output (tests/reference/README.md). Upstream's
   test suites were not run whole against the runtime, and output speed was not
   measured on a real app larger than TodoMVC. Both stay open.
2. **"The loaded machine can hurt the JVM more than light-cljs"**, shown by
   shadow-cljs's system CPU exceeding its user CPU. Possible, and not ruled
   out. The pattern is there at the lowest loads recorded too: at re-frame
   400k, with a load of 5 to 7.5 before every build, shadow-cljs used 849 s of system
   CPU against 274 s of user CPU ([growth][g970]). Whether that is normal for
   shadow-cljs on this virtual machine or contention is unmeasured; a quiet
   rerun is the first item of "What is unmeasured".
3. **"The watch comparison mostly measures watcher polling";** shadow-cljs's
   `:fs-watch {:loop-wait N}` was never lowered. Correct, and now said under
   objection 2. Each tool's own compile step, which excludes noticing, still
   differs on the 200k app: 13 ms against 770 ms for a view and 6 ms against
   3,475 ms for `workload.common`, while the macro edit's steps are close (165
   to 174 ms against 280 to 300 ms) and TodoMVC's widely required edit is a
   tie. The random pause was introduced after the fixed pause was seen to put
   every save at the same point of the poll period, and is reported beside it.
4. **"The hot-reload work is not the same"**: light-cljs reloads one
   namespace where shadow-cljs recompiles and reloads its 206 dependents.
   That is the design difference, and it is what a developer waits for. The
   probes check the result where they look: the page calls the changed
   function as the page holds it, and for the macro edit both tools' logs show
   the macro namespace and its 8 users recompiled, while the probe runs one of
   those users (`workload.breisrei/dokgem`) and turns true only once its
   expansion carries the new constant. The other 7 users' reloaded code is not
   executed by the probe. Dependents are
   not reloaded by default because their calls look the definition up when
   they run (docs/dev-workflow.md); a dependent whose top-level effects must
   run again needs `:reload-strategy :full`, as in shadow-cljs. That strategy
   was not timed; added to "What is unmeasured".
5. **"All compile-scale evidence comes from the authors' own generator,"** and
   the compiler was tuned on it. True: the four growth terms were found on
   these projects. They were general costs (scans of earlier namespaces and
   tables), each fixed with byte-identical output, and the generator's
   library code in the re-frame shape is real. A mid-sized real app timed at
   scale does not exist in this evidence; Penpot was that app, and the user
   chose not to rely on it.
6. **"The release comparisons may not cover the same work"**, and "rebuild,
   nothing changed" is a workflow no shadow-cljs user runs. The work each
   figure includes is now stated under the headline table: no bundling on
   either side in the generated projects, esbuild timed separately in the
   examples, no lowering by either. The cached release rebuild is a CI-style
   case, reported because both tools keep a cache there; the developer's loop
   is objection 2.
7. **"Ratios are stitched across commits and runs."** True of several rows,
   each labelled. The November 13 baseline rows are one binary in one
   alternated session; the growth run is one binary across ten projects. A
   final single-binary rerun of every row was not done.
8. **"The real-app evidence comes from ports"**, with spliced expansions,
   vendored `cljs.test` and vendored libraries. Agreed, and it is why
   compiling unmodified projects is stated as a non-goal rather than claimed.
   "Real apps work" means the ports' output runs and passes the same checks.

It also found two inconsistencies, both fixed: the plain-shape rebuild ratio
was missing beside the re-frame one, and this section was referred to before
it existed. Its point that the summary's 28 s has no shadow-cljs figure
measured alongside it is answered in the text: the ratios in the headline
table are from the alternated `627b9ed5` runs only.
