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

**TodoMVC** (rerun 2026-11-14 by light-cljs-980 with the real cljs.spec.alpha in the port and `light-cljs-31f51dcc`; the stub's measured effect is in "2026-11-14: the real cljs.spec.alpha in TodoMVC and flow" below; light-cljs rows rerun 2026-11-14 by light-cljs-999, asserts elided by default, see "2026-11-14: release elides asserts by default" below; light-cljs rows rerun again 2026-11-14 by light-cljs-985, EDN data reading without namespace registration, see "2026-11-14: EDN data reading is not reflection" below)

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 500,587 | 137,088 | 114,615 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 357,227 | 89,997 | 74,006 |
| light-cljs + esbuild: whole bundle | 451,849 | 141,001 | 122,150 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 309,464 | 95,384 | 82,231 |
| light-cljs output as written, without React | 327,722 | 92,503 | 78,731 |
| light-cljs + esbuild --target=es2015: whole bundle | 463,505 | 144,035 | 124,469 |

Packages imported: react, react-dom/client; npm files in both shadow builds: 7

**simple** (light-cljs rows rerun 2026-11-14 by light-cljs-999, asserts elided by default, see "2026-11-14: release elides asserts by default" below)

| Output | Raw bytes | gzip -9 | brotli -q 11 |
|---|---:|---:|---:|
| shadow-cljs: whole release file | 372,279 | 101,065 | 85,573 |
| shadow-cljs: React alone | 143,360 | 47,091 | 40,609 |
| shadow-cljs: app code (difference) | 228,919 | 53,974 | 44,964 |
| light-cljs + esbuild: whole bundle | 352,675 | 111,339 | 96,838 |
| esbuild: React alone | 142,385 | 45,617 | 39,919 |
| light-cljs: app code (difference) | 210,290 | 65,722 | 56,919 |
| light-cljs output as written, without React | 224,352 | 64,431 | 55,449 |
| light-cljs + esbuild --target=es2015: whole bundle | 362,112 | 113,764 | 98,669 |

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

**Setup.** `light-cljs-71da573e` before (master) and `light-cljs-1e637c3c` after, both V8 builds. The TodoMVC port was made again from porter `shadow-port-tool-983`. Commands: `bb benchmarks/re-frame.bb breakdown todomvc --light BIN` and `bb benchmarks/re-frame.bb size todomvc --light BIN`. light-cljs's sizes are deterministic, so there is one run each. shadow-cljs's whole file moved by 7 bytes between the two runs.

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
