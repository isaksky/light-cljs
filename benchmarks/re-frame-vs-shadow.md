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
bb benchmarks/re-frame.bb breakdown simple todomvc --light BIN
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN   # light-cljs rows only
bb benchmarks/re-frame.bb growth simple todomvc flow --base BIN --light BIN   # what grew, by cause
```

**Run `setup` before `port`, in every fresh checkout or worktree.** The porter compiles the example with shadow-cljs first, which needs the example's `node_modules`; without them shadow-cljs stops at the first npm require (`The required JS dependency "react" is not available`). `port` now says so and names `setup`. The kit writes `node_modules`, the shadow-cljs pin and every build's `out/` into `research/re-frame/examples/APP`, so a worktree works on its own copy of `research/re-frame` (20 MB before setup) and never builds in the shared one. The ports it writes, `research/APP-port`, do not depend on the light-cljs binary (it is only used for the porter's compile check), so one port per app serves every binary measured with it.

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

The tables below are from 2026-11-14, before the output-speed-parity group. Each master commit of that group, master `685f86dd` included, is measured in "2026-11-16: release size across the output-speed-parity group" at the end.

**What each row measures:**
- **Packages.** light-cljs writes ES modules that import `"react"` and `"react-dom/client"`. esbuild bundles and minifies them with React, which is what a user ships. shadow-cljs bundles React itself, through Closure, into one file. Both builds include the same seven npm files (react, react-dom, react-dom/client, scheduler); the script checks this.
- **React alone** is each toolchain building only those two package imports: a shadow-cljs namespace that requires them, and an esbuild module that imports them.
- **App code** is the whole file minus React alone. For compressed sizes this difference is an estimate.

**TodoMVC** (rerun 2026-11-14 by light-cljs-980 with the real cljs.spec.alpha in the port and `light-cljs-31f51dcc`; the stub's measured effect is in "2026-11-14: the real cljs.spec.alpha in TodoMVC and flow" below; light-cljs rows rerun 2026-11-14 by light-cljs-999, asserts elided by default, see "2026-11-14: release elides asserts by default" below; light-cljs rows rerun again 2026-11-14 by light-cljs-985, EDN data reading without namespace registration, see "2026-11-14: EDN data reading is not reflection" below; light-cljs rows rerun again 2026-11-14 by light-cljs-984, constant defs folded through other defs and functions, see "2026-11-14: constant defs fold through other defs and functions" below)

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 500,587 | 137,088 | 114,615 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 357,227 | 89,997 | 74,006 |
| light-cljs + esbuild: whole bundle | 439,336 | 137,348 | 119,062 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 296,951 | 91,731 | 79,143 |
| light-cljs output as written, without React | 314,220 | 89,075 | 75,943 |
| light-cljs + esbuild --target=es2015: whole bundle | 450,945 | 140,293 | 121,218 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**simple** (light-cljs rows rerun 2026-11-14 by light-cljs-999, asserts elided by default, see "2026-11-14: release elides asserts by default" below; rerun again 2026-11-14 by light-cljs-984, constant defs folded through other defs and functions, see "2026-11-14: constant defs fold through other defs and functions" below)

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 372,279 | 101,065 | 85,573 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 228,919 | 53,974 | 44,964 |
| light-cljs + esbuild: whole bundle | 340,205 | 107,990 | 94,176 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 197,820 | 62,373 | 54,257 |
| light-cljs output as written, without React | 210,586 | 61,500 | 52,997 |
| light-cljs + esbuild --target=es2015: whole bundle | 349,606 | 110,452 | 96,112 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**flow** (zprint, sci, rewrite-clj and edamame included; rerun 2026-11-14 by light-cljs-980 with the real cljs.spec.alpha in the port and `light-cljs-31f51dcc`, as TodoMVC above; light-cljs rows rerun 2026-11-14 by light-cljs-999, asserts elided by default, see "2026-11-14: release elides asserts by default" below)

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 1,557,354 | 358,609 | 283,974 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 1,413,994 | 311,518 | 243,365 |
| light-cljs + esbuild: whole bundle | 1,366,200 | 351,552 | 288,748 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 1,223,815 | 305,935 | 248,829 |
| light-cljs output as written, without React | 1,278,110 | 299,219 | 238,458 |
| light-cljs + esbuild --target=es2015: whole bundle | 1,379,841 | 355,156 | 291,385 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**Findings:**
- **TodoMVC, whole file after gzip:** light-cljs is 11.5% larger, 152.9 KB against 137.1 KB, and 15% larger after brotli. With the spec stub the two were level (137.7 KB); the real spec, which shadow-cljs ships too, costs light-cljs about 15 KB after gzip (light-cljs-980, below).
- **TodoMVC app code:** light-cljs is 19% larger after gzip and 24% larger after brotli (2% and 8% with the stub). That is a loss, recorded as found.
- **simple, the smaller app:** light-cljs's app code is 28% larger after gzip and 33% after brotli; the whole file is 13% larger after gzip. That is a loss, recorded as found; light-cljs-979 breaks it down (see "2026-11-14: what the gzip gap consists of").
- **flow, the largest app:** light-cljs's app code is 1.1% smaller after gzip and 2.9% larger after brotli; the whole file is 1.4% smaller after gzip and 2.2% larger after brotli. Both builds now validate zprint's options with the real spec. With the port's spec stub light-cljs's app code was 8.5% smaller after gzip; light-cljs-980 measured the stub's effect at 22.7 KB of the whole file after gzip, so that lead was almost all the stub (the earlier estimate here was 28 KB).
- **Before compression** light-cljs's app code is smaller in all three apps.
- **React** comes out smaller through esbuild than through Closure's simple optimizations: 0.7% raw, 3.1% after gzip and 1.7% after brotli.

**Not minified by light-cljs alone.** esbuild's `--minify` shrinks light-cljs's advanced output from 442,317 to about 353,900 bytes for TodoMVC (378,326 to about 299,400 with the spec stub). After gzip the difference is under 1%, so it is mostly whitespace and names that compress well.

**JavaScript language level.**
- **shadow-cljs:** 3.5.3's default `:output-feature-set` for `:browser` is `:es-unsupported`, so nothing is lowered. Its output is ES2015-level anyway: esbuild with `--target=es2015` leaves it unchanged.
- **light-cljs:** it writes modern JavaScript and does not lower it. TodoMVC has 280 `?.` and 147 `??` operators, both ES2020, plus constructs newer than ES2020.
- **Effect of matching:** the last table row lowers light-cljs's bundle to ES2015 with esbuild `--target=es2015`, which matches shadow's level. That costs 2.0% after gzip in TodoMVC (152,899 to 155,912 bytes), 2.1% in simple and 1.0% in flow, and a user who needs older browsers pays it. At matched language level, TodoMVC's whole file is 13.7% larger than shadow's after gzip (2.5% with the spec stub).

shadow-cljs's output differs by a few bytes from one clean build to the next (500,588 and 500,589 bytes raw; brotli 114,541 and 114,582). light-cljs's output was byte-identical across runs.

**Property renaming.**
- **shadow-cljs:** Closure `:advanced` renames properties. No `cljs$core$...` protocol or arity property names are left in its output.
- **light-cljs:** it does not rename host-facing names, by design (docs/advanced-optimization.md). Its TodoMVC output keeps 34 `$IFn$_invoke$arity$N`-style property names and the protocol method names.
- This accounts for part of the per-library growth in "Where the bytes come from".

### Where the bytes come from

`bb benchmarks/re-frame.bb origins` attributes each output character to a source, using each compiler's source map. Both builds are made with source maps, at the same optimization level as above, and attribution is by segment. The table shows the larger groups for TodoMVC.

| Origin | shadow-cljs | light-cljs |
|---|---:|---:|
| cljs.core (shadow) / light-cljs runtime | 196,560 | 170,996 |
| React (npm) | 141,871 | 0 (bundled later by esbuild) |
| no source in the map | 1,514 | 88,846 |
| cljs.spec (both: the real one since light-cljs-980) | 28,383 | 44,739 |
| reagent/impl | 17,110 | 26,453 |
| Closure Library (shadow) / compat `goog/` shims (light-cljs) | 25,194 | 5,472 |
| cljs.tools.reader (shadow) / light-cljs's built-in reader | 17,045 | 0 |
| reagent/ratom.cljs | 9,350 | 14,933 |
| re_frame/router.cljc | 3,423 | 7,546 |
| re_frame/trace.cljc | 4,334 | 7,164 |
| re_frame/flow | 5,747 | 6,367 |
| total | 499,519 | 441,304 |

Rerun 2026-11-14 by light-cljs-980 (`light-cljs-31f51dcc`, real spec). With the stub, on the same binary, light-cljs's total was 379,217: the real spec adds 44,739 characters of its own, 6,412 of runtime it reaches, 8,219 without a source and about 2,700 elsewhere. light-cljs writes 1.6 times shadow's characters for cljs.spec itself.

For simple: shadow's cljs.core is 154,084 characters and light-cljs's runtime 124,546. reagent/impl is 17,091 against 25,558, re_frame/subs.cljc 4,973 against 10,556, Closure Library 9,235 against 3,707 of shims.

For flow, the larger groups:

| Origin | shadow-cljs | light-cljs |
|---|---:|---:|
| sci/impl | 552,555 | 475,637 |
| cljs.core (shadow) / light-cljs runtime (`js_runtime.js`) | 296,692 | 195,723 |
| no source in the map | 3,456 | 345,865 |
| React (npm) | 141,872 | 0 (bundled later by esbuild) |
| zprint/zprint.cljc | 101,797 | 94,563 |
| zprint/spec.cljc (spec definitions, real in both since light-cljs-980) | 100,311 | 51,273 |
| rewrite_clj/node | 60,720 | 37,725 |
| edamame/impl | 33,118 | 22,629 |
| zprint/config.cljc | 30,805 | 24,096 |
| cljs.spec (both: the real one since light-cljs-980) | 28,391 | 43,818 |
| cljs.tools.reader | 23,632 | 27,740 |
| clojure.string, set, walk, cljs.reader (shadow) / light-cljs's bundled namespaces | 12,587 | 22,925 |
| Closure Library and shadow's runtime (shadow) / compat `goog/` shims (light-cljs) | 23,954 | 7,547 |
| reagent/impl | 17,666 | 26,704 |
| total | 1,554,035 | 1,552,924 |

Rerun 2026-11-14 by light-cljs-980 (`light-cljs-31f51dcc`, real spec), as the TodoMVC table. In flow most of the code is sci and zprint, which are plain ClojureScript with few host-facing names, and light-cljs's output of them is smaller before compression (sci/impl 14% smaller, zprint/spec.cljc's spec definitions 49% smaller). light-cljs's source map leaves 345,865 characters without a source (22% of its output), so each light-cljs row above undercounts by an unknown share and only the totals compare exactly.

**What the attribution shows:**
- **reagent and re-frame code is larger in light-cljs.** It is 1.3 to 2 times larger before compression. The cause is kept property names and fewer inlined calls. flow's sci, rewrite-clj and edamame code goes the other way: smaller in light-cljs.
- **The runtime and the shims are smaller.** With the spec stub that difference kept TodoMVC level overall; with the real spec it no longer does, because light-cljs's spec output is 1.6 times shadow's.
- **Spec is now the same program on both sides** (light-cljs-980): the ports run upstream cljs.spec.alpha, and its macros are expanded on the JVM exactly as shadow-cljs expands them (next section). Before, spec was a stub in the port, 8% of shadow's output in flow.

### 2026-11-14: what the gzip gap consists of (light-cljs-979)

**Setup.**
- **light-cljs:** `research/shadow-credibility/bin/light-cljs-d7357e63`, master `528381be` plus the registration fix below. V8 build.
- **shadow-cljs:** 3.5.3, esbuild 0.25.10, as above.
- **Ports:** simple from porter `shadow-port-tool-982`; TodoMVC from `shadow-port-tool-980`, with the real cljs.spec.alpha.
- **Command:** `bb benchmarks/re-frame.bb breakdown simple todomvc --light BIN`, one build per step. light-cljs's output is deterministic. shadow-cljs's brotli size moves by about 50 bytes between clean builds.

**Method.**
- **Files compared:** both files as a user ships them, React included. On one side, shadow-cljs's release file; on the other, light-cljs's output bundled with React by `esbuild --minify`. The "app code" rows above subtract a React-only build. This section uses whole files instead: inside the two bundles React's gzip share differs by less than 400 bytes, while the two React-only builds differ by 1,474. So the whole-file gap is the app-code gap minus 1,474.
- **Steps:** each step rebuilds light-cljs's side with one more thing that shadow-cljs's release build has and light-cljs's lacks, and records the whole file's gzip size. The steps' source rewrites are measurements on copies under `out/breakdown`; the ports are unchanged.
- **Origins:** after the last step, each compiler's source map (esbuild composes light-cljs's) attributes every character to an origin. An origin's gzip share is its text compressed alone, scaled so that the shares add up to the whole file.

**The gap by cause**, in bytes after gzip -9, light-cljs minus shadow-cljs:

| Cause | simple | TodoMVC |
|---|---:|---:|
| Assertions: shadow-cljs's release mode elides them (`:elide-asserts true`, `shadow/build.clj`), light-cljs's `release` keeps them (step 2) | +1,427 | +1,456 |
| Defines read through a function or a def are not folded: re-frame's `(trace/is-trace-enabled?)` and `interop/debug-enabled?` (`goog.DEBUG`), so the tracing and debug-only code stays (step 3) | +3,250 | +3,612 |
| cljs.reader: light-cljs's reader and the reflection it brings, against shadow-cljs's tools.reader (step 4: light-cljs saves 16,798, shadow-cljs 6,998) | n/a | +9,800 |
| Runtime: light-cljs's runtime slice against cljs.core and shadow-cljs's own runtime | +6,254 | +2,562 |
| Closure Library against the port's `goog/` shims, which implement only what the port uses (not the same code) | -2,009 | -7,436 |
| cljs.spec (the real one on both sides) | n/a | +2,096 |
| clojure.string, set, walk and data | +2,002 | +1,650 |
| re-frame, after the defines are folded | +1,217 | -149 |
| reagent, after asserts are elided | +758 | +798 |
| The app's own namespaces: mostly light-cljs's up-front declaration list of every global (847 and 1,369 alone) | +933 | +1,567 |
| React, other libraries, no source | -167 | -227 |
| **Whole file, as built** (shares are rounded: rows add up to within 2 bytes) | **+13,666** | **+15,730** |

The first three rows are step differences. The others are the origin shares after the last step: step 3 for simple and step 4 for TodoMVC.

**Whole file by step** (bytes):

| App | Step | shadow-cljs gzip | light-cljs gzip | difference | shadow-cljs brotli | light-cljs brotli | difference |
|---|---|---:|---:|---:|---:|---:|---:|
| simple | 1. as built | 101,065 | 114,731 | +13,666 | 85,627 | 99,626 | +13,999 |
| simple | 2. + asserts elided | 101,065 | 113,304 | +12,239 | 85,627 | 98,495 | +12,868 |
| simple | 3. + re-frame's defines read directly | 101,065 | 110,054 | +8,989 | 85,627 | 95,771 | +10,144 |
| TodoMVC | 1. as built | 137,102 | 152,832 | +15,730 | 114,584 | 132,044 | +17,460 |
| TodoMVC | 2. + asserts elided | 137,102 | 151,376 | +14,274 | 114,584 | 130,988 | +16,404 |
| TodoMVC | 3. + re-frame's defines read directly | 137,102 | 147,764 | +10,662 | 114,584 | 127,755 | +13,171 |
| TodoMVC | 4. + no cljs.reader, both compilers | 130,104 | 130,966 | +862 | 108,890 | 113,183 | +4,293 |

**gzip share by origin** after the last step (raw characters, then gzip bytes):

| Origin | simple shadow-cljs | simple light-cljs | gzip difference | TodoMVC shadow-cljs | TodoMVC light-cljs | gzip difference |
|---|---:|---:|---:|---:|---:|---:|
| runtime: cljs.core and shadow's own / light-cljs's runtime | 155,618 / 31,001 | 118,102 / 37,255 | +6,254 | 197,144 / 39,076 | 132,727 / 41,638 | +2,562 |
| Closure Library / the port's goog shims | 9,235 / 3,468 | 3,626 / 1,459 | -2,009 | 25,025 / 9,253 | 4,649 / 1,817 | -7,436 |
| cljs.spec | | | | 28,447 / 8,818 | 41,691 / 10,914 | +2,096 |
| clojure.string, set, walk and data | 5,128 / 1,851 | 13,790 / 3,853 | +2,002 | 6,361 / 2,236 | 13,901 / 3,886 | +1,650 |
| re-frame | 30,480 / 9,710 | 34,701 / 10,927 | +1,217 | 34,087 / 10,727 | 35,564 / 10,578 | -149 |
| reagent | 26,385 / 8,365 | 32,794 / 9,123 | +758 | 26,835 / 8,441 | 33,533 / 9,239 | +798 |
| the app's own namespaces | 1,642 / 639 | 2,771 / 1,572 | +933 | 11,366 / 3,556 | 11,310 / 5,123 | +1,567 |
| React | 141,871 / 45,537 | 142,558 / 45,294 | -243 | 141,871 / 45,518 | 142,929 / 45,316 | -202 |

As built, before steps 2 and 3, the same method gives re-frame +4,108 and reagent +1,793 in simple, and +3,617 and +1,877 in TodoMVC. These are the "1.3 to 2 times larger" libraries above, measured after compression.

Per namespace in simple, gzip share difference as built, then after steps 2 and 3 (each source file compressed alone, from a scratch run of the same method on the same bundles; reagent/impl is its directory):

| Namespace | as built | after steps 2 and 3 |
|---|---:|---:|
| re_frame/trace | +1,308 | +898 |
| re_frame/subs | +1,167 | +1 |
| reagent/impl | +1,018 | +215 |
| re_frame/router | +751 | +452 |
| re_frame/events | +577 | +3 |
| re_frame/fx | +446 | +164 |
| reagent/ratom | +429 | +289 |
| reagent/debug | +270 | +268 |

**What the constructs cost.** These are what-ifs on light-cljs's bundle of each app after step 3: each construct is removed or shortened in the text, and the gzip change measured. Each is an upper bound on what dropping or renaming the construct saves.

| Construct | simple: characters | simple: gzip | TodoMVC: characters | TodoMVC: gzip |
|---|---:|---:|---:|---:|
| Namespace registry (49 `registerReaderNamespace` calls; only TodoMVC has one) | 0 | 0 | 19,967 | -4,468 |
| The up-front declaration list of every global (`let a,b,c,...;`) | 1,140 | -847 | 1,788 | -1,369 |
| Compiler ABI and protocol method property names (`cljs$core$IFn$_invoke$arity$N`, `cljs$lang$maxFixedArity`, `re_frame$router$IEventQueue$_add_event$arity$2`, ...) shortened | 3,891 | -327 | 4,389 | -423 |

- **Property names** kept by design: 0.3 to 0.4 KB after gzip, protocol method slots such as `re_frame$router$IEventQueue$_add_event$arity$2` included, a small part of the gap (light-cljs-232 tracks the mangling).
- **Inlining:** the defines are the one inlining gap measured. Once they fold, re-frame compresses level with shadow-cljs's in TodoMVC and within 1.2 KB in simple, and reagent within 0.8 KB.
- **Protocol dispatch:** no separate cost was found in re-frame or reagent.
- **Constants:** no cost found either. light-cljs's keyword constants are shorter than shadow-cljs's: `f(null,"timer")` against `new E(null,"timer","timer",-1266967739)`.
- **The runtime.**
  - **Size:** light-cljs's runtime slice has fewer characters than cljs.core (118,102 against 155,618 in simple) but compresses to 0.31 gzip bytes per character against cljs.core's 0.20. cljs.core is generated code with repetitive protocol-dispatch templates; light-cljs's runtime is dense hand-written JavaScript in 991 small units (simple), none over 2 KB.
  - **Effect:** that cost barely grows with the app. In simple it is the largest remaining cause: the smaller the app, the larger the gap.
- **cljs.reader:** the reader is one runtime unit that requires the compile-time `inspect`, so a program that reads EDN counts as reflective (tests/compiler/compile_time_units.rs says so). It then registers every namespace with all its retained names and keeps `theNs` and the units it reaches. That is 16.8 KB after gzip, against shadow-cljs's 7.0 KB for cljs.reader on tools.reader.

**The unmapped output** (work item 1).
- **What it was:** the namespace registry. In TodoMVC, 79,224 of 80,313 characters without a source (stub port, `light-cljs-fad5a905`) were the registration that cljs.reader brings. It spelled every name as `\u` escapes, six characters per character.
- **The fix:** the escaping guarded nothing, since the renaming passes skip string literals. The registration now writes plain strings (`d7357e63`, with a regression test). TodoMVC's unmapped output is 24,767 characters, 22,492 of them the registration. Its output as written fell from 92,852 to 90,651 bytes after gzip (stub port).
- **Shipped bundle:** unchanged, because esbuild already unescaped the names. esbuild's renaming changes with its input, so the same program's bundle moved by about 200 bytes after gzip.
- **simple:** it has no registration. Its 2,366 unmapped characters are the module prelude: imports, the runtime's binding list and the declaration list.

**What esbuild's minify removes** (work item 3). Here light-cljs's output is bundled alone, with the packages external, as built:

| Output | simple raw | simple gzip | TodoMVC raw | TodoMVC gzip |
|---|---:|---:|---:|---:|
| light-cljs output as written | 239,536 | 67,845 | 373,357 | 104,175 |
| esbuild, bundling only | 420,119 | 84,986 | 627,171 | 128,177 |
| esbuild --minify-whitespace | 270,805 | 73,515 | 411,988 | 112,226 |
| esbuild --minify-syntax | 363,137 | 80,569 | 546,121 | 122,430 |
| esbuild --minify-identifiers | 384,392 | 81,526 | 585,145 | 124,305 |
| esbuild --minify | 222,528 | 68,388 | 351,781 | 106,278 |

- **Raw effect:** esbuild prints light-cljs's already terse output again. `--minify` takes 7% (simple) and 6% (TodoMVC) off the raw size: redundant blocks and parentheses, `true`/`false`, and names, which esbuild renames again.
- **Compressed effect:** it makes the output larger after gzip, by 543 and 2,103 bytes. Before the registration fix, TodoMVC's 21% raw saving was mostly the unescaped registration.
- **Effect on the comparison:** the shipped bundle needs esbuild for React, so this cost is inside the gaps above. Bundling without identifier renaming is worse: TodoMVC's whole bundle grows from 152,832 to 156,200 bytes.

**Candidate remedies**, none designed here. Per the group rules, the user decides any design change.
- **Folding defines read through a function or a def:** light-cljs-984, maybe later. It extends the advanced optimizer and is worth 3.2 to 3.6 KB here.
- **EDN and cljs.reader reading without namespace registration:** light-cljs-985, maybe later. It splits the reader unit or changes the reflection rule, and is worth up to 9.8 KB in TodoMVC. Done: see "2026-11-14: EDN data reading is not reflection" below.
- **Arities in the bundled clojure.set:** light-cljs-986. The bundled clojure.set defines its public functions as values, so every arity is kept; clojure.walk is kept where Closure drops it. This is a small library fix, worth up to 2 KB. Done: see the next section.
- **Assertions in release builds:** the porter could write `:release {:compiler-options {:elide-asserts true}}`, or light-cljs's `release` could default to it as shadow-cljs's does. That is a default flip, for the user. Worth 1.4 KB.
- **The up-front declaration list:** declaring each global where it is defined is worth 0.8 to 1.4 KB. Not filed.
- **The runtime's compression:** finer slicing for small apps, or a runtime that compresses better, is a design question. It is worth up to 6.3 KB in simple.

### 2026-11-14: bundled clojure.set and clojure.walk keep only what is called (light-cljs-986)

**Change.**
- **clojure.set and clojure.walk** (`src/compiler_libraries/clojure/`) now define their public functions as ordinary multi-arity `defn`s, as upstream does, instead of values wrapped in an arity-checking function. At advanced a program keeps only the methods it calls, and no dispatcher unless it applies or saves the function. re-frame calls only `difference` with two arguments, so that one method is all that remains of clojure.set.
- **Arity errors are kept:** a function with one valid count has a second method that only throws, so its dispatcher still rejects any other count with "Invalid arity: N".
- **clojure.walk's engine** is made at its first call instead of at load, so a program that never walks drops the namespace.

**Setup.** `light-cljs-d7357e63` before (master `c0ee26e6` has the same compiler) and `light-cljs-2c26ffc7` after. Both ports were made again from porter `shadow-port-tool-980` for this run, simple included; the section above used `shadow-port-tool-982` for simple, so its simple baseline is about 300 bytes off this one. Command: `bb benchmarks/re-frame.bb breakdown simple todomvc --light BIN`. The table above is not rerun.

Gzip difference, light-cljs minus shadow-cljs, in bytes, before and after:

| Row | simple before | simple after | TodoMVC before | TodoMVC after |
|---|---:|---:|---:|---:|
| clojure.string, set, walk and data, after the last step | +2,010 | -59 | +1,651 | +330 |
| the same, raw characters of light-cljs's side (shadow-cljs: 5,128 and 6,361) | 13,790 | 5,162 | 13,901 | 7,179 |
| Whole file, 1. as built | +13,960 | +12,599 | +15,735 | +14,360 |
| Whole file, 2. + asserts elided | +12,572 | +10,228 | +14,279 | +12,860 |
| Whole file, 3. + re-frame's defines read directly | +9,282 | +6,862 | +10,667 | +9,212 |
| Whole file, 4. + no cljs.reader, both compilers | | | +866 | -499 |
| Whole file brotli, after the last step | +10,469 | +8,475 | +4,320 | +3,184 |

- **The whole file moves more than the row** (simple: 2,420 against 2,069 bytes): the old libraries also reached runtime units (runtime row +6,228 to +6,128), and the up-front declaration list is shorter (the app's own namespaces row +936 to +829).
- **As built,** before step 3, simple's row is +806 (it was +2,020): reagent's `str-coll` calls `prewalk` in code only `(dev?)` reaches, which stays until the defines fold. shadow-cljs drops it at every step, since Closure folds `goog.DEBUG`.
- **TodoMVC's after-step-4 gap** is now in light-cljs's favour, -499 bytes after gzip, while brotli still favours shadow-cljs by 3,184.
- **After the measurement** the walk engine was changed to read the core functions at each call instead of keeping those of its first call (a `with-redefs` around the first walk was kept). That changes only the engine, which none of the last-step bundles contain.
- shadow-cljs's own size moved by up to 17 bytes between the two runs (137,097 and 137,102 as built), its usual variation between clean builds.

### 2026-11-14: release elides asserts by default (light-cljs-999)

**Change.** `light-cljs release` now omits every `assert` and `fn` `:pre`/`:post` check unless the build says `:compiler-options {:elide-asserts false}`, as shadow-cljs's release mode does (user decision, 2026-11-14). Dev builds and the flag CLI keep asserts. Before this, light-cljs shipped the asserts that shadow-cljs strips; breakdown step 2 above measured them.

**Setup.** One binary, `light-cljs-5dbf5eda` (its compiler is this pellet's commit), and all three ports made again from porter `shadow-port-tool-983`. "Kept" is the same binary with `LIGHT_CLJS='{:builds {:client {:compiler-options {:elide-asserts false}}}}'`, which is what every earlier release build did; "elided" is the new default. Command: `bb benchmarks/re-frame.bb size simple todomvc flow`. Sizes are deterministic for light-cljs, so one run each. The light-cljs rows of the three tables above are the elided run; they also include compiler changes since `light-cljs-31f51dcc` (light-cljs-986 among them), which is why "output as written" moves far more than the asserts account for. The shadow-cljs rows are not rerun: this run's shadow-cljs sizes are within 144 bytes of them.

Whole bundle (light-cljs + esbuild, with React), in bytes:

| App | Raw kept | Raw elided | gzip kept | gzip elided | brotli kept | brotli elided |
|---|---:|---:|---:|---:|---:|---:|
| simple | 361,526 | 352,675 | 113,719 | 111,339 | 98,941 | 96,838 |
| TodoMVC | 489,757 | 484,664 | 151,539 | 150,007 | 130,962 | 129,721 |
| flow | 1,373,807 | 1,366,200 | 353,793 | 351,552 | 290,829 | 288,748 |

- **Saved after gzip:** 2,380 bytes in simple, 1,532 in TodoMVC and 2,241 in flow (brotli: 2,103, 1,241 and 2,081). The 1.4 KB that breakdown step 2 measured for TodoMVC holds.
- **Against shadow-cljs's whole release file after gzip** (this run's shadow-cljs sizes: 101,065, 137,100 and 358,615): simple goes from 12.5% to 10.2% larger, TodoMVC from 10.5% to 9.4% larger, and flow from 1.3% to 2.0% smaller. After brotli: simple 15.6% to 13.2% larger, TodoMVC 14.1% to 13.1% larger, flow 2.4% to 1.6% larger.
- **App code after gzip** against shadow-cljs's: simple 26.2% to 21.8% larger, TodoMVC 17.7% to 16.0% larger, flow 1.1% to 1.8% smaller. The Findings above predate both this and light-cljs-986, so their percentages are older than the tables.
- **The apps still work:** `bb benchmarks/re-frame.bb browser todomvc simple flow --compiler light --runs 1` passed every check on the elided builds.
- **Size ratchet ceilings are unchanged:** tests/compiler/size_ratchet.rs compiles its fixtures through the library with asserts on, not through `release`, so its advanced output does not change.

### 2026-11-14: EDN data reading is not reflection (light-cljs-985)

**Change.** Reading code and reading data are now separate runtime units (user decision, 2026-11-14).
- **Code reading** (cljs.core's `read-string` and `read`) qualifies symbols in syntax quote by `*ns*`, the registered namespaces and the core names. It now lives in the compile-time unit `codeReader`, so it still counts as reflection and still registers every namespace at advanced.
- **Data reading** (`cljs.reader` and `clojure.edn`, and `read-line`) uses the `reader` unit alone, which no longer requires a compile-time unit: `readerCoreNames` moved to `codeReader`, and `inspect`, which reads a value's shape and resolves no namespace, is an ordinary unit.
- **Effect:** a program that only reads data no longer registers its namespaces or keeps `theNs` and what it reaches. Reading behaves as before. tests/compiler/compile_time_units.rs checks both kinds of program (`edn_reading_is_not_reflection`).

**Setup.** `light-cljs-71da573e` before (master) and `light-cljs-1e637c3c` after (its compiler is this pellet's commit), both V8 builds. The TodoMVC port was made again from porter `shadow-port-tool-983`. Commands: `bb benchmarks/re-frame.bb breakdown todomvc --light BIN` and `bb benchmarks/re-frame.bb size todomvc --light BIN`. light-cljs's sizes are deterministic, so there is one run each. shadow-cljs's whole file moved by 7 bytes between the two runs.

TodoMVC whole file (React included), gzip -9 in bytes, light-cljs minus shadow-cljs:

| Step | before | after |
|---|---:|---:|
| 1. as built | +14,574 | +5,356 |
| 2. + asserts elided | +13,034 | +3,899 |
| 3. + re-frame's defines read directly | +9,397 | +385 |
| 4. + no cljs.reader, both compilers | -304 | -299 |
| Brotli, 3. + re-frame's defines read directly | +12,233 | +4,581 |

- **cljs.reader's cost** in light-cljs's bundle (step 3 minus step 4) fell from 16,706 to 7,687 bytes after gzip. shadow-cljs's is 7,003 (cljs.reader on tools.reader).
- **Where it went:** the 49 namespace registration calls are gone (the step 3 what-if "namespace registry calls removed" measured 19,384 characters and -4,300 bytes before, nothing after). The runtime's share as built fell from 53,302 to 49,140 bytes after gzip, and the app's own namespaces, which carried the registration, from 9,745 to 5,078.
- **Release size** (the TodoMVC table above): the whole bundle fell from 150,143 to 141,001 bytes after gzip and from 129,818 to 122,150 after brotli. Against shadow-cljs's whole release file it is now 2.8% larger after gzip (it was 9.5%) and 6.6% after brotli (it was 13.3%). App code is 6.0% larger after gzip (it was 16.1%).
- **The app still works:** `bb benchmarks/re-frame.bb browser todomvc --compiler light --runs 1` passed every check, persistence through local storage included.
- **Shared runtime:** the readable runtime grew by about 290 bytes, so five none/prepare size ratchet ceiling pairs moved up to the next 1,000 (tests/support/ratchet_fixtures.rs). Advanced ceilings are unchanged: no ratchet fixture reads EDN.

### 2026-11-14: constant defs fold through other defs and functions (light-cljs-984)

**Change.** At `--optimize advanced` a def is folded like a `^:const` one when nothing in the program reassigns it (no `set!`, `binding`, `with-redefs`, `js*` hole or second definition) and its one top-level `def` gives it a scalar or symbol literal, a read of another such def, or a call without arguments of a function whose one method takes no parameters and returns such a value; such a call is folded too. The dead branch then goes. This is what re-frame's two flags needed: `(trace/is-trace-enabled?)` around every `with-trace`, and `interop/debug-enabled?`, a def of `goog/DEBUG`. Strings over 16 characters fold only when `^:const`, so long text is not copied to every read. tests/compiler/folding_inlining.rs has the probe (two namespaces, `flags/on?` and `flags/alias?`) and a test that defs reassigned by each of those means never fold.

**Port edit.** The user decided against goog-define flags (2026-11-14). The porter `research/shadow-credibility/shadow-port-tool-984` (983's plus this edit) writes every `(goog-define NAME DEFAULT)` as `(def ^:const NAME DEFAULT)`, the same length so lines stay put; a value the build's base or `:release` `:closure-defines` give the flag is baked into the def (checked with a temporary `:release` define in simple), and the dev ones are dropped as before. `port-report.edn` lists them under `:goog-defines-as-const`: re-frame's `trace-enabled?` and `version` in all three apps. The compat layer's `goog/DEBUG` is a `^:const` def too. The breakdown's step 3 rewrite of `debug-enabled?` now writes a `^:const` def instead of a goog-define.

**Setup.** `light-cljs-1e637c3c` before (master `79337976` has the same compiler) and `light-cljs-8b11492b` after (its compiler is this pellet's first commit), both V8 builds; `light-cljs-7e1d8551`, with the review's fixes for hinted values, gives byte-identical light-cljs sizes. simple and TodoMVC ported from `shadow-port-tool-983` for the breakdown before, and from `shadow-port-tool-984` for everything else (the old compiler gives a goog-define and a `^:const` def the same output, which the size run before confirms: TodoMVC's light-cljs bundle has the raw and gzip sizes of 985's run). Commands: `bb benchmarks/re-frame.bb breakdown simple todomvc --light BIN` and `bb benchmarks/re-frame.bb size simple todomvc --light BIN`. light-cljs's sizes are deterministic, so one run each; load average 5.5 to 7.4 (unreserved). shadow-cljs's whole file moved by up to 8 bytes between runs. flow is not rerun.

Whole file (React included), gzip -9 in bytes, light-cljs minus shadow-cljs:

| Step | simple before | simple after | TodoMVC before | TodoMVC after |
|---|---:|---:|---:|---:|
| 1. as built (asserts kept) | +12,814 | +9,266 | +5,362 | +1,633 |
| 2. + asserts elided (the release default since light-cljs-999) | +10,435 | +6,925 | +3,905 | +258 |
| 3. + re-frame's defines read directly | +7,036 | +6,923 | +391 | +257 |
| 4. + no cljs.reader, both compilers | | | -293 | -370 |
| Brotli, 2. + asserts elided | +11,327 | +8,549 | +7,469 | +4,387 |

- **The rewrite no longer saves anything:** step 3 is within 2 bytes of step 2 in both apps (it was 3,399 and 3,514 bytes). The release default, step 2, is now 111 bytes below what step 3 measured before in simple (107,990 against 108,101) and 139 below in TodoMVC (137,348 against 137,487). The acceptance asked for the as-built row within 100 bytes of the old step 3; as built here still keeps asserts (2,341 bytes in simple), which `release` has elided by default since light-cljs-999, so the release row is the one compared.
- **Where it went:** re-frame's share as built fell from 13,784 to 11,067 bytes after gzip in simple and from 14,175 to 10,749 in TodoMVC; the rest is the runtime functions only the trace code reached (runtime share 37,920 to 37,378 in simple).
- **Release size** (the simple and TodoMVC tables above): the whole bundle fell from 111,500 to 107,990 bytes after gzip in simple and from 141,001 to 137,348 in TodoMVC (brotli: 96,924 to 94,176 and 122,144 to 119,062). Against shadow-cljs's whole release file that is 6.9% larger after gzip in simple (it was 10.3%) and 0.2% in TodoMVC (it was 2.8%); after brotli 10.1% and 3.9%. App code after gzip: 15.6% larger in simple, 1.9% in TodoMVC.
- **The apps still work:** `bb benchmarks/re-frame.bb browser todomvc simple --compiler light --runs 1` passed every check on the new builds.
- **Size ratchet:** no fixture's output changed, so no ceiling moves.

## Build time

`bb benchmarks/re-frame.bb time --runs 5`:
- **Clean builds:** shadow-cljs's `.shadow-cljs/builds` cache is deleted, and light-cljs runs with `--no-cache`.
- **Order:** the two compilers alternate, the first one changing each run.
- **Measures:** medians with the range in parentheses, wall, user and system CPU time, and peak memory, all from `/usr/bin/time -l`, plus the one-minute load average before each run.
- **Machine:** unreserved, and another worker's builds ran throughout. These are small-app numbers.
- **Spec stub:** these timings are of the ports with the spec stub. light-cljs-980 put the real cljs.spec.alpha in TodoMVC's and flow's ports, which gives light-cljs more to compile; they were not re-timed (the timing wave reruns them).

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
- TodoMVC's output reaches 7: `cljs/spec/alpha`, `goog`, `goog/events`, `goog/functions`, `goog/history/EventType`, `goog/object` and `light/compat`. With light-cljs-980's porter (973's 35 files without the spec stub) it reaches `goog/async/nextTick` instead of `cljs/spec/alpha`.
- simple's reaches 5: the same without spec and `goog/history/EventType`.
- flow's reaches 8: `cljs/spec/alpha`, `goog`, `goog/array`, `goog/events`, `goog/functions`, `goog/object`, `goog/string` and `light/compat`. With light-cljs-980's porter, `goog/async/nextTick` instead of `cljs/spec/alpha`.

Unreached shims are not in any output and change nothing. The sizes below are output characters by source map, from the TodoMVC build unless marked.

| Edit | Cause | Apps | light-cljs output | What shadow-cljs ships instead |
|---|---|---|---:|---:|
| Release panic on `(or a (if c (recur) false))` | compiler gap, fixed on master (`4d2669bc`) | TodoMVC, simple | none | none |
| ns clause as a vector, metadata on an ns clause, `#'cljs.reader/read-date` | compiler gaps, fixed in this branch | flow | none | none |
| `light/compat.cljs`: `cljs.core/pr-writer`, which reagent calls; the measured port rewrites references to it (catalogue `:core-gaps`) | compiler gap, fixed on master (light-cljs-981): `cljs.core/pr-writer` is defined, so the shim is no longer needed. The porter `research/shadow-credibility/shadow-port-tool-981` has `:core-gaps {}`, and TodoMVC ported with it builds in dev and release | all | 108 (simple: 10, flow: 149) in the measured build; none with the 981 porter | part of cljs.core |
| The real cljs.spec.alpha of ClojureScript 1.12.145, vendored (light-cljs-980; before, the stub `compat/cljs/spec/alpha.cljs` made `s/valid?` always true). Its 10 macros that TodoMVC reaches (`s/def`, `s/keys`, `s/and`, `s/map-of` and 6 more) resolve symbols through `cljs.analyzer.api`, so the porter expands their 19 call sites on the JVM and splices them: 7 in `todomvc/db.cljs`, 4 in spec's own `alpha.cljs`. cljs.spec.gen.alpha's 6 macros run in light-cljs. TodoMVC's `check-spec-interceptor` now validates the db after every event | JVM-only macro | TodoMVC | 44,739 (cljs.spec) | 28,383 (cljs.spec), the same expansions |
| flow: the same spec, 13 macros at 641 call sites; 254 spliced forms are zprint's `s/def`s in `zprint/spec.cljc`, which zprint checks with `s/valid?` and `s/explain-data` | JVM-only macro, as above | flow | 95,091 (zprint/spec.cljc 51,273, cljs.spec 43,818) | 128,702 (zprint/spec.cljc 100,311, cljs.spec 28,391) |
| ClojureScript 1.12.145 `cljs/spec/alpha.cljs`: the measured port removes its `specize*` extension to `SetLite`, the set type of ClojureScript's lite mode | compiler gap, fixed on master (light-cljs-983): `cljs.core/SetLite` resolves to a type no light-cljs value has (light-cljs has no lite mode), so the extension compiles and never dispatches. TodoMVC's port with the extension put back builds in release. The porter `research/shadow-credibility/shadow-port-tool-983` is 980's without this entry, without the tools.reader entry (light-cljs-982) and without garden's type-extension patches | TodoMVC, flow | one method fewer in the measured build; none with the 983 porter | one method no light-cljs value reaches |
| cljs.spec.gen.alpha's `dynaload` marks its reference to an optional test.check function `:cljs.analyzer/no-resolve` behind `exists?`; light-cljs refused it while test.check is not loaded | compiler gap, fixed in light-cljs-980 with a regression test: such a symbol reads its JavaScript path, as upstream emits it | TodoMVC, flow | none | none |
| `goog.Uri`, which cljs.spec.gen.alpha constructs in its `uri?` generator: a string-holding `deftype` in `compat/goog.cljs` | Closure Library use: a non-goal | TodoMVC, flow | part of goog.cljs (1,682 in TodoMVC) | goog.Uri |
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
| `(goog-define NAME DEFAULT)` written as `(def ^:const NAME DEFAULT)`, a base or `:release` `:closure-defines` value baked in (porter `shadow-port-tool-984`; port-report `:goog-defines-as-const`): re-frame's `trace-enabled?` and `version`. The compat layer's `goog/DEBUG` is a `^:const` def as well | user decision (2026-11-14): no goog-define flags; light-cljs folds a def nothing reassigns at advanced (light-cljs-984) | all | same code (the flags fold either way) | goog-defines |
| `:dev {:compiler-options {:closure-defines ...}}` dropped (light-cljs refuses dev-mode defines); `:dev-http` and `:nrepl` translated | shadow-cljs build feature, dev only | all | none in a release build | none in a release build |
| `:devtools {:preloads [hashp.core]}` dropped: light-cljs has no preloads | shadow-cljs build feature, dev only | flow | none in a release build | none in a release build |
| `:deps true`: the porter vendors the library sources the build reached into `vendor/` (reagent, secretary, clojure.data, macrovich; flow also zprint, sci, rewrite-clj, edamame, tools.reader) | other: light-cljs resolves no dependencies | all | none | none |

**Compiler-gap entries still open on master** after this branch: none. `cljs.core/SetLite` was found by light-cljs-980, which also fixed the `:cljs.analyzer/no-resolve` gap; `cljs.core/pr-writer` (light-cljs-981), `cljs.core/PersistentArrayMapSeq` (light-cljs-982) and `cljs.core/SetLite` (light-cljs-983) were fixed afterwards. The three ns-form and reader gaps and the release panic are fixed. The sci entries are JVM-only macros, port data rather than compiler work. Closure Library entries are recorded as non-goals, not work.

### Full inventory

`bb benchmarks/re-frame.bb inventory todomvc simple flow` lists every `compat/` file with its cause and the characters it puts in the source-mapped light-cljs release; 0 means the file is not reached. It then lists every macro, configuration key and report entry from `port-report.edn`. Macros classed `keep` run unchanged in light-cljs.

The todomvc and flow parts were rerun on 2026-11-14 by light-cljs-980: its ports, with the real cljs.spec.alpha, by `research/shadow-credibility/shadow-port-tool-980` and `light-cljs-31f51dcc`. They therefore also show 973's shim changes. simple's part is still 971's.

#### todomvc: every compat/ file

| File | Cause | Source bytes | light-cljs output characters |
|---|---|---:|---:|
| cljs/pprint.cljs | other: development library replaced by a stub | 837 | 0 |
| cljs/test.cljs | JVM-only macro: cljs.test stub | 308 | 0 |
| clojure/math.cljs | other: a library light-cljs does not ship, written over js/Math | 1,019 | 0 |
| clojure/pprint.cljs | other: development library replaced by a stub | 840 | 0 |
| clojure/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 608 | 0 |
| clojure/test/check.cljs | JVM-only macro: test.check stub | 168 | 0 |
| clojure/test/check/generators.cljs | JVM-only macro: test.check stub | 2,618 | 0 |
| clojure/test/check/properties.cljs | JVM-only macro: test.check stub | 151 | 0 |
| expound/alpha.cljs | JVM-only macro: stub of a spec library | 131 | 0 |
| goog.cljs | Closure Library use (non-goal) | 2,841 | 1,682 |
| goog/array.cljs | Closure Library use (non-goal) | 665 | 0 |
| goog/async.cljs | Closure Library use (non-goal) | 114 | 0 |
| goog/async/nextTick.cljs | Closure Library use (non-goal) | 1,378 | 1,287 |
| goog/crypt.cljs | Closure Library use (non-goal) | 237 | 0 |
| goog/date/Date.cljs | Closure Library use (non-goal) | 86 | 0 |
| goog/date/UtcDateTime.cljs | Closure Library use (non-goal) | 181 | 0 |
| goog/dom.cljs | Closure Library use (non-goal) | 410 | 0 |
| goog/events.cljs | Closure Library use (non-goal) | 2,568 | 1,858 |
| goog/events/EventType.cljs | Closure Library use (non-goal) | 559 | 0 |
| goog/functions.cljs | Closure Library use (non-goal) | 1,075 | 596 |
| goog/history/EventType.cljs | Closure Library use (non-goal) | 113 | 174 |
| goog/i18n/uChar.cljs | Closure Library use (non-goal) | 1,131 | 0 |
| goog/math.cljs | Closure Library use (non-goal) | 341 | 0 |
| goog/net.cljs | Closure Library use (non-goal) | 1,602 | 0 |
| goog/net/WebSocket/EventType.cljs | Closure Library use (non-goal) | 122 | 0 |
| goog/object.cljs | Closure Library use (non-goal) | 775 | 1,557 |
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
| cljs.spec.alpha/init-compile-asserts | expand | 1 | cljs.env/*compiler* |
| cljs.spec.gen.alpha/dynaload | keep | 46 |  |
| cljs.spec.gen.alpha/lazy-combinator | keep | 20 |  |
| cljs.spec.alpha/def | expand | 8 | cljs.spec.alpha/def reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve; cljs.analyzer/*cljs-ns*; cljs.analyzer/resolve-var |
| re-frame.trace/finish-trace | keep | 17 |  |
| cljs.spec.alpha/and | expand | 1 | cljs.spec.alpha/and reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.alpha/every | expand | 1 | cljs.spec.alpha/every reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| secretary.core/defroute | expand | 2 | secretary.core/defroute uses interop IllegalArgumentException. |
| reagent.debug/assert-component | keep | 7 |  |
| cljs.spec.alpha/every-kv | expand | 1 | cljs.spec.alpha/every-kv reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.gen.alpha/lazy-prim | keep | 21 |  |
| reagent.debug/assert-js-object | keep | 3 |  |
| cljs.spec.alpha/tuple | expand | 1 | cljs.spec.alpha/tuple reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.gen.alpha/delay | keep | 6 |  |
| net.cgrand.macrovich/deftime | keep | 1 |  |
| reagent.core/reaction | keep | 1 |  |
| cljs.spec.alpha/keys | expand | 2 | cljs.spec.alpha/keys reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.gen.alpha/lazy-prims | keep | 1 |  |
| cljs.spec.alpha/map-of | expand | 1 | cljs.spec.alpha/map-of reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| reagent.debug/warn | keep | 6 |  |
| cljs.spec.gen.alpha/lazy-combinators | keep | 1 |  |
| reagent.debug/dev? | keep | 6 |  |
| re-frame.trace/with-trace | keep | 17 |  |
| cljs.spec.alpha/mres | expand | 2 | cljs.spec.alpha/mres reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| reagent.debug/warn-unless | keep | 1 |  |
| re-frame.trace/merge-trace! | keep | 35 |  |
| reagent.debug/assert-new-state | keep | 2 |  |
| cljs.spec.alpha/conformer | expand | 1 | cljs.spec.alpha/conformer reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
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

Report: {:files 40, :spliced 13, :patches 1, :missing-goog (goog.History goog.Uri), :unprintable {}, :unsafe {}, :splice-mismatch [], :edits-dropped []}

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
| cljs/test.cljs | JVM-only macro: cljs.test stub | 308 | 0 |
| clojure/math.cljs | other: a library light-cljs does not ship, written over js/Math | 1,019 | 0 |
| clojure/pprint.cljs | other: development library replaced by a stub | 840 | 0 |
| clojure/spec/alpha.cljs | JVM-only macro: spec stub, specs check nothing | 608 | 0 |
| clojure/test/check.cljs | JVM-only macro: test.check stub | 168 | 0 |
| clojure/test/check/generators.cljs | JVM-only macro: test.check stub | 2,618 | 0 |
| clojure/test/check/properties.cljs | JVM-only macro: test.check stub | 151 | 0 |
| expound/alpha.cljs | JVM-only macro: stub of a spec library | 131 | 0 |
| goog.cljs | Closure Library use (non-goal) | 2,841 | 548 |
| goog/array.cljs | Closure Library use (non-goal) | 665 | 88 |
| goog/async.cljs | Closure Library use (non-goal) | 114 | 0 |
| goog/async/nextTick.cljs | Closure Library use (non-goal) | 1,378 | 1,300 |
| goog/crypt.cljs | Closure Library use (non-goal) | 237 | 0 |
| goog/date/Date.cljs | Closure Library use (non-goal) | 86 | 0 |
| goog/date/UtcDateTime.cljs | Closure Library use (non-goal) | 181 | 0 |
| goog/dom.cljs | Closure Library use (non-goal) | 410 | 0 |
| goog/events.cljs | Closure Library use (non-goal) | 2,568 | 1,859 |
| goog/events/EventType.cljs | Closure Library use (non-goal) | 559 | 0 |
| goog/functions.cljs | Closure Library use (non-goal) | 1,075 | 603 |
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
| cljs.spec.alpha/init-compile-asserts | expand | 1 | cljs.env/*compiler* |
| zprint.macros/dbg-print | keep | 2 |  |
| zprint.smacros/only-keys | expand | 40 | its expansion calls a macro that is expanded |
| cljs.spec.gen.alpha/dynaload | keep | 46 |  |
| cljs.spec.gen.alpha/lazy-combinator | keep | 20 |  |
| cljs.spec.alpha/def | expand | 255 | cljs.spec.alpha/def reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve; cljs.analyzer/*cljs-ns*; cljs.analyzer/resolve-var |
| sci.impl.fns/gen-fn | expand | 84 | its expansion calls a macro that is expanded |
| sci.impl.analyzer/gen-return-needs-ctx-call | expand | 1 | its expansion calls a macro that is expanded |
| re-frame.trace/finish-trace | keep | 17 |  |
| zprint.macros/dbg-s-pr | keep | 7 |  |
| sci.impl.analyzer/gen-return-and | expand | 1 | its expansion calls a macro that is expanded |
| cljs.spec.alpha/coll-of | expand | 25 | cljs.spec.alpha/coll-of reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| sci.impl.namespaces/copy-core-var | keep | 356 |  |
| sci.impl.analyzer/gen-return-binding-call | expand | 1 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| cljs.spec.alpha/every | expand | 71 | cljs.spec.alpha/every reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.alpha/or | expand | 15 | cljs.spec.alpha/or reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| sci.impl.evaluator/resolve-symbol | keep | 1 |  |
| reagent.debug/assert-component | keep | 7 |  |
| cljs.spec.alpha/every-kv | expand | 46 | cljs.spec.alpha/every-kv reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.gen.alpha/lazy-prim | keep | 21 |  |
| cljs.spec.alpha/nilable | expand | 49 | cljs.spec.alpha/nilable reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| reagent.debug/assert-js-object | keep | 3 |  |
| cljs.spec.alpha/tuple | expand | 50 | cljs.spec.alpha/tuple reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| sci.impl.faster/nth-2 | expand | 420 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| zprint.macros/dbg-s | keep | 79 |  |
| cljs.spec.gen.alpha/delay | keep | 6 |  |
| sci.impl.vars/with-writeable-var | keep | 2 |  |
| net.cgrand.macrovich/deftime | keep | 1 |  |
| reagent.core/reaction | keep | 1 |  |
| zprint.macros/dbg | keep | 57 |  |
| sci.impl.faster/deref-1 | expand | 1 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| sci.impl.evaluator/def-fn-call | keep | 1 |  |
| cljs.spec.alpha/keys | expand | 40 | cljs.spec.alpha/keys reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.alpha/merge | expand | 40 | cljs.spec.alpha/merge reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.spec.gen.alpha/lazy-prims | keep | 1 |  |
| zprint.macros/dbg-form | keep | 8 |  |
| sci.impl.analyzer/gen-return-recur | expand | 1 | its expansion calls a macro that is expanded |
| sci.impl.analyzer/gen-return-or | expand | 1 | its expansion calls a macro that is expanded |
| cljs.spec.alpha/map-of | expand | 46 | cljs.spec.alpha/map-of reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| cljs.tools.reader.reader-types/log-source | keep | 4 |  |
| sci.core/require-cljs-analyzer-api | port | 1 | catalogue or compat port |
| reagent.debug/warn | keep | 6 |  |
| rewrite-clj.custom-zipper.switchable/defn-switchable | keep | 22 |  |
| sci.impl.analyzer/gen-return-call | expand | 1 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| cljs.spec.gen.alpha/lazy-combinators | keep | 1 |  |
| reagent.debug/dev? | keep | 6 |  |
| sci.impl.types/->Node | expand | 180 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| sci.impl.namespaces/copy-var | keep | 406 |  |
| sci.impl.utils/kw-identical? | expand | 55 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| re-frame.trace/with-trace | keep | 17 |  |
| cljs.spec.alpha/mres | expand | 2 | cljs.spec.alpha/mres reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
| reagent.debug/warn-unless | keep | 1 |  |
| re-frame.trace/merge-trace! | keep | 35 |  |
| sci.impl.faster/get-2 | expand | 2 | sci.impl.macros/? reads &env [(contains? &env (quote &env))] |
| sci.impl.vars/with-bindings | keep | 5 |  |
| sci.impl.macros/deftime | keep | 17 |  |
| reagent.debug/assert-new-state | keep | 2 |  |
| sci.impl.analyzer/gen-return-do | expand | 1 | its expansion calls a macro that is expanded |
| cljs.spec.alpha/conformer | expand | 1 | cljs.spec.alpha/conformer reads &env [&env]; cljs.spec.alpha/& reads &env [&env]; cljs.analyzer.api/resolve |
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

Report: {:files 141, :spliced 355, :patches 5, :missing-goog (goog.Uri goog.string.StringBuffer goog.string.format), :unprintable {}, :unsafe {}, :splice-mismatch [], :edits-dropped []}

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

Every check passes on both builds, and there were no console errors or uncaught exceptions. No flakes appeared in three runs (a 1 min 21 s run of all three apps, unreserved machine). A rerun with `light-cljs-cd731ddb`, the same change rebased onto master `bc4de387`, gave the same tables. light-cljs-980 reran TodoMVC and flow with the real cljs.spec.alpha in their ports (`light-cljs-31f51dcc`), and every check still passes 3/3 on both builds.

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

## 2026-11-14: the real cljs.spec.alpha in TodoMVC and flow

light-cljs-980 replaced the porter's cljs.spec.alpha stub with the real library, so TodoMVC's `check-spec-interceptor` and zprint's option checks now validate in light-cljs's output as they do in shadow-cljs's.

```sh
bb benchmarks/re-frame.bb port todomvc flow --porter research/shadow-credibility/shadow-port-tool-980
bb benchmarks/re-frame.bb origins todomvc flow --light BIN    # cljs/spec in both columns
bb benchmarks/re-frame.bb size todomvc flow --light BIN
bb benchmarks/re-frame.bb browser todomvc flow --light BIN
```

### Setup

- **light-cljs:** `research/shadow-credibility/bin/light-cljs-31f51dcc`, built from commit `31f51dcc`: master `9b3819d5` plus the compiler fix below. V8 build.
- **Ports:** `research/shadow-credibility/todomvc-port-980` and `flow-port-980`, written by `research/shadow-credibility/shadow-port-tool-980`, which is 973's porter plus the changes below. simple does not reach spec; its port and numbers are unchanged.
- **With the stub:** 973's ports (`todomvc-port-973`, `flow-port-973`) built by the same binary.
- shadow-cljs 3.5.3, esbuild 0.25.10, Chromium 1248 as in the sections above; unreserved machine, two other workers active.

### What changed

**Porter** (`shadow-port-tool-980`, outside git):
- **The stub is gone.** `compat/cljs/spec/alpha.cljs` is removed from the catalogue, so the porter vendors cljs.spec.alpha and cljs.spec.gen.alpha from ClojureScript 1.12.145 like any library. spec's macros resolve symbols through `cljs.analyzer.api`, so the porter classes them as JVM-only and splices shadow-cljs's own expansions at each call site; light-cljs compiles exactly the code shadow-cljs compiles there.
- **A `.cljc` macro file beside a `.cljs` of the same namespace** (cljs.spec.gen.alpha) was copied next to it, and light-cljs, which reads one file per namespace, never loaded it. The porter now moves the needed macros into the `.cljs`, as it already did for a `.clj`. A moved definition writes the core names the `.cljs` excludes as `cljs.core/...`: cljs.spec.gen.alpha excludes `symbol`, `list` and `map` and defines generators of those names.
- **Catalogue entry** for `org.clojure/clojurescript` 1.12.145: removes `specize*` on `SetLite` (compiler gap, light-cljs-983).
- **`goog.Uri` shim** in `compat/goog.cljs`, a non-goal Closure Library use.

**Compiler** (this branch, with a regression test in `tests/compiler/macros.rs`): a symbol marked `:cljs.analyzer/no-resolve` whose namespace is not loaded reads its JavaScript path, as upstream emits it, instead of failing. cljs.spec.gen.alpha's `dynaload` guards its test.check references this way. Nothing in the compiler is specific to spec.

### Checks

- **spec validates.** TodoMVC's spliced spec definitions, compiled by light-cljs into a scratch namespace and run in Node: `s/valid?` is true for the default db and a valid todo, and false for an unsorted todos map, a non-string title and an unknown `:showing`. `s/explain-str` prints upstream's messages, such as `2 - failed: string? in: [:todos 1 1 :title] at: [:todos 1 :title] spec: :todomvc.db/title`.
- **The apps still run.** The browser checks pass 3/3 on both builds for TodoMVC and flow, with no console errors. TodoMVC's light-cljs bundle contains the interceptor's "spec check failed" path.
- **`origins` shows cljs/spec in light-cljs's column:** 44,739 characters for TodoMVC against shadow-cljs's 28,383, and 43,818 against 28,391 for flow.

### The stub's measured effect

Same binary, same day. Bytes of the light-cljs + esbuild bundle, React included:

| App | Output | With the stub | Real spec | Change |
|---|---|---:|---:|---:|
| TodoMVC | whole bundle, raw | 443,152 | 496,296 | +53,144 (+12.0%) |
| TodoMVC | whole bundle, gzip -9 | 138,034 | 152,899 | +14,865 (+10.8%) |
| TodoMVC | whole bundle, brotli -q 11 | 119,669 | 132,028 | +12,359 (+10.3%) |
| TodoMVC | app code, gzip -9 | 92,417 | 107,282 | +14,865 (+16.1%) |
| flow | whole bundle, raw | 1,279,480 | 1,369,365 | +89,885 (+7.0%) |
| flow | whole bundle, gzip -9 | 330,970 | 353,707 | +22,737 (+6.9%) |
| flow | whole bundle, brotli -q 11 | 272,073 | 290,362 | +18,289 (+6.7%) |
| flow | app code, gzip -9 | 285,353 | 308,090 | +22,737 (+8.0%) |

**Findings:**
- **The stub flattered light-cljs.** TodoMVC goes from level with shadow-cljs after gzip to 11.5% larger (whole file), and its app code from 2% to 19% larger. flow's 8.5% gzip lead in app code shrinks to 1.1%, and after brotli light-cljs is now 2.9% larger. Recorded as found.
- **light-cljs's output of spec is 1.6 times shadow-cljs's** (44,739 against 28,383 characters in TodoMVC), and spec also pulls in 6,412 more characters of light-cljs's runtime. As for reagent, the likely causes are kept property names and fewer inlined calls; this was not broken down further.
- **Run time** was not measured here: the real spec works, so light-cljs-975 times the same validating program on both sides.

**Rows and notes updated above by this section:** the TodoMVC and flow tables in "Release size" and their findings; the minified-size and language-level notes; the TodoMVC and flow tables and notes in "Where the bytes come from" and "What the attribution shows"; a note in "Build time" that its timings are of the stub ports; the compat reach list and the spec rows in "Port edits by cause"; the todomvc and flow parts of "Full inventory"; the browser rerun line in "Results, 3 runs each".

## 2026-11-16: release size across the output-speed-parity group (light-cljs-1032, light-cljs-1042)

The output-speed-parity group turns the runtime's values into classes (benchmarks/runtime-vs-shadow.md, from light-cljs-1009 on). Its rules ask every pellet to report re-frame simple's and TodoMVC's gzip sizes where they move, and most of its sections say they were not measured. Two Sol reviews flagged it, for light-cljs-1026 (light-cljs-1032) and light-cljs-1016 (light-cljs-1042). This section measures every master commit of the group that has an immutable binary, so each pellet's step shows, with flow as well.

```sh
bb benchmarks/re-frame.bb setup
bb benchmarks/re-frame.bb port simple todomvc flow --porter research/shadow-credibility/shadow-port-tool-983 --light BIN
bb benchmarks/re-frame.bb size simple todomvc flow --light BIN                # f533efb1 and master: both compilers
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN   # every other binary
bb benchmarks/re-frame.bb browser todomvc simple flow --light research/shadow-credibility/bin/light-cljs-685f86dd
```

### Setup

- **light-cljs:** the release binaries in `research/shadow-credibility/bin` of the 24 master commits in the tables, from `f533efb1`, the group's start, to `685f86dd`, master now (light-cljs-1015). `light-cljs-56bf7795` was built for this section. `light-cljs-685f86dd` gives the same sizes as a build of this branch, whose compiler sources equal `685f86dd`'s. All are V8 builds. Each binary is the master commit it is named after, not a pellet's work-in-progress build.
- **shadow-cljs** 3.5.3 builds the unmodified examples. esbuild 0.25.10 bundles light-cljs's module with React (`--minify`, ES module), as in "Release size" above.
- **Ports:** `shadow-port-tool-983`, the porter of the runtime kit (benchmarks/runtime-vs-shadow.md, "Commands"). Each app was ported once, and every binary built that port. A port does not depend on the binary, which is used only for the porter's compile check.
  - **Porter 984** writes goog-define flags as `^:const` defs. With it, master's and `f533efb1`'s outputs have the same raw sizes in all three apps, and gzip sizes within 113 bytes.
  - **flow's port** spells some generated names differently each time it is made. A second port with the same porter left flow's raw size unchanged and moved its gzip size by 37 bytes. Every flow row below is from one port, except `685f86dd`'s. That row is from a second port, on which `56bf7795` gives 348,003 bytes gzip instead of 347,994. Its change is against `56bf7795` on that same port.
- **Determinism:** light-cljs's sizes are stable: master's light-cljs rows were identical in a full run and in a `--light-only` run. shadow-cljs's whole file moved by up to 92 bytes after gzip between its two clean builds (flow 358,567 and 358,659; TodoMVC 137,100 and 137,102; simple identical). Its master-run sizes are used below.
- **Machine:** sizes do not depend on load. The 1-minute load average was 7.6 to 17.3, unreserved.
- **Raw results:** `research/output-speed/app-sizes-1032`. It holds one `size` output per binary (`full-*` with shadow-cljs, `light-*` without) and the browser checks.

### light-cljs's whole bundle per master commit, gzip -9

This is the light-cljs + esbuild bundle with React, what a user ships, in bytes. The change column is against the row above. Pellets are light-cljs numbers, the ones each commit landed since the row before.

| Commit | Pellets | simple | change | TodoMVC | change | flow | change |
|---|---|---:|---:|---:|---:|---:|---:|
| `f533efb1` | group start | 108,631 |  | 138,015 |  | 328,645 |  |
| `80b7a37b` | 1024 | 108,607 | -24 | 137,979 | -36 | 328,645 | 0 |
| `cf613766` | 1027, 1009 | 109,497 | +890 | 138,883 | +904 | 329,417 | +772 |
| `85bf5d44` | 1025 | 109,758 | +261 | 139,133 | +250 | 329,678 | +261 |
| `f91e0e62` | 1010 | 110,313 | +555 | 139,774 | +641 | 330,337 | +659 |
| `5a99032e` | 1011 | 111,691 | +1,378 | 141,785 | +2,011 | 340,095 | +9,758 |
| `ef5c9df2` | 1029 | 112,321 | +630 | 142,331 | +546 | 340,990 | +895 |
| `518edac7` | 1026 | 112,906 | +585 | 142,911 | +580 | 341,552 | +562 |
| `bb6e8fad` | 1030 | 113,606 | +700 | 143,597 | +686 | 342,363 | +811 |
| `1bd59e34` | 1028 | 114,215 | +609 | 144,153 | +556 | 342,979 | +616 |
| `765a92e1` | 1012 | 115,074 | +859 | 145,067 | +914 | 343,821 | +842 |
| `6f5beb29` | 1033 | 115,112 | +38 | 145,140 | +73 | 343,828 | +7 |
| `87e5db4b` | 1034 | 116,051 | +939 | 146,028 | +888 | 344,934 | +1,106 |
| `31578fd9` | 1031, 1036 | 116,898 | +847 | 146,709 | +681 | 345,775 | +841 |
| `4b52e192` | 1013 | 117,086 | +188 | 146,984 | +275 | 346,138 | +363 |
| `daea1afc` | 1035 | 118,292 | +1,206 | 148,198 | +1,214 | 347,464 | +1,326 |
| `c88f22ae` | 1037 | 118,240 | -52 | 148,197 | -1 | 347,162 | -302 |
| `5cd32c66` | 1038 | 115,488 | -2,752 | 148,034 | -163 | 346,977 | -185 |
| `a224bc05` | 1022, 1020, 1021 | 115,463 | -25 | 148,038 | +4 | 347,107 | +130 |
| `1bd3ac3d` | 1014 | 116,176 | +713 | 148,774 | +736 | 347,830 | +723 |
| `15b858bf` | 1016 | 116,549 | +373 | 149,052 | +278 | 347,830 | 0 |
| `0b473fdb` | 1040, 1018 | 116,640 | +91 | 149,378 | +326 | 347,994 | +164 |
| `56bf7795` | 1045 | 116,640 | 0 | 149,378 | 0 | 347,994 | 0 |
| `685f86dd` | 1015 (master now) | 115,247 | -1,393 | 147,758 | -1,620 | 346,561 | -1,442 |

### The same bundle, raw bytes

| Commit | Pellets | simple | change | TodoMVC | change | flow | change |
|---|---|---:|---:|---:|---:|---:|---:|
| `f533efb1` | group start | 341,559 |  | 440,694 |  | 1,260,895 |  |
| `80b7a37b` | 1024 | 341,625 | +66 | 440,760 | +66 | 1,260,961 | +66 |
| `cf613766` | 1027, 1009 | 345,561 | +3,936 | 444,680 | +3,920 | 1,267,080 | +6,119 |
| `85bf5d44` | 1025 | 346,229 | +668 | 445,334 | +654 | 1,269,408 | +2,328 |
| `f91e0e62` | 1010 | 350,628 | +4,399 | 449,738 | +4,404 | 1,274,059 | +4,651 |
| `5a99032e` | 1011 | 353,931 | +3,303 | 454,163 | +4,425 | 1,288,728 | +14,669 |
| `ef5c9df2` | 1029 | 357,642 | +3,711 | 458,110 | +3,947 | 1,293,161 | +4,433 |
| `518edac7` | 1026 | 361,013 | +3,371 | 461,554 | +3,444 | 1,296,663 | +3,502 |
| `bb6e8fad` | 1030 | 367,258 | +6,245 | 468,137 | +6,583 | 1,304,310 | +7,647 |
| `1bd59e34` | 1028 | 372,127 | +4,869 | 473,022 | +4,885 | 1,309,211 | +4,901 |
| `765a92e1` | 1012 | 377,288 | +5,161 | 478,192 | +5,170 | 1,314,492 | +5,281 |
| `6f5beb29` | 1033 | 377,306 | +18 | 478,210 | +18 | 1,314,510 | +18 |
| `87e5db4b` | 1034 | 380,420 | +3,114 | 481,193 | +2,983 | 1,317,670 | +3,160 |
| `31578fd9` | 1031, 1036 | 383,387 | +2,967 | 484,186 | +2,993 | 1,321,138 | +3,468 |
| `4b52e192` | 1013 | 384,948 | +1,561 | 485,995 | +1,809 | 1,323,105 | +1,967 |
| `daea1afc` | 1035 | 390,596 | +5,648 | 491,687 | +5,692 | 1,329,052 | +5,947 |
| `c88f22ae` | 1037 | 390,768 | +172 | 491,766 | +79 | 1,328,044 | -1,008 |
| `5cd32c66` | 1038 | 380,540 | -10,228 | 492,261 | +495 | 1,328,763 | +719 |
| `a224bc05` | 1022, 1020, 1021 | 380,579 | +39 | 492,341 | +80 | 1,329,026 | +263 |
| `1bd3ac3d` | 1014 | 384,252 | +3,673 | 496,262 | +3,921 | 1,333,019 | +3,993 |
| `15b858bf` | 1016 | 386,507 | +2,255 | 497,918 | +1,656 | 1,333,019 | 0 |
| `0b473fdb` | 1040, 1018 | 386,978 | +471 | 498,376 | +458 | 1,333,475 | +456 |
| `56bf7795` | 1045 | 386,978 | 0 | 498,376 | 0 | 1,333,475 | 0 |
| `685f86dd` | 1015 (master now) | 380,341 | -6,637 | 491,693 | -6,683 | 1,326,649 | -6,826 |

### Against shadow-cljs: the group's start and master now

The group's start is `f533efb1` and master now is `685f86dd`. light-cljs's row is the whole bundle with React. shadow-cljs's is its whole release file, which includes React.

| App | Measure | light-cljs at start | light-cljs now | Change | shadow-cljs | light/shadow at start | light/shadow now |
|---|---|---:|---:|---:|---:|---:|---:|
| simple | raw | 341,559 | 380,341 | +38,782 (+11.4%) | 372,279 | 0.917 | 1.022 |
| simple | gzip -9 | 108,631 | 115,247 | +6,616 (+6.1%) | 101,065 | 1.075 | 1.140 |
| simple | brotli -q 11 | 94,673 | 99,807 | +5,134 (+5.4%) | 85,573 | 1.106 | 1.166 |
| TodoMVC | raw | 440,694 | 491,693 | +50,999 (+11.6%) | 500,588 | 0.880 | 0.982 |
| TodoMVC | gzip -9 | 138,015 | 147,758 | +9,743 (+7.1%) | 137,102 | 1.007 | 1.078 |
| TodoMVC | brotli -q 11 | 119,540 | 127,488 | +7,948 (+6.6%) | 114,644 | 1.043 | 1.112 |
| flow | raw | 1,260,895 | 1,326,649 | +65,754 (+5.2%) | 1,557,342 | 0.810 | 0.852 |
| flow | gzip -9 | 328,645 | 346,561 | +17,916 (+5.5%) | 358,659 | 0.916 | 0.966 |
| flow | brotli -q 11 | 270,104 | 284,601 | +14,497 (+5.4%) | 284,350 | 0.950 | 1.001 |

light-cljs's output as written, without React and before esbuild, measured on the same builds:

| App | Measure | At start | Now |
|---|---|---:|---:|
| simple | raw | 211,991 | 255,537 |
| simple | gzip -9 | 62,150 | 68,914 |
| TodoMVC | raw | 315,624 | 372,504 |
| TodoMVC | gzip -9 | 89,731 | 99,635 |
| flow | raw | 1,170,310 | 1,241,489 |
| flow | gzip -9 | 276,463 | 294,317 |

Every row of every binary is in the raw results.

### Findings

- **The group grew the shipped apps by 5.5 to 7.1% after gzip and 5 to 12% raw.** The growth came in steps of 500 to 1,400 bytes after gzip per conversion pellet, spread over most of them. light-cljs-1015, which removed the kind tables and protocol wrappers, gave back 1,393 to 1,620 bytes after gzip (6,637 to 6,826 raw). This is the size cost of the speed-first rule. It is recorded, not a reason to stop.
- **Against shadow-cljs after gzip:**
  - TodoMVC went from level (+0.7%) to 7.8% larger.
  - simple went from 7.5% to 14.0% larger.
  - flow went from 8.4% smaller to 3.4% smaller.
- **Against shadow-cljs before compression:** light-cljs was smaller in all three apps at the start (8 to 19%). Now simple is 2.2% larger, TodoMVC 1.8% smaller and flow still 14.8% smaller.
- **light-cljs-1026** (`ef5c9df2` to `518edac7`), flagged by light-cljs-1032, cost 585 bytes after gzip in simple, 580 in TodoMVC and 562 in flow (raw 3,371, 3,444 and 3,502). Each keyed field read spells its key.
- **light-cljs-1016** (`1bd3ac3d` to `15b858bf`), flagged by light-cljs-1042, cost 373 bytes after gzip in simple and 278 in TodoMVC (raw 2,255 and 1,656), for the call-site entries.
  - flow's output did not change at all.
  - The entries are used only where every value that can hold a core protocol slot has one. The 1016 section of benchmarks/runtime-vs-shadow.md calls this `ProgramFacts::core_slots_runtime_only`. flow, with sci and zprint, apparently does not meet that condition, so it keeps the operations as before.
- **The largest single step was light-cljs-1011, keywords and symbols as classes.** Every keyword constant carries its hash. That cost 1,378 bytes after gzip in simple, 2,011 in TodoMVC and 9,758 in flow, whose sci and zprint hold many keywords.
- **light-cljs-1038 gave simple back 2,752 bytes after gzip, 10,228 raw.**
  - simple builds no sorted collection, yet it kept the sorted map's red-black tree code from light-cljs-1035 through 1037.
  - 1038 made a sorted set hold the tree map, and no store inventory is left. simple drops 31 functions, among them `TreeMapCell`, `treePut`, `treeRemove`, the sorted set's constructor and the store inventory `kindedStore`. This comes from comparing pseudo-named builds of 1037 and 1038.
  - TodoMVC, whose todos are a sorted map, and flow keep the tree code. Each moved by under 200 bytes.
- **Little or no change:** light-cljs-1045 (dev builds) leaves release output byte-for-byte the same; light-cljs-1024 (the Pants build's runtime) moves it by 66 bytes raw and at most 36 after gzip, and light-cljs-1033 by 18 bytes raw.

### The apps still work

`bb benchmarks/re-frame.bb browser todomvc simple flow --light BIN` ran 3 runs per compiler in headless Chromium, with `light-cljs-56bf7795` and again with `light-cljs-685f86dd` (master now). The results below hold for both.
- Every check passed 3/3 on both builds: TodoMVC's 9 behaviours, simple's clock and colour input, and flow's flows.
- No build had console errors or uncaught exceptions.
- flow's page text after each step equals shadow-cljs's.

### Kit changes made for this

- **`port` names `setup` when the example has no `node_modules`.** It failed before with only "port failed". light-cljs-1042 found this in a worktree without `setup`. It was a missing step, not a porter bug: shadow-cljs could not find `"react"`. A failed port now also prints the porter's cause. See the note under the commands at the top. `bb benchmarks/re-frame-kit-test.bb` tests both failure paths, with the example and the porter's process stubbed, so it builds nothing.
- **`size --light-only` skips the two shadow-cljs builds.** Those builds do not depend on the light-cljs binary. A run of all three apps takes about 11 seconds instead of 1 minute 45, which makes a run over many binaries practical. The light-cljs rows are the same as a full run's.

## 2026-11-16: what grew the bundles since f533efb1 (light-cljs-1047)

The previous section measured the growth. From `f533efb1` to master `685f86dd`, each whole bundle grew after gzip:
- simple: 6,616 bytes;
- TodoMVC: 9,743;
- flow: 17,916.

The user wants the size back, without giving up the speed the group gained ("yea i need a size recovery", 2026-11-16). This section attributes the growth by cause. It also lists the recovery pellets filed for it, each with its expected saving.

```sh
bb benchmarks/re-frame.bb growth simple todomvc flow \
  --base research/shadow-credibility/bin/light-cljs-f533efb1 --light research/shadow-credibility/bin/light-cljs-685f86dd
```

### Setup

- **Binaries:** `light-cljs-f533efb1`, the group's start, and `light-cljs-685f86dd`, master's compiler when this was measured (master `cf3e3539` changed only benchmarks). Both are V8 builds.
- **Ports:** made with porter `shadow-port-tool-983` and the `685f86dd` binary, one port per app, as in the previous section.
  - simple's and TodoMVC's outputs have exactly the sizes measured there.
  - flow's port spells some generated names differently each time it is made. With this port, flow's bundle is 346,580 bytes after gzip, against 346,561 in the previous section. Its output at `f533efb1` is 276,460, against 276,463.
- **Machine:** the load average was 10 to 11, unreserved. Sizes do not depend on load.

**Method.**
- **Builds:** each binary builds each port at `:advanced`, with asserts elided as `release` does. The result is one ES module, with a source map that holds the sources. The output is the release output plus the map comment, which is removed before measuring.
- **Attribution:** the map gives every character of runtime text its line in that binary's `js_runtime.js`, and so its `//@unit`. Units only the newer output has are new. Units only the older one has are dropped.
- **The chain:** the causes are taken out one after another, from both outputs. Each row is how much more the cause costs in the newer output after gzip -9. Because every step is applied to both sides, the rows add up exactly to the growth of the output as written. The command checks this. The steps, in order:
  1. keyed names lose their key, kind by kind;
  2. the long protocol and compiler names are shortened to two or three characters;
  3. keyword constants lose their hash argument;
  4. the new units are removed group by group, then the dropped ones;
  5. the rest is what remains: units in both whose text changed, the program, and gzip's interaction between the parts.
- **What is compressed:** light-cljs's output alone, as written. It grew by 6,764, 9,904 and 17,864 bytes, within 1% of the whole bundles' growth.

### The growth by cause

These are gzip -9 bytes of light-cljs's output as written: the cost in `685f86dd`'s output less the cost in `f533efb1`'s. In brackets, the number of units, then their raw bytes.

| Cause | simple | TodoMVC | flow |
|---|---:|---:|---:|
| **Whole output, `f533efb1` to `685f86dd`** | **62,150 to 68,914 (+6,764)** | **89,731 to 99,635 (+9,904)** | **276,460 to 294,324 (+17,864)** |
| Keyed names: the key on field names (`cnt$<key>`) | +605 | +631 | +713 |
| Keyed names: the key on the kind name | +28 | +17 | +18 |
| Keyed names: the key on slot names | +81 | +102 | +155 |
| Keyed names: the key on markers | +9 | +18 | +23 |
| Keyed names: the key on masks | +13 | +25 | +22 |
| Protocol and compiler names written out (`cljs$core$ICounted$_count$arity$1`, masks, markers) | +574 | +558 | +539 |
| Keyword constants' hashes (light-cljs-1011) | +1,043 | +1,524 | +9,195 |
| New units: protocol methods of the classes | +1,630 (124 units, 17,304 raw) | +1,685 (128, 17,563) | +1,819 (138, 18,375) |
| New units: map, transient and seq helpers on the fields, `runtimeKey` | +1,732 (38, 7,253) | +1,649 (38, 7,260) | +1,882 (41, 7,802) |
| New units: equality and hashing by fields (light-cljs-1014) | +987 (13, 3,918) | +1,008 (13, 3,917) | +1,042 (13, 3,918) |
| New units: class declarations: prototype, kind, masks, markers | +585 (31, 2,718) | +673 (36, 3,196) | +734 (41, 3,377) |
| New units: class constructors (`VectorCell` and the others) | +537 (34, 3,778) | +583 (38, 4,193) | +730 (43, 4,801) |
| New units: recognisers by keyed kind (`isHashMap`, `viewOf`, ...) | +512 (41, 3,290) | +513 (41, 3,290) | +556 (41, 3,292) |
| New units: slot-first wrappers of the operations (`count.slots`, ...) | +431 (16, 4,224) | +425 (16, 4,230) | +463 (16, 4,230) |
| New units: copies with another prototype (`specify!`, metadata) | +423 (5, 1,979) | +423 (5, 1,979) | +452 (5, 1,979) |
| New units: `extend-type` over slots (`extendedMethod`, ...) | +309 (4, 1,032) | +303 (4, 1,032) | +306 (4, 1,032) |
| New units: call-site entries (light-cljs-1016) | +288 (14, 2,203) | +213 (10, 1,609) | none |
| New units: sorted map and set as red-black trees (light-cljs-1035) | +42 (2, 398) | +1,844 (48, 9,634) | +1,937 (48, 9,634) |
| Dropped units: kind-table branches of the operations | -1,115 (54, -5,351) | -1,093 (54, -5,365) | -1,162 (54, -5,366) |
| Dropped units: brand stores (`payloads`, `trieBrand`, ...) | -527 (7, -1,585) | -555 (7, -1,585) | -571 (8, -1,638) |
| Dropped units: protocol wrappers (`publicMethod`, ...) | -444 (7, -1,456) | -440 (6, -1,390) | -455 (6, -1,390) |
| Dropped units: the sorted map's and set's old constructors | -557 (18, -1,883) | -86 (4, -281) | -86 (4, -281) |
| Dropped units: other (`compareKeys`, `VectorView`, `ChunkedCell`, ...) | -1,138 (34, -3,747) | -779 (31, -2,704) | -942 (33, -2,924) |
| The rest: units in both whose text changed, the program, interaction | +716 | +663 | +494 |

**By cause, summed:**

| | simple | TodoMVC | flow |
|---|---:|---:|---:|
| the key on keyed names | +736 | +793 | +931 |
| protocol and compiler names written out | +574 | +558 | +539 |
| keyword constants' hashes | +1,043 | +1,524 | +9,195 |
| new units | +7,476 | +9,319 | +9,921 |
| dropped units | -3,781 | -2,953 | -3,216 |
| the rest | +716 | +663 | +494 |

**The size ratchet's fixtures** at `:advanced` (`--compile --optimize advanced --text`), in bytes:

| Fixture | raw at `f533efb1` | raw at `685f86dd` | change | gzip at `f533efb1` | gzip at `685f86dd` | change |
|---|---:|---:|---:|---:|---:|---:|
| vector-printed | 11,366 | 16,019 | +4,653 | 4,397 | 5,352 | +955 |
| map-printed | 14,314 | 23,955 | +9,641 | 5,268 | 7,092 | +1,824 |
| seq-pipeline | 29,787 | 44,510 | +14,723 | 9,147 | 10,950 | +1,803 |
| keyword-case | 3,780 | 5,697 | +1,917 | 1,518 | 1,985 | +467 |
| record-protocol | 5,234 | 5,832 | +598 | 2,240 | 2,485 | +245 |

The runtime slice's floor rose with the classes: a program that prints one map grew 35% after gzip. The program of its own record and protocol grew least.

### What-ifs on master's output

Each transform is applied to `685f86dd`'s output alone. Most rows give the gzip change of the whole bundle with React, measured as `size` measures it. The last three rows are measured on light-cljs's output as written instead, because they leave names undeclared or whole units out, which changes what esbuild renames.

| Transform | simple | TodoMVC | flow |
|---|---:|---:|---:|
| **Whole bundle, gzip -9** | **115,247** | **147,758** | **346,580** |
| Keyed names as `a$<key>`: a short index, then the key | -909 | -1,027 | -1,308 |
| Keyed names as `$<key>a`: the key, then a short index | -1,018 | -1,188 | -1,462 |
| Keyed names as `$a`, without the key (a lower bound only: the design needs the key) | -1,688 | -1,852 | -2,262 |
| Keyword constants without their hash | -1,070 | -1,497 | -9,356 |
| Slot guards that answer `noBranch` removed | -250 | -206 | -240 |
| No up-front declaration list, `let a,b,...;` (output as written) | -728 | -1,268 | -4,940 |
| No runtime binding list: the runtime's `return[...]` and its destructuring (output as written) | -753 | -1,167 | -1,999 |
| Slot-first wrappers, the 21 operation bodies, the recognisers and the slot guards removed: an upper bound (output as written) | -5,432 | -5,784 | -5,892 |

- **Counts behind them:**
  - simple has 180 keyword constants, TodoMVC 280 and flow 1,214.
  - There are 1,372 keyed name uses in simple, 1,614 in TodoMVC and 2,117 in flow. Field names are 1,039, 1,267 and 1,357 of them, on about 62 names.
  - The slot guards number 58 in simple and 61 in the others, in the 124 units that require `noBranch`.
- **Two more measurements outside the table**, on the output as written:
  - **The sorted set in TodoMVC:** cljs.spec's `(extend-protocol Specize ... PersistentTreeSet ...)` makes the slice keep the set's 26 units: 4,470 raw bytes, 1,087 after gzip. TodoMVC never builds a sorted set, and at `f533efb1` the same extension kept no unit. flow keeps those units anyway, because it builds sorted sets.
  - **The side stores:** these are `publicCollectionSlots`, `constructorHashes`, `metadataValues`, `vectorRoots`, `canonicalVectorRoots`, `subvecSources`, `hashRoots` and `hashTraversals`. The runtime lines that read them outside the operations ship 3,825, 3,895 and 4,099 characters, with a gzip marginal of 973, 904 and 966 bytes.

### Findings

- **New code is most of the growth.** The class layout adds 7.5 to 9.9 KB after gzip of new units, and drops 3.0 to 3.8 KB of old ones.
  - **The speed's own code:**
    - the classes' protocol methods, 1.6 to 1.8 KB;
    - equality and hashing by fields, 1.0 KB;
    - the class constructors and declarations, 1.1 to 1.5 KB;
    - the call-site entries, 0.2 to 0.3 KB.
  - **Migration leftovers next to the new paths:**
    - the slot-first wrappers, 0.4 KB;
    - the recognisers, 0.5 KB;
    - the `noBranch` guards inside the slots;
    - the old operation bodies, which still branch on the runtime's own classes;
    - the side stores, which the design says the fields replace.
- **Keyword hashes are flow's largest cause:** 9.2 KB of its 17.9 KB. They cost 1.0 to 1.5 KB in the other two apps.
- **The keyed names cost 1.3 to 1.5 KB.**
  - The key itself is 0.7 to 0.9 KB. Most of it is on field names, which every field read spells.
  - The long protocol names written at every class and use are another 0.5 to 0.6 KB.
- **TodoMVC's red-black tree costs 1.8 KB.** TodoMVC's sorted map needs part of it. The sorted set's own 26 units are there only because cljs.spec extends the set: removing them alone saves 1,087 bytes.
- **Not caused by the group, but recoverable:** the declaration list and the runtime binding list together cost 1.5 to 6.9 KB after gzip, the most in flow.

### Recovery pellets

These are in the output-speed-parity group, before light-cljs-1049, which checks the sizes after them. The savings are of each whole bundle after gzip -9. "Measured" means the what-if above. An estimate says what it starts from.

| Pellet | What it does | simple | TodoMVC | flow | Basis |
|---|---|---:|---:|---:|---|
| light-cljs-1048 (refined) | short keyed names at `:advanced`, key kept | 900 to 1,020 | 1,020 to 1,190 | 1,300 to 1,460 | measured, the two forms |
| light-cljs-1050 | keyword hashes built by the runtime, not written | 1,070 | 1,497 | 9,356 | measured, less the runtime's hashing code |
| light-cljs-1051 | each global and runtime root declared where it is defined | 1,000 to 1,450 | 2,000 to 2,400 | 5,500 to 6,900 | upper bounds measured, less the declarations the definitions gain |
| light-cljs-1052 | an extended class nothing builds keeps only its prototype | 0 | 1,087 | 0 | measured on the output |
| light-cljs-1053 | the side stores moved into the classes' fields | 600 to 900 | 600 to 900 | 600 to 900 | the reads' measured 904 to 973, less the field reads that replace them |
| light-cljs-1054 | core operations shaped as upstream's: slot, then host values | 2,500 to 4,000 | 2,500 to 4,000 | 2,500 to 4,000 | upper bound 5,432 to 5,892, less upstream-shaped bodies of about 1,500 to 2,000 |
| **Sum** | | **6,070 to 8,440** | **8,704 to 11,074** | **19,256 to 22,616** | |
| **Growth since `f533efb1`** | | **6,616** | **9,743** | **17,916** | the previous section |

- **Do the savings cover the growth?**
  - flow: yes, even at the low end of every range.
  - simple: yes, from about a quarter of the way up its range.
  - TodoMVC: yes, from a little under half of the way up.
  - For both, it depends mostly on how small light-cljs-1054's upstream-shaped operations come out.
- **The savings are not exactly additive.** gzip's savings interact, and some text is counted twice: light-cljs-1054's guards and operation bodies spell keyed names that light-cljs-1048 shortens. light-cljs-1049 measures what the pellets give together.
- **Not filed:**
  - **The class layout's own code:** the protocol methods, equality and hashing by fields, the constructors and declarations, the call-site entries, `extend-type` over slots, and the red-black tree where an app builds sorted collections. This code carries the speed the group gained. light-cljs-1048 and light-cljs-1054 shrink its names and guards.
  - **The copies with another prototype**, 0.4 KB, which `specify!` and metadata on runtime values need.

### Kit change made for this

- **`growth APP... --base BIN --light BIN`** prints the two tables above: the chain and the what-ifs. It throws when the chain's rows do not add up to the growth.
- `bb benchmarks/re-frame-kit-test.bb` tests the attribution and the transforms on small hand-made outputs.

## 2026-11-17: the side stores in the values' fields (light-cljs-1053)

light-cljs-1053 deleted `metadataValues`, `constructorHashes`, `publicCollectionSlots`, `subvecSources`, `vectorRoots` and `canonicalVectorRoots`: what they held is each value's own keyed fields (docs/value-layout.md, "What light-cljs-1053 landed"). The previous section expected it to save 600 to 900 bytes per app.

```sh
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN
```

- **Binaries:** `light-cljs-f533efb1`, master `light-cljs-d8a28b21` and `light-cljs-1053-4fd7638c`, the change; all V8 builds.
- **Ports:** porter `shadow-port-tool-983` with the `d8a28b21` binary, one port per app, built by all three binaries.
- **Machine:** unreserved; sizes do not depend on load.

The whole bundle with React, gzip -9, in bytes:

| App | `f533efb1` | master `d8a28b21` | light-cljs-1053 | against master | against `f533efb1` |
|---|---:|---:|---:|---:|---:|
| simple | 108,631 | 115,748 | 115,688 | -60 | +7,057 |
| TodoMVC | 138,015 | 148,293 | 148,132 | -161 | +10,117 |
| flow | 328,664 | 347,000 | 346,888 | -112 | +18,224 |

- **light-cljs's output as written**, gzip -9: simple 69,428 to 69,372 (-56), TodoMVC 100,171 to 100,099 (-72), flow 294,777 to 294,708 (-69).
- **Raw**, the whole bundle grows 119 to 190 bytes: simple 382,779 to 382,969, TodoMVC 494,152 to 494,271, flow 1,329,154 to 1,329,304.

**Why it saves a tenth of the estimate.**
- **A field read is longer than a store call.** The minifier gives each store a short name, so `publicCollectionSlots.get(v)` ships as about `Xa.get(v)`, 9 characters. Its field, `v.slots$<key>`, ships as 17, until light-cljs-1048 shortens keyed names.
- **The branches stay.** Most of the lines that read the stores serve a public constructor's answers, such as a queue's or a set's count as given or a subvector's bounds. Those answers need the slots, so the branches read the field instead of the store.
- **Measured per unit**, TodoMVC's runtime at master against a first version that wrote `meta` and `hash` in every class's constructor:
  - the initialisers cost about 1,000 raw bytes: 24 to 48 bytes in each of 25 constructors;
  - they outweighed the removed reads. That version saved 128 bytes after gzip in TodoMVC and added 32 in flow.
  - The fields are now given on demand.
- **What shrank:** `hashStructural` -104 raw bytes, `collectionWithMeta` -46, `publicFields` -43, the list slots -110 (a list of kind 12 is never the empty list), `ownMeta`, `conjMeta`, `setMeta`, the constructors that wrote the stores, and the six store declarations.
- **What grew:**
  - the field reads in the operations, 15 to 30 bytes each in about 20 units;
  - new units: `extraFields`, the fields a copy keeps; `metaFields`; `heldHash` and `holdHash`, which the Pants build's profile swaps;
  - `setMeta`'s non-enumerable field.

## 2026-11-17: short keyed names at `:advanced` (light-cljs-1048)

light-cljs-1048 writes every keyed name of `:advanced` output short: `$`, the build's key, and a code of one or two letters, the most used names first (docs/value-layout.md, section 1, "Short names at `:advanced`"). This is the `$<key>a` form of the what-ifs above, which saved 109 to 161 bytes more after gzip than `a$<key>`.

```sh
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN
bb benchmarks/re-frame.bb growth simple todomvc flow --base research/shadow-credibility/bin/light-cljs-f533efb1 --light BIN
```

### Setup

- **Binaries:** `light-cljs-4fbc0e65`, master when this was measured (with light-cljs-1053's and light-cljs-1055's fields and slots), and `light-cljs-1048-74d8d955`, this pellet rebased on it with the Sol review's fixes. Both are V8 builds. `light-cljs-f533efb1` is the group's start.
- **Ports:** made with porter `shadow-port-tool-983` and master `4fbc0e65`'s binary, one port per app. Each app's bundle at `f533efb1` is within 12 bytes of the earlier sections' (simple 108,631, TodoMVC 138,015, flow 328,657).
- **Machine:** the load average was 7 to 12, unreserved. Sizes do not depend on load.

### The whole bundle with React, gzip -9

| App | `f533efb1` | master `4fbc0e65` | light-cljs-1048 | Against master | Against `f533efb1` |
|---|---:|---:|---:|---:|---:|
| re-frame simple | 108,631 | 116,302 | 115,599 | -703 (-0.60%) | +6,968 (+6.4%) |
| TodoMVC | 138,015 | 148,918 | 148,035 | -883 (-0.59%) | +10,020 (+7.3%) |
| flow | 328,657 | 347,566 | 346,565 | -1,001 (-0.29%) | +17,908 (+5.4%) |

light-cljs's output as written, without React, after gzip: simple 70,052 to 69,300 (-752), TodoMVC 100,831 to 99,951 (-880), flow 295,394 to 294,409 (-985). Raw, it is 11,647, 12,707 and 27,461 bytes smaller.

The browser behaviour checks pass for all three apps with light-cljs-1048's builds (`bb benchmarks/re-frame.bb browser simple todomvc flow --compiler light --runs 1`).

### Against the what-if

The saving is 69%, 74% and 68% of the what-if's 1,018, 1,188 and 1,462 bytes, measured on master `685f86dd` with this form, short of the pellet's first target ranges (900 to 1,020, 1,020 to 1,190 and 1,300 to 1,460). The user decided on 2026-11-17 that a pellet's size targets are reported, not gated; light-cljs-1049 is the size gate.

**The cause is a table the what-if left out.** The runtime builds protocol slot and marker names from a protocol's key as the program runs, in `protocolSlot`. It uses them for the methods `defineType`, `reify`, `extend-type` and `specify!` install, for a protocol function's dispatch and for the protocol lookups. The renamed text the what-if bundled would have broken at load:
- its slots would no longer have matched the names `protocolSlot` builds;
- the masks were built by concatenation, so the what-if renamed the bare `$` before the key into a short name of its own.

So the output now carries the codes of the protocol slots and markers its text spells, for `protocolSlot`: ` stem code stem code `.
- **Its size:** 32 stems in simple and TodoMVC and 38 in flow, 1,319 to 1,534 raw bytes.
- **What it costs:** about 380 bytes after gzip. Taking it out of simple's and TodoMVC's output, which then no longer runs, saves 376 and 382 bytes.
- **Encodings tried, measured on the output as written:**
  - stems without `cljs$core$` and `$arity$`: 36 bytes less;
  - the protocols' keys, `ICounted/-count/1`, which the text already spells: 79 bytes less, but the runtime would have to normalise every key spelling;
  - the stems sorted: no change.
- **No table at all** would need the runtime never to build a name the text spells. That means passing names with every method table, protocol function and protocol lookup, which costs about as many bytes per implementation as the table does per name.

### `growth`, rerun

`bb benchmarks/re-frame.bb growth simple todomvc flow --base light-cljs-f533efb1 --light light-cljs-1048-f5c498cd`, gzip -9 of light-cljs's output as written. The whole output's growth since `f533efb1` is now:
- simple: +6,435;
- TodoMVC: +9,502;
- flow: +17,246.

The rows that changed against the section above:

| Cause | simple | TodoMVC | flow |
|---|---:|---:|---:|
| Keyed names: the key on field, kind, slot, marker and mask names | +0 (was +736) | +0 (was +793) | +0 (was +931) |
| Protocol and compiler names written out | +99 (was +574) | +34 (was +558) | -213 (was +539) |
| The rest: units in both whose text changed, the program, interaction | +1,100 (was +716) | +1,045 (was +663) | +987 (was +494) |

- **Why the keyed rows are 0:** no keyed name of the form `stem$<key>` is left for the chain to strip. The short names keep the key, as the design requires.
- **Where their cost went:** the key's share is now counted in "the rest".
- **What "names written out" holds now:** mostly the table's stems.
- **What else moved:** the new-unit rows moved by up to 300 bytes. Most of that is light-cljs-1046's slots for sets, queues and vectors, which landed between the two runs.
- **What-ifs:** the three keyed-name what-ifs find no names to rename and save nothing.

## 2026-11-17: declarations where they are defined (light-cljs-1051)

light-cljs-1047's what-ifs found two lists in a single file's `:advanced` output that only repeat names the definitions write anyway: the up-front `let gq,gr,...;` of the program's globals, and the runtime binding list, `const[b,m,...]=(()=>{"use strict";...return[Atom,iY,...]})()`. light-cljs-1051 removes the first (docs/advanced-optimization.md, Phase 3, "Declarations where they are defined"): each top-level definition declares its global where it defines it, `var gq=...`, and adjacent definitions share one `var`. A `var` is hoisted, so a read before the definition runs finds `undefined`, as the `let` up front gave. The second was built, measured and left out (below; light-cljs-1062).

```sh
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN
```

### Setup

- **Binaries:** `light-cljs-f533efb1`, the group's start; `light-cljs-c49a646a`, master with light-cljs-1056 and light-cljs-1057; and `light-cljs-1051-d05aa74d`, this pellet on it. All are V8 builds. The first measurements below used master `light-cljs-1df0a071` and `light-cljs-1051-bc8abfdf`, which also wrote the runtime's units in place.
- **Ports:** porter `shadow-port-tool-983` with the `1df0a071` binary, one port per app, built by every binary. Each app's bundle at `f533efb1` is within 7 bytes of the earlier sections' (simple 108,631, TodoMVC 138,015, flow 328,664).
- **Machine:** unreserved, load average 4 to 44 (other workers' test runs). Sizes do not depend on load.

### The whole bundle with React, gzip -9

| App | `f533efb1` | master `c49a646a` | light-cljs-1051 | Against master | Against `f533efb1` |
|---|---:|---:|---:|---:|---:|
| re-frame simple | 108,631 | 115,634 | 114,985 | -649 (-0.56%) | +6,354 (+5.8%) |
| TodoMVC | 138,015 | 148,128 | 147,043 | -1,085 (-0.73%) | +9,028 (+6.5%) |
| flow | 328,664 | 346,646 | 342,633 | -4,013 (-1.16%) | +13,969 (+4.3%) |

- **Against the pellet's expectation** for both lists together (simple 1,000 to 1,450, TodoMVC 2,000 to 2,400, flow 5,500 to 6,900): the declarations alone save less, 649, 1,085 and 4,013 bytes. The binding list, left out, was to give the rest. The user decided on 2026-11-17 that these targets are reported, not gated; light-cljs-1049 is the size gate.
- **light-cljs's output as written**, without React, after gzip: simple 69,368 to 68,641 (-727), TodoMVC 100,027 to 98,864 (-1,163), flow 294,488 to 289,645 (-4,843); raw 1,047, 1,481 and 6,708 bytes smaller.
- **brotli -q 11:** simple 100,410 to 99,938, TodoMVC 127,944 to 127,272, flow 284,917 to 281,809.
- **Workload 200k** (the generated project, benchmarks/runtime-vs-shadow.md): 3,547,313 to 3,529,299 bytes raw, 896,335 to 883,581 after gzip (-1.42%).

The browser behaviour checks pass for all three apps (`bb benchmarks/re-frame.bb browser simple todomvc flow --compiler light --runs 1`, with the `1051-bc8abfdf` builds, whose declarations are these).

### What was left out: the runtime binding list

`light-cljs-1051-bc8abfdf` also wrote the slice's units at the start of the program's own function in ES modules, with one name table for the units and the program (as a chunked build's module-level units are), instead of the factory and its returned array. Gzip -9, against master `1df0a071` except the first row, which is the table above against `c49a646a`:

| | simple | TodoMVC | flow | workload 200k |
|---|---:|---:|---:|---:|
| declarations alone | -649 | -1,085 | -4,013 | -12,754 |
| declarations and units in the program's function | -1,817 | -2,636 | -7,073 | +4,676 |
| declarations and units, the program's function nested in the runtime's | -1,345 | -2,031 | | +6,800 |

- **Why it was left out:** at 200k lines the units in the program's scope cost 17,360 bytes (1.9%) after gzip, and 65,469 raw. The program's tens of thousands of names share the units' table, which lengthens the runtime's own names. Nesting the program's function inside the runtime's, so that its names could reuse those of bindings it does not read, cost about as much there.
- **Speed:** with the units in place, `kw-hash100` ran 18% slower right after `get-hash100` in the same process (equal alone); the declarations alone show no change.
- **Strictness:** outside a module the units in place need a `"use strict"` at the program function's start, which makes the program strict too; `js-arguments` aliasing its parameters and writes to frozen objects can tell (the Sol review's finding). Only modules could take them.
- **A word that keeps the factory:** a unit binding that keeps its spelling, such as `Atom`, would capture a host global of that name, so a program that could name one keeps the factory (chunked builds' rule). flow first kept it for docstrings spelling `Atom` and an escape in `clojure.string/reverse`'s regular expression, which name nothing.
- light-cljs-1062 holds these numbers, to find a placement that saves in both.

## 2026-11-18: a program protocol's slots named by the text (light-cljs-1058)

light-cljs-1048's table of slot codes for the runtime's `protocolSlot` held every protocol slot and marker the output spells. light-cljs-1058 takes a program protocol's slots out of it: `Advanced` output names them where the runtime needs them, in each protocol function's `protocolMethod` and in each method entry that does not name its dispatcher (docs/value-layout.md, section 1, "A program protocol's slots from the text"). The core protocols' slots and the markers stay in the table, which the runtime's own lookups read.

```sh
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN
```

### Setup

- **Binaries:** `light-cljs-1051-d05aa74d`, whose code is master `8c21c1ae`'s (light-cljs-1051's declarations), and `light-cljs-1058-d4fa8e7d`, this pellet rebased on it. Both are V8 builds. The first measurement, before light-cljs-1051 landed, used master `light-cljs-c49a646a` and `light-cljs-1058-7e6d4545`.
- **Ports:** made with porter `shadow-port-tool-983` and master `1ffb4d00`'s binary, one port per app.
- **Machine:** the load average was 10 to 18, unreserved. Sizes do not depend on load.

### The whole bundle with React, gzip -9

| App | master `8c21c1ae` | light-cljs-1058 | Change | Before light-cljs-1051 (`c49a646a`) |
|---|---:|---:|---:|---:|
| re-frame simple | 114,985 | 114,924 | -61 (-0.05%) | 115,634 to 115,555 (-79) |
| TodoMVC | 147,043 | 146,921 | -122 (-0.08%) | 148,128 to 148,010 (-118) |
| flow | 342,664 | 342,556 | -108 (-0.03%) | 346,665 to 346,368 (-297) |

light-cljs's output as written, without React, after gzip: simple 68,641 to 68,480 (-161), TodoMVC 98,864 to 98,735 (-129), flow 289,637 to 289,437 (-200); before light-cljs-1051 -154, -126 and -173. Raw, the whole bundles are 760, 1,491 and 2,609 bytes smaller both times. The bundles' gzip change moves more between the two masters than the output's does: esbuild's minified bundle changes around the names. After brotli -q 11 the bundles move by -60, +8 and -238 bytes, and the output as written by +17, -43 and +7.

### Against the what-if

light-cljs-1048 saved 703, 883 and 1,001 bytes against its master. With this pellet's 61, 122 and 108 that is about 764, 1,005 and 1,109 bytes, 75%, 85% and 76% of the what-if's 1,018, 1,188 and 1,462 (from 69%, 74% and 68%; with the first measurement's 79, 118 and 297, 77%, 84% and 89%). The savings are against different masters, so the sums are approximate.

**Where the bytes went**, measured on the output as written by taking each part out of the text (which then no longer runs):
- **The table:** 36 stems in simple and TodoMVC and 42 in flow cost 414 to 480 bytes after gzip, the 11 to 16 program protocols' slots 151 to 215 of them. Now it holds 25, 25 and 26 stems, all core slots and markers, which cost 261 to 273.
- **The names that replace them:** each dispatcher's `names`, 8, 14 and 41 of them, cost 25, 57 and 135 bytes; the entries that name their slot after the function, where an `extend-type`'s target may be a host type's name as it runs, 0 to 4. A method entry that names its slot in place of its key costs nothing: the slot's name is shorter than the key.

**Why the core slots stay in the table:** the runtime names them from the keys its own text spells, at each call's arity (`methodKey` and `keySlot`, `coreMethod`, the record methods, and each core entry of a program's type). Passing their names costs about 3 bytes after gzip at each of those sites, which outnumber the 25 stems, and the table costs about 10 bytes a stem.

**Encodings tried**, each against master `1ffb4d00`'s bundles (115,578, 147,978 and 346,543 bytes after gzip):
- **Every entry keeps its key and names its slot after its function:** simple -14, TodoMVC +100, flow +97. The names cost more than the table's program slots did.
- **The slot in place of the key, where `installMethods` alone reads the entry** (a type's, a `reify`'s, a `specify!`'s): simple -104, TodoMVC -114, flow -152; with an `extend-type` of a program or runtime type taking that form too, -104, -122 and -202. This is the form kept.
- **The table's stems spelled as the keys the text holds,** `ICounted/-count/1` for `cljs$core$ICounted$_count$arity$1`, so that gzip finds them in the text: a further 4, 15 and 44 bytes off the bundles (34 to 44 off the output as written). Not kept: the rewriting in `protocolSlot` costs about 170 bytes raw, 80 after gzip, in every program that keeps it, more than a small program's table.
- **A dispatcher's arities and names as one object,** `{1:"$<key>b"}` for `[1]` and `["$<key>b"]`: 1 to 7 bytes larger after gzip, though 16 to 82 smaller raw. Not kept.

The browser behaviour checks pass for all three apps with light-cljs-1058's builds (`bb benchmarks/re-frame.bb browser simple todomvc flow --compiler light --runs 1`).

## 2026-11-17: keyword constants without their hash (light-cljs-1050)

Since light-cljs-1011 each keyword constant at `:advanced` carried the hash the compiler computed, `p(null,"todos",630308868)`, so that startup did no hashing. light-cljs-1047 measured these digits as the largest single cause of flow's growth. light-cljs-1050 writes the constant without them, `p(null,"todos")`. The keyword's first `hash` computes its hash and caches it in the keyword's field, as it already did below `:advanced` and for every keyword built from data. The hash is the same value as before (`value_hash::keyword`, checked by `keyword_constants_are_hashed_on_first_use`), so hash maps and sets of keywords keep their order.

The other option, computing each constant's hash when it is built at load, was not taken. It would add the hashing units to every program that builds a keyword and hash 1,214 keywords at flow's startup, while the gate shows no cost for hashing on first use (benchmarks/runtime-vs-shadow.md, same date). Hashing on first use adds no runtime code: `hash` already computed a keyword's hash when its field was empty.

```sh
bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN
bb benchmarks/re-frame.bb growth simple todomvc flow --base research/shadow-credibility/bin/light-cljs-f533efb1 --light BIN
```

### Setup

- **Binaries:** `light-cljs-f533efb1`, the group's start; `light-cljs-8c21c1ae`, master with light-cljs-1051; and `light-cljs-1050-827965c3`, this pellet on it. All are V8 builds. They were measured before light-cljs-1060 and light-cljs-1058 landed; this pellet changes none of their code.
- **Ports:** porter `shadow-port-tool-983` with the `8c21c1ae` binary, one port per app, built by every binary. simple's and TodoMVC's bundles at `f533efb1` and at master have exactly the previous section's sizes (its `f533efb1` and light-cljs-1051 columns); flow's, whose port spells some generated names differently each time it is made, are 16 and 8 bytes larger.
- **Machine:** unreserved, load average 9 to 17. Sizes do not depend on load.

### The whole bundle with React, gzip -9

| App | `f533efb1` | master `8c21c1ae` | light-cljs-1050 | Against master | Against `f533efb1` |
|---|---:|---:|---:|---:|---:|
| re-frame simple | 108,631 | 114,985 | 113,918 | -1,067 (-0.93%) | +5,287 (+4.9%) |
| TodoMVC | 138,015 | 147,043 | 145,521 | -1,522 (-1.04%) | +7,506 (+5.4%) |
| flow | 328,680 | 342,641 | 333,134 | -9,507 (-2.77%) | +4,454 (+1.4%) |

- **Against the pellet's expectation** (1,070, 1,497 and 9,356 bytes, less what the runtime adds to compute the hash): the runtime adds nothing, and the saving is that measure within 2%.
- **light-cljs's output as written**, without React, after gzip: simple 68,641 to 67,591 (-1,050), TodoMVC 98,864 to 97,347 (-1,517), flow 289,646 to 280,433 (-9,213); raw 2,005, 3,089 and 13,347 bytes smaller.
- **brotli -q 11**, the whole bundle: simple 99,938 to 99,035, TodoMVC 127,272 to 125,973, flow 281,783 to 274,409.
- **Workload 200k** (the generated project, benchmarks/runtime-vs-shadow.md): 3,529,299 to 3,460,653 bytes raw, 883,581 to 853,838 after gzip (-3.4%).

### `growth`, rerun

`growth` against `f533efb1` now finds no keyword constant with a hash: its "keyword constants' hashes" row and the "keyword constants without their hash" what-if are 0 in all three apps. The whole output's growth since `f533efb1`, gzip -9 of light-cljs's output as written, is now:
- simple: +5,441;
- TodoMVC: +7,616;
- flow: +3,969.

The largest causes left are the classes' new units: their protocol methods (about 2,100 to 2,300 bytes per app) and the map, transient and seq helpers (about 2,500 to 2,800).
