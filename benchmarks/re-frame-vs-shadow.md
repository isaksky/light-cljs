# re-frame's example apps: light-cljs and shadow-cljs like for like

This answers "the real-world comparison is not the same program" (shadow-comparison-credibility group, light-cljs-971) with re-frame's own example apps instead of Penpot:

- **shadow-cljs** builds the unmodified upstream examples in `research/re-frame/examples`. Only the shadow-cljs pin changed, from 3.2.0 to 3.5.3.
- **light-cljs** builds the porter's ports of the same examples.
- Every port edit is listed below by cause.

Compiling unmodified third-party shadow-cljs projects is not a light-cljs goal. The porter and its shims are the supported route, so the inventory says exactly what the port changes.

Everything here is rerun with `benchmarks/re-frame.bb`:

```sh
bb benchmarks/re-frame.bb setup                 # pin shadow-cljs 3.5.3, npm install, esbuild
bb benchmarks/re-frame.bb port todomvc simple flow --porter research/shadow-credibility/shadow-port-tool-982
bb benchmarks/re-frame.bb size todomvc simple flow --light BIN
bb benchmarks/re-frame.bb origins todomvc simple flow --light BIN
bb benchmarks/re-frame.bb time todomvc simple flow --runs 5 --light BIN
bb benchmarks/re-frame.bb inventory todomvc simple flow --light BIN
```

## 2026-11-13: TodoMVC, simple and flow

### Setup

- **light-cljs:** an immutable copy of the release binary at `ad4da290`, which is master `627b9ed5` plus the four fixes below (`research/shadow-credibility/bin/light-cljs-ad4da290`). V8 build.
- **shadow-cljs:** 3.5.3 with Closure Compiler v20260907, run as `npx shadow-cljs release client`.
- **Other tools:** esbuild 0.25.10, React 18.3.1, reagent 1.2.0, re-frame at `research/re-frame`, Node 26.12.1, OpenJDK 25.0.2.
- **Machine:** macOS, 24 cores, 32 GB, unreserved. Another worker's builds ran throughout, with load averages of 10 to 20 (recorded per run below).

**Ports.**
- **TodoMVC** still ports on master, and re-running the porter reproduces the existing `research/todomvc-port` source for source. Its dev build works on master `627b9ed5`, but its release build panics there; the first fix below is needed for it.
- **simple** ports and builds the same way. Its port is in `research/shadow-credibility/simple-port`.
- **flow** ports and builds once the porter's catalogue handles two JVM-only sci macros (below). Its demo pretty-prints with zprint, which brings sci, rewrite-clj and edamame: 139 vendored files against simple's 30. Its port is in `research/shadow-credibility/flow-port`.

**Compiler fixes made for these ports**, each a general semantics fix with a regression test:

| Commit | Gap | Found in |
|---|---|---|
| Keep and/or a statement when its other operand recurs | At `:advanced`, `(loop [...] (or a (if c (recur ...) false)))` panicked the emitter ("recur in value position"). The `||` short circuit took an operand holding the loop's `recur`. **TodoMVC's release build panicked on master `627b9ed5`**; only dev builds had been tried. light-cljs-977 landed the same fix on master as `4d2669bc`, so this branch keeps master's and adds only a second regression test. | reagent.ratom `arr-eq` |
| Accept vector clauses in ns forms | `(ns a #?@(:cljs [[:require-macros ...]]))` splices a vector clause; upstream destructures either shape | zprint |
| Ignore metadata on ns clauses | `(ns a "doc" ^{:added "0.4.0"} (:refer-clojure ...))` | rewrite-clj |
| Add cljs.reader's private read-date, read-queue and read-uuid | upstream cljs.tagged-literals calls `#'reader/read-date` | cljs.tagged-literals, through sci |

**Porter fixes** (research tool outside git; the patched copy is `research/shadow-credibility/shadow-port-tool-971`, used with `port --porter`):
- vector ns clauses;
- reader conditionals inside a libspec (`[x #?@(:cljs [:include-macros true])]`);
- unwrapping every `(deftime (defmacro ...))`. light-cljs defines macros in the namespace itself and never loads a separate `ns$macros`, so macrovich-style macro definitions are always needed;
- a catalogue entry for sci 0.3.4's two JVM-only macros, `require-cljs-analyzer-api` and `copy-ns` (see "Port edits by cause"). It is port data, not a compiler feature.

## Release size

`bb benchmarks/re-frame.bb size`: one clean release build per compiler, in bytes.

**What each row measures:**
- **Packages.** light-cljs writes ES modules that import `"react"` and `"react-dom/client"`. esbuild bundles and minifies them with React, which is what a user ships. shadow-cljs bundles React itself, through Closure, into one file. Both builds include the same seven npm files (react, react-dom, react-dom/client, scheduler); the script checks this.
- **React alone** is each toolchain building only those two package imports: a shadow-cljs namespace that requires them, and an esbuild module that imports them.
- **App code** is the whole file minus React alone. For compressed sizes this difference is an estimate.

**TodoMVC**

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 500,589 | 137,094 | 114,582 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 357,229 | 90,003 | 73,973 |
| light-cljs + esbuild: whole bundle | 441,802 | 137,694 | 119,551 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 299,417 | 92,077 | 79,632 |
| light-cljs output as written, without React | 378,326 | 92,798 | 79,145 |
| light-cljs + esbuild --target=es2015: whole bundle | 452,520 | 140,525 | 121,850 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**simple**

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 372,279 | 101,065 | 85,573 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 228,919 | 53,974 | 44,964 |
| light-cljs + esbuild: whole bundle | 366,722 | 114,709 | 99,578 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 224,337 | 69,092 | 59,659 |
| light-cljs output as written, without React | 239,419 | 67,812 | 58,169 |
| light-cljs + esbuild --target=es2015: whole bundle | 376,232 | 117,164 | 101,600 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**flow** (zprint, sci, rewrite-clj and edamame included; spec is a stub in the port, see below)

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 1,557,351 | 358,599 | 284,287 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 1,413,991 | 311,508 | 243,678 |
| light-cljs + esbuild: whole bundle | 1,278,131 | 330,726 | 272,246 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 1,135,746 | 285,109 | 232,327 |
| light-cljs output as written, without React | 1,453,360 | 288,184 | 228,264 |
| light-cljs + esbuild --target=es2015: whole bundle | 1,291,261 | 334,209 | 274,802 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**Findings:**
- **TodoMVC, whole file after gzip:** the two are level, 137.7 KB against 137.1 KB.
- **TodoMVC app code:** light-cljs is 2% larger after gzip and 8% larger after brotli.
- **simple, the smaller app:** light-cljs's app code is 28% larger after gzip and 33% after brotli; the whole file is 13% larger after gzip. That is a loss, recorded as found; light-cljs-979 breaks it down.
- **flow, the largest app:** light-cljs's app code is 8.5% smaller after gzip and 4.7% after brotli; the whole file is 7.8% smaller after gzip. **Do not read that lead as the compiler's: the two builds validate differently.** zprint validates its options with spec, and shadow's output carries 128,702 characters of it (zprint/spec.cljc and cljs.spec; "Where the bytes come from"), against 1,111 for the port's stub. A rough estimate at shadow's average gzip ratio for app code (0.22) puts that near 28 KB, the same size as light-cljs's 26 KB gzip lead. Spec definitions may compress better or worse than the average, so the stub's real compressed effect is unmeasured, and flow's like-for-like result is open until light-cljs-980 measures it; flow is the app where it matters most.
- **Before compression** light-cljs's app code is smaller in all three apps.
- **React** comes out smaller through esbuild than through Closure's simple optimizations: 0.7% raw, 3.1% after gzip and 1.7% after brotli.

**Not minified by light-cljs alone.** esbuild's `--minify` shrinks light-cljs's advanced output from 378,326 to about 299,400 bytes for TodoMVC. After gzip the difference is under 1%, so it is mostly whitespace and names that compress well.

**JavaScript language level.**
- **shadow-cljs:** 3.5.3's default `:output-feature-set` for `:browser` is `:es-unsupported`, so nothing is lowered. Its output is ES2015-level anyway: esbuild with `--target=es2015` leaves it unchanged.
- **light-cljs:** it writes modern JavaScript and does not lower it. TodoMVC has 280 `?.` and 147 `??` operators, both ES2020, plus constructs newer than ES2020.
- **Effect of matching:** the last table row lowers light-cljs's bundle to ES2015 with esbuild `--target=es2015`, which matches shadow's level. That costs 2.1% after gzip in TodoMVC and simple (TodoMVC 137,694 to 140,525 bytes) and 1.1% in flow, and a user who needs older browsers pays it. At matched language level, TodoMVC's whole file is 2.5% larger than shadow's after gzip.

shadow-cljs's output differs by a few bytes from one clean build to the next (500,588 and 500,589 bytes raw; brotli 114,541 and 114,582). light-cljs's output was byte-identical across runs.

**Property renaming.**
- **shadow-cljs:** Closure `:advanced` renames properties. No `cljs$core$...` protocol or arity property names are left in its output.
- **light-cljs:** it does not rename host-facing names, by design (docs/advanced-optimization.md). Its TodoMVC output keeps 34 `$IFn$_invoke$arity$N`-style property names and the protocol method names.
- This accounts for part of the per-library growth in "Where the bytes come from".

### Where the bytes come from

`bb benchmarks/re-frame.bb origins` attributes each output character to a source, using each compiler's source map. Both builds are made with source maps, at the same optimization level as above, and attribution is by segment. The table shows the larger groups for TodoMVC.

| Origin | shadow-cljs | light-cljs |
|---|---:|---:|
| cljs.core (shadow) / light-cljs runtime | 196,560 | 164,527 |
| React (npm) | 141,871 | 0 (bundled later by esbuild) |
| no source in the map | 1,520 | 80,313 |
| cljs.spec (shadow) / compat stub (light-cljs) | 28,383 | 116 |
| reagent/impl | 17,110 | 26,445 |
| Closure Library (shadow) / compat `goog` shims (light-cljs) | 25,194 | 5,578 |
| cljs.tools.reader (shadow) / light-cljs's built-in reader | 17,045 | 0 |
| reagent/ratom.cljs | 9,350 | 14,935 |
| re_frame/router.cljc | 3,423 | 7,545 |
| re_frame/trace.cljc | 4,334 | 7,155 |
| re_frame/flow | 5,747 | 6,355 |
| total | 499,520 | 377,410 |

For simple: shadow's cljs.core is 154,084 characters and light-cljs's runtime 124,546. reagent/impl is 17,091 against 25,558, re_frame/subs.cljc 4,973 against 10,556, Closure Library 9,235 against 3,707 of shims.

For flow, the larger groups:

| Origin | shadow-cljs | light-cljs |
|---|---:|---:|
| sci/impl | 552,543 | 474,563 |
| cljs.core (shadow) / light-cljs runtime (`js_runtime.js`) | 296,692 | 195,383 |
| no source in the map | 3,447 | 336,933 |
| React (npm) | 141,872 | 0 (bundled later by esbuild) |
| zprint/zprint.cljc | 101,797 | 94,716 |
| zprint/spec.cljc (spec definitions; the port's are stubbed) | 100,311 | 962 |
| rewrite_clj/node | 60,720 | 37,664 |
| edamame/impl | 33,118 | 22,664 |
| zprint/config.cljc | 30,805 | 28,179 |
| cljs.spec (shadow) / compat stub (light-cljs) | 28,391 | 149 |
| cljs.tools.reader | 23,632 | 27,664 |
| clojure.string, set, walk, cljs.reader (shadow) / light-cljs's bundled namespaces | 12,587 | 22,873 |
| Closure Library and shadow's runtime (shadow) / compat `goog` shims (light-cljs) | 23,975 | 6,235 |
| reagent/impl | 17,666 | 26,578 |
| total | 1,554,035 | 1,451,506 |

In flow most of the code is sci and zprint, which are plain ClojureScript with few host-facing names, and light-cljs's output of them is smaller before compression (sci/impl 14% smaller). light-cljs's source map leaves 336,933 characters without a source (23% of its output), so each light-cljs row above undercounts by an unknown share and only the totals compare exactly.

**What the attribution shows:**
- **reagent and re-frame code is larger in light-cljs.** It is 1.3 to 2 times larger before compression. The cause is kept property names and fewer inlined calls. flow's sci, rewrite-clj and edamame code goes the other way: smaller in light-cljs.
- **The runtime and the shims are smaller,** and that difference is what keeps TodoMVC level overall.
- **Part of TodoMVC and flow is not the same program,** as the next section explains: spec is a stub in the port, and in flow it is 8% of shadow's output.

## Build time

`bb benchmarks/re-frame.bb time --runs 5`:
- **Clean builds:** shadow-cljs's `.shadow-cljs/builds` cache is deleted, and light-cljs runs with `--no-cache`.
- **Order:** the two compilers alternate, the first one changing each run.
- **Measures:** medians with the range in parentheses, wall, user and system CPU time, and peak memory, all from `/usr/bin/time -l`, plus the one-minute load average before each run.
- **Machine:** unreserved, and another worker's builds ran throughout. These are small-app numbers.

**TodoMVC**, 5 runs each

| Build | Wall s | User CPU s | System CPU s | Peak memory MB | Load average before |
|---|---:|---:|---:|---:|---:|
| shadow-cljs release (npx, JVM start included) | 13.18 (11.91-20.97) | 56.51 (51.90-58.73) | 8.84 (8.17-24.39) | 1586.27 (1291.01-1801.62) | 17.92 (10.85-19.71) |
| light-cljs release | 0.29 (0.23-0.30) | 0.23 (0.22-0.26) | 0.06 (0.02-0.20) | 95.86 (93.91-98.39) | 15.44 (13.44-19.70) |
| esbuild bundle of light-cljs's output | 0.06 (0.04-0.11) | 0.03 (0.03-0.04) | 0.01 (0.00-0.01) | 43.88 (43.29-45.97) | 15.44 (13.44-19.70) |

**simple**, 5 runs each

| Build | Wall s | User CPU s | System CPU s | Peak memory MB | Load average before |
|---|---:|---:|---:|---:|---:|
| shadow-cljs release (npx, JVM start included) | 11.60 (10.54-22.18) | 53.22 (48.95-55.73) | 7.62 (5.85-18.72) | 1333.94 (1245.00-1560.23) | 17.86 (14.81-20.95) |
| light-cljs release | 0.17 (0.17-0.19) | 0.18 (0.18-0.19) | 0.02 (0.02-0.02) | 88.26 (86.59-89.13) | 19.06 (14.81-20.69) |
| esbuild bundle of light-cljs's output | 0.03 (0.03-0.03) | 0.03 (0.03-0.03) | 0.00 (0.00-0.00) | 39.16 (38.83-39.32) | 19.06 (14.81-20.69) |

**flow**, 5 runs each (measured later the same evening; another worker's timing runs were active, load averages 7.7 to 11.1)

| Build | Wall s | User CPU s | System CPU s | Peak memory MB | Load average before |
|---|---:|---:|---:|---:|---:|
| shadow-cljs release (npx, JVM start included) | 17.42 (16.64-19.09) | 76.26 (74.79-77.70) | 17.33 (14.43-24.50) | 1833.12 (1807.01-2319.68) | 7.85 (7.71-9.17) |
| light-cljs release | 1.21 (1.17-1.28) | 1.26 (1.22-1.34) | 0.09 (0.08-0.12) | 336.10 (320.95-339.72) | 9.79 (7.85-11.13) |
| esbuild bundle of light-cljs's output | 0.11 (0.11-0.14) | 0.11 (0.11-0.11) | 0.01 (0.00-0.02) | 97.03 (96.93-99.86) | 9.17 (7.85-11.13) |

**JVM start-up dominates at this size.** In an earlier single run shadow-cljs reported "Build completed ... 6.72 s" of a 13.1 s wall time. The rest is starting `npx`, the Clojure CLI and the JVM and loading shadow-cljs and Closure. These apps are far too small to say anything about compile speed or memory at scale; the generated projects answer that (light-cljs-970, 977).

light-cljs's own release takes about a quarter of a second for TodoMVC and simple and 1.2 s for flow, whose 139 files include all of sci. Bundling with esbuild adds a few tens of milliseconds (0.1 s for flow), listed as its own row.

**flow is where JVM start-up stops dominating.** In a single check run shadow-cljs reported "Build completed. (224 files, 151 compiled, 0 warnings, 14.03s)" of an 18.4 s wall time, so most of its time there is the build itself, and light-cljs is about 14 times faster on wall time and 70 times on user plus system CPU time (medians). It is still one small app; the generated projects are the scale evidence.

## Port edits by cause

`bb benchmarks/re-frame.bb inventory` lists the port's `compat/` files and the porter's report (port-report.edn).

**compat/ is the porter's whole compat layer.** The porter copies all 34 files into every port, though most are never reached:
- TodoMVC's output reaches 7: `cljs/spec/alpha`, `goog`, `goog/events`, `goog/functions`, `goog/history/EventType`, `goog/object` and `light/compat`.
- simple's reaches 5: the same without spec and `goog/history/EventType`.
- flow's reaches 8: `cljs/spec/alpha`, `goog`, `goog/array`, `goog/events`, `goog/functions`, `goog/object`, `goog/string` and `light/compat`.

Unreached shims are not in any output and change nothing. The sizes below are output characters by source map, from the TodoMVC build unless marked.

| Edit | Cause | Apps | light-cljs output | What shadow-cljs ships instead |
|---|---|---|---:|---:|
| Release panic on `(or a (if c (recur) false))` | compiler gap, fixed on master (`4d2669bc`) | TodoMVC, simple | none | none |
| ns clause as a vector, metadata on an ns clause, `#'cljs.reader/read-date` | compiler gaps, fixed in this branch | flow | none | none |
| `light/compat.cljs`: `cljs.core/pr-writer`, which reagent calls; the measured port rewrites references to it (catalogue `:core-gaps`) | compiler gap, fixed on master (light-cljs-981): `cljs.core/pr-writer` is defined, so the shim is no longer needed. The porter `research/shadow-credibility/shadow-port-tool-981` has `:core-gaps {}`, and TodoMVC ported with it builds in dev and release | all | 108 (simple: 10, flow: 149) in the measured build; none with the 981 porter | part of cljs.core |
| `compat/cljs/spec/alpha.cljs`: `s/def` expands to nil and `s/valid?` returns true, so TodoMVC's `check-spec-interceptor` never fails | JVM-only macro: upstream spec's macros resolve through the JVM analyzer | TodoMVC | 116 | 28,383 (cljs.spec), which really validates the db: light-cljs-980 |
| The same spec stub in flow: zprint defines its options with 254 `s/def`s and checks them with `s/valid?` and `s/explain-data`, which the stub makes always pass | JVM-only macro, as above | flow | 1,111 (stub 149, zprint/spec.cljc 962) | 128,702 (zprint/spec.cljc 100,311, cljs.spec 28,391), 8% of shadow's output: light-cljs-980 |
| sci 0.3.4 `require-cljs-analyzer-api`: on the JVM it requires `cljs.analyzer.api` and defines a Var, so its expansion has no source form. The catalogue writes `nil` at its one call site, which is what upstream expands it to under self-hosted ClojureScript | JVM-only macro | flow | none | none (a JVM-side definition) |
| sci 0.3.4 `copy-ns`: its self-hosted branch reads `cljs.analyzer.api/ns-publics`, which light-cljs does not provide and refuses to resolve in the macro's body, where upstream only warns. The catalogue makes that branch throw a clear error; nothing in flow calls `copy-ns` | JVM-only macro | flow | none (macro code) | none |
| 13 sci macros (`sci.impl.faster/nth-2`, `sci.impl.types/->Node`, `sci.impl.utils/kw-identical?`, `sci.impl.fns/gen-fn` and 9 more), 97 call sites expanded on the JVM and spliced | other: they read `&env` through sci's macrovich-style `?`, and the porter expands such macros on the JVM instead of running them in light-cljs. Not shown to be a light-cljs gap | flow | same code as shadow's expansion | the same expansion |
| tools.reader 1.6.0 `impl/inspect.cljs`: the measured port removes `inspect*` methods on `cljs.core/PersistentArrayMapSeq` and `cljs.core/NodeSeq` | compiler gap, fixed on master (light-cljs-982): both names resolve to the types of a map seq. The porter `research/shadow-credibility/shadow-port-tool-982` (971's without this catalogue entry) ports flow with the file unmodified, and its light-cljs release builds | flow | two one-line methods fewer in the measured build; none with the 982 porter | two methods that only name a value in an error message |
| `secretary.core/defroute`, 2 call sites expanded on the JVM and spliced into `todomvc/core.cljs` | JVM-only macro (an `IllegalArgumentException.` branch) | TodoMVC | same code as shadow's expansion | the same expansion |
| `goog.cljs` (`goog.History`, `getUid`), `goog/events.cljs` (listen, BrowserEvent), `goog/functions.cljs`, `goog/history/EventType.cljs`, `goog/object.cljs`. The porter's report lists `goog.History` and `goog.async.nextTick` as `:missing-goog`, added to the compat layer by hand | Closure Library use: a non-goal, never compiler work | TodoMVC (simple: no History) | 5,578 (simple: 3,707) | 25,194 of Closure Library (simple: 9,235) |
| flow: the same shims plus `goog/array.cljs` and `goog/string.cljs` (`StringBuffer`, `format`, which rewrite-clj uses). Its report lists `goog.async.nextTick`, `goog.string.StringBuffer` and `goog.string.format` as `:missing-goog`; the compat layer already has all three | Closure Library use: a non-goal | flow | 6,490 | 11,894 of Closure Library |
| `:target :browser` becomes `:esm`, bundled by esbuild | shadow-cljs build feature | all | the esbuild wrapper | a self-contained script |
| `:modules {:client {:init-fn todomvc.core/main}}`: light-cljs has no `:init-fn`. `main` is the module's default export, and the page must call it | shadow-cljs build feature | all | none | the call to `main` |
| `:release {:build-options {:ns-aliases {day8.re-frame.tracing day8.re-frame.tracing-stubs}}}` dropped; TodoMVC requires neither namespace, so nothing changes | shadow-cljs build feature | TodoMVC | none | none |
| `:dev {:compiler-options {:closure-defines ...}}` dropped (light-cljs refuses dev-mode defines); `:dev-http` and `:nrepl` translated | shadow-cljs build feature, dev only | all | none in a release build | none in a release build |
| `:devtools {:preloads [hashp.core]}` dropped: light-cljs has no preloads | shadow-cljs build feature, dev only | flow | none in a release build | none in a release build |
| `:deps true`: the porter vendors the library sources the build reached into `vendor/` (reagent, secretary, clojure.data, macrovich; flow also zprint, sci, rewrite-clj, edamame, tools.reader) | other: light-cljs resolves no dependencies | all | none | none |

**Compiler-gap entries still open on master** after this branch: none; `cljs.core/pr-writer` (light-cljs-981) and `cljs.core/PersistentArrayMapSeq` (light-cljs-982) were fixed afterwards. The three ns-form and reader gaps and the release panic are fixed. The sci entries are JVM-only macros, port data rather than compiler work. Closure Library entries are recorded as non-goals, not work.

### Full inventory

`bb benchmarks/re-frame.bb inventory todomvc simple flow` lists every `compat/` file with its cause and the characters it puts in the source-mapped light-cljs release; 0 means the file is not reached. It then lists every macro, configuration key and report entry from `port-report.edn`. Macros classed `keep` run unchanged in light-cljs.

#### todomvc: every compat/ file

| File | Cause | Source bytes | light-cljs output characters |
|---|---|---:|---:|
| cljs/pprint.cljs | other: development library replaced by a stub | 837 | 0 |
| cljs/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 605 | 116 |
| cljs/test.cljs | JVM-only macro: cljs.test stub | 308 | 0 |
| clojure/math.cljs | other: a library light-cljs does not ship, written over js/Math | 1,019 | 0 |
| clojure/pprint.cljs | other: development library replaced by a stub | 840 | 0 |
| clojure/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 608 | 0 |
| clojure/test/check.cljs | JVM-only macro: test.check stub | 168 | 0 |
| clojure/test/check/generators.cljs | JVM-only macro: test.check stub | 2,618 | 0 |
| clojure/test/check/properties.cljs | JVM-only macro: test.check stub | 151 | 0 |
| expound/alpha.cljs | JVM-only macro: stub of a spec library | 131 | 0 |
| goog.cljs | Closure Library use (non-goal) | 2,209 | 1,391 |
| goog/array.cljs | Closure Library use (non-goal) | 665 | 0 |
| goog/async.cljs | Closure Library use (non-goal) | 114 | 0 |
| goog/crypt.cljs | Closure Library use (non-goal) | 237 | 0 |
| goog/date/Date.cljs | Closure Library use (non-goal) | 86 | 0 |
| goog/date/UtcDateTime.cljs | Closure Library use (non-goal) | 181 | 0 |
| goog/dom.cljs | Closure Library use (non-goal) | 410 | 0 |
| goog/events.cljs | Closure Library use (non-goal) | 2,568 | 1,858 |
| goog/events/EventType.cljs | Closure Library use (non-goal) | 559 | 0 |
| goog/functions.cljs | Closure Library use (non-goal) | 1,075 | 595 |
| goog/history/EventType.cljs | Closure Library use (non-goal) | 113 | 174 |
| goog/i18n/uChar.cljs | Closure Library use (non-goal) | 1,131 | 0 |
| goog/math.cljs | Closure Library use (non-goal) | 341 | 0 |
| goog/net.cljs | Closure Library use (non-goal) | 1,602 | 0 |
| goog/net/WebSocket/EventType.cljs | Closure Library use (non-goal) | 122 | 0 |
| goog/object.cljs | Closure Library use (non-goal) | 775 | 1,560 |
| goog/string.cljs | Closure Library use (non-goal) | 2,082 | 0 |
| light/compat.cljs | compiler gap: cljs.core names light-cljs lacks (catalogue :core-gaps) | 389 | 108 |
| malli/generator.cljs | other: development library replaced by a stub | 545 | 0 |
| malli/sci.cljs | other: development library replaced by a stub | 202 | 0 |
| me/flowthing/pp.cljs | other: development library replaced by a stub | 229 | 0 |
| shadow/esm.cljs | shadow-cljs build feature | 596 | 0 |
| shadow/lazy.cljs | shadow-cljs build feature | 280 | 0 |
| shadow/resource.cljs | shadow-cljs build feature | 231 | 0 |

#### todomvc: every macro the port classified

| Macro | Class | Call sites | Reason |
|---|---|---:|---|
| reagent.debug/assert-callable | keep | 6 |  |
| cljs.spec.alpha/def | port | 7 | catalogue or compat port |
| re-frame.trace/finish-trace | keep | 17 |  |
| cljs.spec.alpha/and | port | 1 | catalogue or compat port |
| cljs.spec.alpha/every | port | 1 | catalogue or compat port |
| secretary.core/defroute | expand | 2 | secretary.core/defroute uses interop IllegalArgumentException. |
| reagent.debug/assert-component | keep | 7 |  |
| cljs.spec.alpha/every-kv | port | 1 | catalogue or compat port |
| reagent.debug/assert-js-object | keep | 3 |  |
| cljs.spec.alpha/tuple | port | 1 | catalogue or compat port |
| net.cgrand.macrovich/deftime | keep | 1 |  |
| reagent.core/reaction | keep | 1 |  |
| cljs.spec.alpha/keys | port | 2 | catalogue or compat port |
| cljs.spec.alpha/map-of | port | 1 | catalogue or compat port |
| reagent.debug/warn | keep | 6 |  |
| reagent.debug/dev? | keep | 6 |  |
| re-frame.trace/with-trace | keep | 17 |  |
| reagent.debug/warn-unless | keep | 1 |  |
| re-frame.trace/merge-trace! | keep | 35 |  |
| reagent.debug/assert-new-state | keep | 2 |  |
| reagent.debug/assert-some | keep | 3 |  |

#### todomvc: every configuration key

- [:nrepl] translated: an nREPL listener on its own port; :host and middleware dropped (sessions are ClojureScript)
- [:deps] translated: light-cljs resolves no dependencies: the porter vendored the reached library sources into vendor/
- [:dev-http] needs-feature: a static root served at /; the dev server serves output dirs and source paths
- [:builds :client :target] needs-feature: :browser output: light-cljs writes ES modules only; translated to :esm
- [:builds :client :output-dir] translated: rebased onto the original project, where its pages load the output
- [:builds :client :modules :client :init-fn] needs-feature: no :init-fn; a generated entry namespace can call it
- [:builds :client :modules] needs-feature: one module with one entry; the other entries become light.lazy/loadable chunks only where the code loads them; several entries and named chunk groups are code-splitting Phase 4
- [:builds :client :compiler-options :closure-defines] kept: --define, release only
- [:builds :client :dev :closure-defines] dropped: light-cljs refuses :closure-defines in dev mode (a permanent deviation); turn the flags into :config or code
- [:builds :client :build-options :ns-aliases] needs-feature: no namespace aliasing in the compiler

Report: {:files 38, :spliced 2, :patches 0, :missing-goog (goog.History goog.async.nextTick), :unprintable {}, :unsafe {}, :splice-mismatch [], :edits-dropped []}

#### simple: every compat/ file

| File | Cause | Source bytes | light-cljs output characters |
|---|---|---:|---:|
| cljs/pprint.cljs | other: development library replaced by a stub | 837 | 0 |
| cljs/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 605 | 0 |
| cljs/test.cljs | JVM-only macro: cljs.test stub | 308 | 0 |
| clojure/math.cljs | other: a library light-cljs does not ship, written over js/Math | 1,019 | 0 |
| clojure/pprint.cljs | other: development library replaced by a stub | 840 | 0 |
| clojure/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 608 | 0 |
| clojure/test/check.cljs | JVM-only macro: test.check stub | 168 | 0 |
| clojure/test/check/generators.cljs | JVM-only macro: test.check stub | 2,618 | 0 |
| clojure/test/check/properties.cljs | JVM-only macro: test.check stub | 151 | 0 |
| expound/alpha.cljs | JVM-only macro: stub of a spec library | 131 | 0 |
| goog.cljs | Closure Library use (non-goal) | 2,209 | 198 |
| goog/array.cljs | Closure Library use (non-goal) | 665 | 0 |
| goog/async.cljs | Closure Library use (non-goal) | 114 | 0 |
| goog/crypt.cljs | Closure Library use (non-goal) | 237 | 0 |
| goog/date/Date.cljs | Closure Library use (non-goal) | 86 | 0 |
| goog/date/UtcDateTime.cljs | Closure Library use (non-goal) | 181 | 0 |
| goog/dom.cljs | Closure Library use (non-goal) | 410 | 0 |
| goog/events.cljs | Closure Library use (non-goal) | 2,568 | 1,756 |
| goog/events/EventType.cljs | Closure Library use (non-goal) | 559 | 0 |
| goog/functions.cljs | Closure Library use (non-goal) | 1,075 | 486 |
| goog/history/EventType.cljs | Closure Library use (non-goal) | 113 | 0 |
| goog/i18n/uChar.cljs | Closure Library use (non-goal) | 1,131 | 0 |
| goog/math.cljs | Closure Library use (non-goal) | 341 | 0 |
| goog/net.cljs | Closure Library use (non-goal) | 1,602 | 0 |
| goog/net/WebSocket/EventType.cljs | Closure Library use (non-goal) | 122 | 0 |
| goog/object.cljs | Closure Library use (non-goal) | 775 | 1,465 |
| goog/string.cljs | Closure Library use (non-goal) | 2,082 | 0 |
| light/compat.cljs | compiler gap: cljs.core names light-cljs lacks (catalogue :core-gaps) | 389 | 10 |
| malli/generator.cljs | other: development library replaced by a stub | 545 | 0 |
| malli/sci.cljs | other: development library replaced by a stub | 202 | 0 |
| me/flowthing/pp.cljs | other: development library replaced by a stub | 229 | 0 |
| shadow/esm.cljs | shadow-cljs build feature | 596 | 0 |
| shadow/lazy.cljs | shadow-cljs build feature | 280 | 0 |
| shadow/resource.cljs | shadow-cljs build feature | 231 | 0 |

#### simple: every macro the port classified

| Macro | Class | Call sites | Reason |
|---|---|---:|---|
| reagent.debug/assert-callable | keep | 6 |  |
| re-frame.trace/finish-trace | keep | 14 |  |
| reagent.debug/assert-component | keep | 7 |  |
| reagent.debug/assert-js-object | keep | 3 |  |
| net.cgrand.macrovich/deftime | keep | 1 |  |
| reagent.core/reaction | keep | 1 |  |
| reagent.debug/warn | keep | 6 |  |
| reagent.debug/dev? | keep | 6 |  |
| re-frame.trace/with-trace | keep | 14 |  |
| reagent.debug/warn-unless | keep | 1 |  |
| re-frame.trace/merge-trace! | keep | 28 |  |
| reagent.debug/assert-new-state | keep | 2 |  |
| reagent.debug/assert-some | keep | 3 |  |

#### simple: every configuration key

- [:deps] translated: light-cljs resolves no dependencies: the porter vendored the reached library sources into vendor/
- [:nrepl] translated: an nREPL listener on its own port; :host and middleware dropped (sessions are ClojureScript)
- [:dev-http] needs-feature: a static root served at /; the dev server serves output dirs and source paths
- [:builds :client :target] needs-feature: :browser output: light-cljs writes ES modules only; translated to :esm
- [:builds :client :output-dir] translated: rebased onto the original project, where its pages load the output
- [:builds :client :modules :client :init-fn] needs-feature: no :init-fn; a generated entry namespace can call it
- [:builds :client :modules] needs-feature: one module with one entry; the other entries become light.lazy/loadable chunks only where the code loads them; several entries and named chunk groups are code-splitting Phase 4

Report: {:files 30, :spliced 0, :patches 0, :missing-goog (goog.async.nextTick), :unprintable {}, :unsafe {}, :splice-mismatch [], :edits-dropped []}

#### flow: every compat/ file

| File | Cause | Source bytes | light-cljs output characters |
|---|---|---:|---:|
| cljs/pprint.cljs | other: development library replaced by a stub | 837 | 0 |
| cljs/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 605 | 149 |
| cljs/test.cljs | JVM-only macro: cljs.test stub | 308 | 0 |
| clojure/math.cljs | other: a library light-cljs does not ship, written over js/Math | 1,019 | 0 |
| clojure/pprint.cljs | other: development library replaced by a stub | 840 | 0 |
| clojure/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 608 | 0 |
| clojure/test/check.cljs | JVM-only macro: test.check stub | 168 | 0 |
| clojure/test/check/generators.cljs | JVM-only macro: test.check stub | 2,618 | 0 |
| clojure/test/check/properties.cljs | JVM-only macro: test.check stub | 151 | 0 |
| expound/alpha.cljs | JVM-only macro: stub of a spec library | 131 | 0 |
| goog.cljs | Closure Library use (non-goal) | 2,209 | 255 |
| goog/array.cljs | Closure Library use (non-goal) | 665 | 88 |
| goog/async.cljs | Closure Library use (non-goal) | 114 | 0 |
| goog/crypt.cljs | Closure Library use (non-goal) | 237 | 0 |
| goog/date/Date.cljs | Closure Library use (non-goal) | 86 | 0 |
| goog/date/UtcDateTime.cljs | Closure Library use (non-goal) | 181 | 0 |
| goog/dom.cljs | Closure Library use (non-goal) | 410 | 0 |
| goog/events.cljs | Closure Library use (non-goal) | 2,568 | 1,850 |
| goog/events/EventType.cljs | Closure Library use (non-goal) | 559 | 0 |
| goog/functions.cljs | Closure Library use (non-goal) | 1,075 | 600 |
| goog/history/EventType.cljs | Closure Library use (non-goal) | 113 | 0 |
| goog/i18n/uChar.cljs | Closure Library use (non-goal) | 1,131 | 0 |
| goog/math.cljs | Closure Library use (non-goal) | 341 | 0 |
| goog/net.cljs | Closure Library use (non-goal) | 1,602 | 0 |
| goog/net/WebSocket/EventType.cljs | Closure Library use (non-goal) | 122 | 0 |
| goog/object.cljs | Closure Library use (non-goal) | 775 | 1,877 |
| goog/string.cljs | Closure Library use (non-goal) | 2,082 | 1,820 |
| light/compat.cljs | compiler gap: cljs.core names light-cljs lacks (catalogue :core-gaps) | 389 | 149 |
| malli/generator.cljs | other: development library replaced by a stub | 545 | 0 |
| malli/sci.cljs | other: development library replaced by a stub | 202 | 0 |
| me/flowthing/pp.cljs | other: development library replaced by a stub | 229 | 0 |
| shadow/esm.cljs | shadow-cljs build feature | 596 | 0 |
| shadow/lazy.cljs | shadow-cljs build feature | 280 | 0 |
| shadow/resource.cljs | shadow-cljs build feature | 231 | 0 |

#### flow: every macro the port classified

| Macro | Class | Call sites | Reason |
|---|---|---:|---|
| reagent.debug/assert-callable | keep | 6 |  |
| zprint.macros/dbg-pr | keep | 87 |  |
| zprint.macros/dbg-print | keep | 2 |  |
| zprint.smacros/only-keys | keep | 40 |  |
| cljs.spec.alpha/def | port | 254 | catalogue or compat port |
| sci.impl.fns/gen-fn | expand | 84 | its expansion calls a macro that is expanded |
| sci.impl.analyzer/gen-return-needs-ctx-call | expand | 1 | its expansion calls a macro that is expanded |
| re-frame.trace/finish-trace | keep | 17 |  |
| zprint.macros/dbg-s-pr | keep | 7 |  |
| sci.impl.analyzer/gen-return-and | expand | 1 | its expansion calls a macro that is expanded |
| cljs.spec.alpha/coll-of | port | 25 | catalogue or compat port |
| sci.impl.namespaces/copy-core-var | keep | 356 |  |
| sci.impl.analyzer/gen-return-binding-call | expand | 1 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| cljs.spec.alpha/every | port | 71 | catalogue or compat port |
| cljs.spec.alpha/or | port | 15 | catalogue or compat port |
| sci.impl.evaluator/resolve-symbol | keep | 1 |  |
| reagent.debug/assert-component | keep | 7 |  |
| cljs.spec.alpha/every-kv | port | 46 | catalogue or compat port |
| cljs.spec.alpha/nilable | port | 49 | catalogue or compat port |
| reagent.debug/assert-js-object | keep | 3 |  |
| cljs.spec.alpha/tuple | port | 50 | catalogue or compat port |
| sci.impl.faster/nth-2 | expand | 420 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| zprint.macros/dbg-s | keep | 79 |  |
| sci.impl.vars/with-writeable-var | keep | 2 |  |
| net.cgrand.macrovich/deftime | keep | 1 |  |
| reagent.core/reaction | keep | 1 |  |
| zprint.macros/dbg | keep | 57 |  |
| sci.impl.faster/deref-1 | expand | 1 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| sci.impl.evaluator/def-fn-call | keep | 1 |  |
| cljs.spec.alpha/keys | port | 40 | catalogue or compat port |
| cljs.spec.alpha/merge | port | 40 | catalogue or compat port |
| zprint.macros/dbg-form | keep | 8 |  |
| sci.impl.analyzer/gen-return-recur | expand | 1 | its expansion calls a macro that is expanded |
| sci.impl.analyzer/gen-return-or | expand | 1 | its expansion calls a macro that is expanded |
| cljs.spec.alpha/map-of | port | 46 | catalogue or compat port |
| cljs.tools.reader.reader-types/log-source | keep | 4 |  |
| sci.core/require-cljs-analyzer-api | port | 1 | catalogue or compat port |
| reagent.debug/warn | keep | 6 |  |
| rewrite-clj.custom-zipper.switchable/defn-switchable | keep | 22 |  |
| sci.impl.analyzer/gen-return-call | expand | 1 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| reagent.debug/dev? | keep | 6 |  |
| sci.impl.types/->Node | expand | 180 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| sci.impl.namespaces/copy-var | keep | 406 |  |
| sci.impl.utils/kw-identical? | expand | 55 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| re-frame.trace/with-trace | keep | 17 |  |
| reagent.debug/warn-unless | keep | 1 |  |
| re-frame.trace/merge-trace! | keep | 35 |  |
| sci.impl.faster/get-2 | expand | 2 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| sci.impl.vars/with-bindings | keep | 5 |  |
| sci.impl.macros/deftime | keep | 17 |  |
| reagent.debug/assert-new-state | keep | 2 |  |
| sci.impl.analyzer/gen-return-do | expand | 1 | its expansion calls a macro that is expanded |
| reagent.debug/assert-some | keep | 3 |  |

#### flow: every configuration key

- [:deps] translated: light-cljs resolves no dependencies: the porter vendored the reached library sources into vendor/
- [:nrepl] translated: an nREPL listener on its own port; :host and middleware dropped (sessions are ClojureScript)
- [:dev-http] needs-feature: a static root served at /; the dev server serves output dirs and source paths
- [:builds :client :target] needs-feature: :browser output: light-cljs writes ES modules only; translated to :esm
- [:builds :client :output-dir] translated: rebased onto the original project, where its pages load the output
- [:builds :client :devtools :preloads] dropped: no preloads; require them from a dev entry
- [:builds :client :modules :client :init-fn] needs-feature: no :init-fn; a generated entry namespace can call it
- [:builds :client :modules] needs-feature: one module with one entry; the other entries become light.lazy/loadable chunks only where the code loads them; several entries and named chunk groups are code-splitting Phase 4

Report: {:files 139, :spliced 97, :patches 4, :missing-goog (goog.async.nextTick goog.string.StringBuffer goog.string.format), :unprintable {}, :unsafe {}, :splice-mismatch [], :edits-dropped []}

## re-frame's own test suite

re-frame's ClojureScript tests (`research/re-frame/test`) compiled by each compiler and run in the same headless Chromium (light-cljs-974). These are tests nobody here wrote, for the library at the centre of this comparison.

Rerun (it ports the suite into `research/shadow-credibility/re-frame-test-port` when that directory is missing; delete it to port again):

```sh
bb benchmarks/re-frame-tests.bb --light BIN --runs 3
```

### 2026-11-14: 118 of 118 tests pass on both compilers

**Setup.**
- **Suite:** re-frame at `1a1bf1df`. The 15 namespaces re-frame's own `:browser-test` build selects with `:ns-regexp "re-frame\\..*-test$"`. `re-frame.event-test` has no live tests, so 14 namespaces hold the 118 tests and 446 assertions.
- **shadow-cljs:** 3.5.3, re-frame's `:browser-test` build with its `:ns-regexp`, built with `compile`. Two changes from upstream: the binaryage/devtools preload is dropped, and the runner namespace below replaces shadow's default one. Upstream pins 3.2.0.
- **light-cljs:** an immutable copy of the release binary at `f9a21e24` (master `052ad00e` plus the deref fix below), `research/shadow-credibility/bin/light-cljs-f9a21e24`. V8 build. Two builds of the port:
  - the dev build, `light-cljs compile`;
  - the release build at `:advanced`, `light-cljs release --warnings` (see the port edits for why `--warnings`).
- **Runner:** one generated `cljs.test` runner namespace, compiled by both compilers. It records each test var's passes, failures and errors.
- **Runtime:** headless Chromium from playwright-core 1.63.0 (headless shell 1243). Each page is served from disk.
  - esbuild 0.25.12 bundles React 18.2.0 for the light-cljs dev page, and bundles the release build's ES modules.
  - Other versions: reagent 1.2.0, Node 26.12.1, OpenJDK 25.0.2.
- **Pass rule:** a test passes in a run only if it ended with no failure or error, and the run finished with no page errors. It passes for a build only if it passes in all three runs; a test whose runs disagree is flaky. The script exits with an error if any run did not finish. The machine was unreserved, but nothing here is timed.

**Reference pass list:** shadow-cljs passed all 118 tests in all three runs. Nothing is excluded.

| Namespace | Tests | shadow-cljs | light-cljs dev build | light-cljs release |
|---|---:|---:|---:|---:|
| re-frame.dispatch-source-meta-test | 5 | 5 | 5 | 5 |
| re-frame.flow.alpha-test | 5 | 5 | 5 | 5 |
| re-frame.fx-test | 2 | 2 | 2 | 2 |
| re-frame.handler-source-meta-test | 13 | 13 | 13 | 13 |
| re-frame.interceptor-test | 15 | 15 | 15 | 15 |
| re-frame.query-v-for-reaction-test | 8 | 8 | 8 | 8 |
| re-frame.registrar-test | 5 | 5 | 5 | 5 |
| re-frame.restore-test | 4 | 4 | 4 | 4 |
| re-frame.subs-test | 19 | 19 | 19 | 19 |
| re-frame.subs.alpha-test | 18 | 18 | 18 | 18 |
| re-frame.subscribe-source-meta-test | 6 | 6 | 6 | 6 |
| re-frame.tooling-test | 3 | 3 | 3 | 3 |
| re-frame.trace-test | 14 | 14 | 14 | 14 |
| re-frame.utils-test | 1 | 1 | 1 | 1 |
| **total** | **118** | **118** | **118** | **118** |
| assertions in each of the 3 runs | | 446, 446, 446 | 446, 446, 446 | 446, 446, 446 |

**Remaining failures: 0.** No test failed or flaked on either light-cljs build in the three recorded runs.

**What the run does not cover.** re-frame.core-instrumented's macros read `&env` and `*file*`, so the porter splices shadow-cljs's expansions of them in place of the calls. That is 24 call sites (`dispatch`, `subscribe`, `reg-event-*` and others) plus 40 `defalias` forms. The three source-meta namespaces therefore test light-cljs running shadow-cljs's expansion of those macros, not light-cljs expanding them. Every other macro was expanded by light-cljs, including `cljs.test`'s, re-frame.trace's and reagent's.

**Failures found on the way.** The first complete run, before the deref fix and the metadata edits, used the binary at `a02f3fe9` and passed 113 of 118 on both light-cljs builds.

| Tests | Cause | Resolution |
|---|---|---|
| subs-test/test-dynamic-subscriptions | **Compiler.** It expects `(thrown-with-msg? js/Error #"No protocol method IDeref" @x)`. light-cljs's deref of a value without `IDeref` threw "value does not support deref". | Fixed in `f9a21e24`, "Name the protocol method when deref meets a value without IDeref". Regression test in `tests/compiler/protocols.rs`. |
| dispatch-source-meta-test (2), subscribe-source-meta-test (2) | **Port.** The porter prints the spliced expansions without the reader metadata on the call's literal vector (`^:my-flag [...]`). The tests check that this metadata survives the macro. | Port edit restoring the metadata (below). With it, light-cljs's `vary-meta` keeps the user's metadata as shadow-cljs's does. |

**Port edits by cause** (all in the port's `edits.edn`, applied by `benchmarks/re-frame-tests.bb`):

- **cljs.test.** The porter's catalogue has only a stub, because apps do not run tests. The port's `compat/cljs/test.cljs` is ClojureScript's own `cljs.test` from the jar shadow-cljs uses (1.12.145): the runtime file, with the macros of `cljs/test.cljc` defined in the namespace, plus 12 edits:
  - **light-cljs syntax:** `:require-macros` is retired, and a `.cljs` file cannot hold the two reader conditionals.
  - **Compiler information light-cljs macros cannot read:**
    - The current namespace comes from `&env`'s `:ns :name`.
    - There is no `*load-tests*` switch, and no output directory option (used only in stack-trace file names).
    - `is` cannot resolve a computed symbol to see whether it names a function. So only a fixed list of `cljs.core` predicates (`=`, `nil?`, `contains?` and 30 others) gets the detailed `(not (= ...))` report; any other form is asserted as a value. Pass and fail are unchanged.
    - A namespace's test vars, fixtures and `test-ns-hook` are read at run time with `ns-interns`, in the same `:line` order, instead of from the analyzer during expansion.
    - `run-test` applies no fixtures, and `run-all-tests` is an error. The suite uses neither.
- **Closure Library gaps (non-goals, shimmed):**
  - `goog.async.nextTick`, which re-frame's router reads as a global. Same shim as `re-frame-libs.bb`.
  - The porter's `goog` shim defaults `goog.DEBUG` to false. Closure defaults it to true and shadow-cljs's `compile` keeps that, and re-frame's debug checks and traces depend on it. A light-cljs dev build takes no `--define`, so the shim's default is changed to true for both builds.
- **light-cljs semantics that differ from ClojureScript on purpose.** These are recorded, not changed: changing either is a language decision for the user.
  - `:refer` of a private Var is an error in light-cljs, as in Clojure. ClojureScript warns only where the Var is used. `interceptor-test` refers the private `re-frame.interceptor/context`, so the port reaches it through the alias instead (two edits).
  - light-cljs treats a `goog-define` Var as a constant and rejects `set!` of it at every level, as the Closure Compiler rejects reassigning a `@define` in optimized builds. The tests `set!` `re-frame.trace/trace-enabled?`, so the port defines it with `def`, keeping its default.
- **Porter output.**
  - The four metadata restorations above.
  - The porter keeps re-frame.core-instrumented's `defmacro`s after splicing their call sites. Their bodies name helpers that exist only under `:clj`, so light-cljs's strict default rejects them as unresolved. The release build therefore uses `--warnings`, the ClojureScript-compatible analysis a dev build always uses, and the macros are never called.

## 2026-11-14: Running in a browser

light-cljs-973 runs the three apps in headless Chromium, built by each compiler, with behaviour checks. This is the first time light-cljs's output of these apps has been run.

```sh
bb benchmarks/re-frame.bb setup        # also installs playwright-core 1.64.1
bb benchmarks/re-frame.bb port todomvc simple flow --porter research/shadow-credibility/shadow-port-tool-973
bb benchmarks/re-frame.bb browser todomvc simple flow --runs 3 --light BIN
```

### Setup

- **light-cljs:** an immutable copy of the release binary at `97e8b250` (master `052ad00e` plus the Var fix below), `research/shadow-credibility/bin/light-cljs-97e8b250`. V8 build.
- **shadow-cljs:** 3.5.3, `release client` of the unmodified examples, as in the sections above.
- **Browser:** Chromium revision 1248 headless shell, driven by playwright-core 1.64.1 from Node 26.12.1. esbuild 0.25.10.
- **Ports:** rewritten by the porter at `research/shadow-credibility/shadow-port-tool-973`, which is 971's porter plus the two shim fixes below.

**What is served.** `bb benchmarks/re-frame.bb bundle APP` makes, per compiler, `out/browser-COMPILER` in the example: a copy of the example's own `resources/public` page with `js/client.js` replaced.
- **shadow-cljs:** its release output as written.
- **light-cljs:** its release output (one ES module importing `react` and `react-dom/client`) bundled by esbuild with React into one minified script, as `index.html` loads a classic script.
- **Entry point.** shadow-cljs calls the build's `:init-fn`, and light-cljs has none. The entry module light-cljs exports is the value of the entry namespace's final form, which in all three apps is the `:init-fn` function. A two-line module imports that export and calls it. The script checks that the `:init-fn` is the final form before relying on it. This is bundling, not a port edit.

**What is checked.** `benchmarks/re-frame-browser.mjs` (Playwright) serves the directory over local HTTP. Each check runs in a fresh browser context, so localStorage starts empty. Console errors and uncaught exceptions from every check are recorded. Checks poll the page after each action, because re-frame handles events in a later task and reagent renders on the next animation frame.
- **TodoMVC:** the TodoMVC specification's behaviours, against re-frame's markup:
  - **renders:** heading, placeholder, focus on load, no list without todos;
  - **add:** trims the title, clears the input, ignores a blank title, updates the counter;
  - **complete:** toggles one todo and back, with the `completed` class and the counter;
  - **toggle all:** completes all and none, and follows the todos;
  - **edit:** double-click, then Enter, blur and Escape; an empty title deletes the todo;
  - **delete;**
  - **filter routes:** links, `#/active` in the URL, the selected link, the back button and a typed route;
  - **clear completed:** the button's visibility and effect;
  - **persistence:** a reload keeps the todos and their completed state.
- **simple:** the clock renders a time and ticks; it starts orange; typing `red` in the colour input turns it red.
- **flow:** the five buttons are clicked through eleven steps (adds, delete, clear flow, register flow, clear). After each step, once the page has settled, the whole page text is recorded. That text includes the zprint rendering of app-db, which goes through sci, rewrite-clj and edamame. Every run must match shadow-cljs's first run step for step, and three absolute checks hold: the initial "No items.", "Item 1" after the first add, and the too-many warning after four adds.

### Results, 3 runs each

Every check passes on both builds, and there were no console errors or uncaught exceptions. No flakes appeared in three runs (a 1 min 21 s run of all three apps, unreserved machine). A rerun with `light-cljs-cd731ddb`, the same change rebased onto master `bc4de387`, gave the same tables.

**TodoMVC**

| Check | shadow-cljs | light-cljs |
|---|---|---|
| renders the app | pass 3/3 | pass 3/3 |
| add todos | pass 3/3 | pass 3/3 |
| complete a todo | pass 3/3 | pass 3/3 |
| toggle all | pass 3/3 | pass 3/3 |
| edit a todo | pass 3/3 | pass 3/3 |
| delete a todo | pass 3/3 | pass 3/3 |
| filter routes | pass 3/3 | pass 3/3 |
| clear completed | pass 3/3 | pass 3/3 |
| persistence | pass 3/3 | pass 3/3 |
| no console errors or uncaught exceptions | pass 3/3 | pass 3/3 |

**simple**

| Check | shadow-cljs | light-cljs |
|---|---|---|
| renders the clock | pass 3/3 | pass 3/3 |
| clock ticks | pass 3/3 | pass 3/3 |
| colour input recolours the clock | pass 3/3 | pass 3/3 |
| no console errors or uncaught exceptions | pass 3/3 | pass 3/3 |

**flow**

| Check | shadow-cljs | light-cljs |
|---|---|---|
| renders and runs the flows | pass 3/3 | pass 3/3 |
| no console errors or uncaught exceptions | pass 3/3 | pass 3/3 |
| page text after each step equals shadow-cljs's first run | pass 3/3 | pass 3/3 |

### Failures found on the way, by cause

With master `052ad00e` and 971's ports, **none of the three apps started**. Fixing three causes made them all pass:

| Failure | Cause | Apps | Fix |
|---|---|---|---|
| `ReferenceError: goog is not defined` at load | **Port shim gap (Closure Library, a non-goal).** re-frame.interop reads `goog.async.nextTick` as a global path. 971's port reports listed it under missing goog, and nothing provided it. | all three | The porter's catalogue gains `compat/goog/async/nextTick.cljs`, which installs `globalThis.goog.async.nextTick` (a MessageChannel task, as Closure's is) when the namespace loads. |
| `ReferenceError: typeof is not defined` on the first render | **Port shim bug.** The catalogue's `goog.typeOf` was `(js/typeof x)`. Upstream emits that as the call `typeof(x)`, which works by accident; light-cljs emits a read of a global named `typeof`. reagent's util calls `goog/typeOf`. | all three | The shim now uses `(js* "typeof ~{}" x)` and returns Closure's `"null"` and `"array"`. |
| `Cannot define property ...NodeCoerceable$coerce..., object is not extensible` at load | **Compiler defect (runtime).** `Var.prototype` was the runtime's frozen base object, so `extend-type Var` threw. rewrite-clj extends its NodeCoerceable protocol to Var. | flow | Fixed in light-cljs: Var gets an extensible prototype over the frozen base, as keywords, symbols and collections already do. Regression: `extensions::var_extension_reaches_vars_made_before_and_after`. |

**No failure came from bundling.**

**Check-script fixes, which failed on both builds alike.**
- Playwright's `check()` fails on a React-controlled checkbox: the box keeps its old state until re-frame's event re-renders it. The checks click and then wait for the state.
- The initial colour in simple is `orange`.

**Port edits added by this pellet** (on top of "Port edits by cause" above): `compat/goog/async/nextTick.cljs` (Closure Library use, a non-goal) in all three ports, and the corrected `goog.typeOf` in `compat/goog.cljs`. The ports themselves are regenerated by the `port` command above; copies are in `research/shadow-credibility/{todomvc,simple,flow}-port-973`.

The porter lives outside git, like 971's, so the two shims are recorded here in full, to rebuild `shadow-port-tool-973` from `shadow-port-tool-971` if that copy is lost.

`compat/goog/async/nextTick.cljs`, new:

```clojure
(ns goog.async.nextTick
  "goog.async.nextTick: call F soon, in a later task, as Closure's does.
  Callers name it by its global path (re-frame.interop:
  `(def next-tick goog.async.nextTick)`), which light-cljs emits as a read of
  globalThis.goog, so loading this namespace installs it there. Added for
  re-frame's examples (light-cljs-973), whose port listed it as missing goog.")

(def ^:private pending #js [])

(defn- flush! []
  (let [fs (.splice pending 0)]
    (dotimes [i (alength fs)]
      (try ((aget fs i))
           (catch :default e (js/setTimeout #(throw e) 0))))))

(def ^:private channel
  (when (exists? js/MessageChannel)
    (let [ch (js/MessageChannel.)]
      (set! (.. ch -port1 -onmessage) (fn [_] (flush!)))
      ch)))

(defn nextTick
  "Call F (with `this` SCOPE) in a later macrotask: a MessageChannel message
  where there is one, else setTimeout 0."
  ([f] (nextTick f nil))
  ([f scope]
   (.push pending (if (some? scope) #(.call f scope) f))
   (when (== 1 (alength pending))
     (if channel
       (.postMessage (.-port2 channel) nil)
       (js/setTimeout flush! 0)))
   nil))

(let [g (or (unchecked-get js/globalThis "goog")
            (let [o #js {}] (unchecked-set js/globalThis "goog" o) o))
      a (or (unchecked-get g "async")
            (let [o #js {}] (unchecked-set g "async" o) o))]
  (unchecked-set a "nextTick" nextTick))
```

`compat/goog.cljs`, replacing `(defn typeOf [x] (js/typeof x))`:

```clojure
(defn typeOf
  "Closure's goog.typeOf: the typeof operator's answer, but \"null\" for nil
  and \"array\" for arrays. Was (js/typeof x), which upstream emits as the
  call typeof(x) and light-cljs as a read of a global named typeof: a
  ReferenceError at the first call (reagent's util, light-cljs-973)."
  [x]
  (let [t (js* "typeof ~{}" x)]
    (cond (not (identical? t "object")) t
          (nil? x) "null"
          (array? x) "array"
          :else "object")))
```
