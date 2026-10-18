# Code splitting design

Add chunk splitting and lazy loading to light-cljs: a program names the
features it wants to load on demand, and the compiler emits a startup file plus
one file per feature (and per group of features sharing code), each holding
exactly the definitions, constants and runtime units that first become
necessary there. [closure-library-chunk-splitting.md](closure-library-chunk-splitting.md)
describes how Closure Library and Closure Compiler do this for JavaScript. This
document maps that feature onto light-cljs, whose architecture differs in the
ways that matter most: it compiles one program from a semantic IR with
whole-program knowledge, emits one JavaScript expression or ES module, has no
build configuration file, and already treats its runtime as a graph of units.
It is the design that "Phase 4: deferred" of
[advanced-optimization.md](advanced-optimization.md) names as "chunk splitting
with cross-chunk motion".

The plan is incremental: Phase 1 is useful on its own (the lazy loading API in
a single file), Phase 2 ships chunked output at every optimization level, and
Phase 3 brings definition-level placement and per-chunk runtime slices at
`advanced`. Each phase has a measurable exit criterion.

## Goal

A ClojureScript program with two optional features, written as

```clojure
(ns app.main (:require [app.shared :as shared] [light.lazy :as lazy]))

(def editor (lazy/loadable app.editor/start))
(def reports (lazy/loadable app.reports/start))

(defn on-click [feature]
  (-> (lazy/load feature) (.then (fn [start] (start)))))
```

compiles to `app.main.js` (startup: `app.main`, `app.shared`, the runtime
both need), `app.editor.js`, `app.reports.js` and, when the two features share
code that startup does not use, `app.editor+app.reports.js`. Loading a feature
is one `import()` whose promise resolves to the Var's value once the chunk and
its prerequisites have evaluated. The browser's or bundler's module loader
orders, deduplicates and caches the loads; no loader library is emitted.

Targets, measured in Phase 0 on the example application
(`examples/features`, below). Bytes are raw / `gzip -9` of the single-file
`--esm` output at `advanced`:

| Measurement | Today | Measured bound | Target |
|---|---|---|---|
| Startup bytes, `advanced`, both features lazy | the whole program, 72,486 / 21,732; with resident loadables (light-cljs-271) 80,407 / 24,286; written tersely despite `clojure.string/join`'s `js*` (light-cljs-294) 76,145 / 23,900; with the runtime's `Loadable` (light-cljs-272) 69,500 / 21,646; chunked at namespace granularity (light-cljs-274), `app.main.js` 69,587 / 21,575, with the whole-program runtime slice and every constant still in startup; with definitions moved toward their readers (light-cljs-277), 69,539 / 21,551; with constants placed by their readers (light-cljs-278), 69,000 / 21,242, the whole-program runtime slice and the menu's two keywords; with the runtime sliced per chunk (light-cljs-279), 9,046 / 3,373: the units the menu needs (those of the lower bound, 4,184 bytes here), the `Loadable` type (255) and the `lazy$` helper, and 2,879 bytes of hash-trie units that share state with `lookup` ("Open questions") | lower bound, the application without its features: 4,535 / 1,701 | the same application compiled without its features, plus under 200 bytes per `loadable`: under 4,935 raw |
| Bytes loaded for one feature | n/a: both load at startup; chunked (light-cljs-274), `app.editor.js` 276 / 224 and `app.reports.js` 832 / 517, reading the runtime startup holds; with definitions moved (light-cljs-277), 361 / 255 and 874 / 531, each also importing the shared `app.editor+app.reports.js`, 63 / 81; with constants (light-cljs-278) the shared chunk holds `documents` with its table, 632 / 397, and the features 361 / 258 and 874 / 531; a shared chunk that would hold no code is not written (light-cljs-299), so a feature imports one only for code or constants it shares; with the runtime sliced per chunk (light-cljs-279) the shared chunk holds the runtime both features need, 54,195 / 17,412, and `app.editor.js` 5,453 / 2,459 and `app.reports.js` 4,513 / 2,096: the first feature loads 59,648 / 19,871 (editor) or 58,708 / 19,508 (reports), the second only its own file | upper bound, the feature as its own entry: `app.editor` 64,447 / 20,035, `app.reports` 66,841 / 20,117, terse since light-cljs-294 63,927 / 19,817 | the feature's own code and runtime units, plus a shared chunk only when the other feature shares code with it and startup does not |
| Observable behavior | n/a; chunked (light-cljs-274), the example and the Phase 2 fixtures print what the single file prints at every level; the fixture matrix (light-cljs-276) through `--out-dir` and `chunks.json` in Node, and the example in Chrome without a bundler and through Vite 8.0.8, at every level | n/a | identical to the single-file program at every level, given the same `load` calls |
| Compile time of a chunked build | n/a | n/a | within 10% of the single-file build of the same program |

`bb benchmarks/features.bb` regenerates the bounds from the sources. It
builds the compiler and prints raw / `gzip -9` bytes of the single-file
`--esm` output at each level (`advanced` is terse), and the runtime slice of
the `advanced` output as `--size-report` counts it. A variant without a
feature is generated into a temporary directory: the feature's file is left
out, its libspec is dropped, and each form naming it (a `lazy/loadable` site,
or before light-cljs-271 a direct call) becomes `nil`. A feature as its own
entry is `--entry app.editor`, whose value is its last form, `start`. Measured
on October 15, 2026 at `6804bfb`, with `app.main` still calling both features
directly:

| Program | `none` | `prepare` | `advanced` | `advanced` runtime slice |
| --- | ---: | ---: | ---: | ---: |
| `app.main`, the whole application | 452,213 / 108,808 | 452,213 / 108,808 | 72,486 / 21,732 | 67,147 |
| `app.main` without `app.editor` | 450,559 / 108,607 | 450,559 / 108,607 | 67,064 / 20,162 | 62,504 |
| `app.main` without `app.reports` | 436,315 / 106,437 | 436,192 / 106,420 | 64,870 / 20,192 | 63,791 |
| `app.main` without `app.editor` and `app.reports`: startup lower bound | 434,998 / 106,245 | 433,736 / 106,054 | 4,535 / 1,701 | 4,189 |
| `app.editor` as its own entry: chunk upper bound | 435,455 / 106,284 | 435,064 / 106,209 | 64,447 / 20,035 | 63,644 |
| `app.reports` as its own entry: chunk upper bound | 449,706 / 108,526 | 449,706 / 108,526 | 66,841 / 20,117 | 62,504 |

What the bounds say:

- **Only `advanced` separates the programs.** `none` and `prepare` embed the
  whole runtime (432,063 bytes) in every row, so the features are 17 KB of
  their 452 KB. The phases are measured at `advanced`.
- **Startup is 6% of the program.** Without its features the application is
  4,535 bytes: the menu needs keywords, `name`, `subs` and `str`, and
  `app.shared/documents` and the map helpers go with the features. Lazy
  loading can take 94% of the bytes out of startup.
- **Each feature's own entry is its runtime slice.** The runtime is 98.8% of
  `app.editor` as its own entry (63,644 of 64,447 bytes) and 93.5% of
  `app.reports` (62,504 of 66,841). By subtraction from the whole
  application's 67,147-byte slice, only about 4.6 KB of runtime is needed by
  the editor alone and 3.4 KB by reports alone, against 4.2 KB at startup;
  the remaining 55 KB (the collection constructors behind
  `app.shared/documents`, hash maps and sets, seqs, `into`, and the printer
  behind `str`) is needed by both features and not by startup. That is the
  `app.editor+app.reports` chunk Phase 3 has to produce: one feature's load
  stays near its upper bound, and the second feature's load costs only its
  own few kilobytes. The subtraction is approximate, since the terse slice names
  its bindings by frequency per program.
- **`clojure.string/join` turned terse emission off.** Its body is `js*`
  text, and a program with raw `js*` kept its layout and prefixed names, so
  the program part of every row that includes `app.reports` was several times
  larger than its code: 4,337 bytes beside the runtime for `app.reports` as
  its own entry, against 803 for `app.editor`, and 5,339 for the whole
  application against 1,079 without `app.reports`. Since light-cljs-294 raw
  text keeps its tokens inside terse output, and `bb benchmarks/features.bb`
  reports no `advanced` output with prefixed names: `app.reports` as its own
  entry is 63,927 / 19,817 (1,423 bytes beside the runtime), and with
  resident loadables the whole application 76,145 / 23,900 instead of
  80,407 / 24,286 (2,385 beside the runtime instead of 6,647) and without
  `app.editor` 70,816 / 22,215 instead of 74,481 / 22,577.
- **After the switch to `light.lazy` (light-cljs-271),** both targets
  resident in the one file, `bb benchmarks/features.bb` gives 80,407 / 24,286
  bytes for the whole application at `advanced` (runtime slice 73,760) and
  5,233 / 1,923 without both features (slice 4,695): the `Loadable` type
  brings `rt$defineType` (6,553 bytes) into every row that keeps a site, and
  `start` keeps its `lazy/load` calls when the sites become `nil`. The
  feature rows are unchanged. light-cljs-272 measures and trims the stubs.
- **With the runtime's `Loadable` (light-cljs-272)** (Phase 1, "As built
  (light-cljs-272)"), `bb benchmarks/features.bb` gives:

  | Program | `none` | `prepare` | `advanced` | `advanced` runtime slice |
  | --- | ---: | ---: | ---: | ---: |
  | `app.main`, the whole application | 455,109 / 109,611 | 455,109 / 109,611 | 69,500 / 21,646 | 67,482 |
  | `app.main` without `app.editor` | 453,558 / 109,423 | 453,558 / 109,423 | 64,575 / 20,090 | 62,839 |
  | `app.main` without `app.reports` | 439,014 / 107,224 | 438,761 / 107,181 | 65,426 / 20,431 | 64,126 |
  | `app.main` without `app.editor` and `app.reports`: startup lower bound | 437,668 / 107,004 | 436,216 / 106,778 | 4,680 / 1,753 | 4,189 |
  | `app.editor` as its own entry: chunk upper bound | 436,985 / 106,840 | 436,594 / 106,764 | 64,447 / 20,035 | 63,644 |
  | `app.reports` as its own entry: chunk upper bound | 451,236 / 109,083 | 451,236 / 109,083 | 63,927 / 19,817 | 62,504 |

  At `advanced` the whole application is 392 bytes, 196 a site, over the
  69,108 of the same example with direct calls (each target read from a
  required namespace, `js/Promise.resolve` for each `load`; the ratchet's
  `features-direct`), and 596 over the 68,904 of the example before
  light-cljs-271, which called both features from `start`. Its runtime slice
  is 335 bytes over theirs: the `Loadable` class, its names and its slot in
  the runtime factory. Startup without the features is 145 bytes over its
  4,535: `start` and the `load` it calls once the sites are `nil`. `none`
  and `prepare` embed a runtime 1,530 bytes longer.

Non-goals of this design: a module manager with retries and backoff, bundling
npm code (the npm seam stays as designed: packages are static imports a bundler
resolves), separate compilation of chunks against a shared runtime, source
maps, hot reloading, and web worker chunks. "Phase 5: deferred" lists what is
left open on purpose.

## What upstream does

Research sources are the clones under `research/`; the Closure facts come from
[closure-library-chunk-splitting.md](closure-library-chunk-splitting.md), which
cites them.

### Closure Library and Closure Compiler

Closure separates two graphs and three mechanisms:

- The *source* graph (`goog.require`) orders files. The *chunk* graph is an
  explicit tree the build supplies (`--chunk name:count:deps`), in dependency
  order, with input files assigned by position. Nothing infers chunks from the
  program.
- `JSChunkGraph.manageDependencies` moves a source file required by several
  chunks to their deepest common prerequisite, which must already exist: no
  shared chunk is created for an overlap it discovers.
- `CrossChunkCodeMotion` then moves individual declarations toward their users
  (the smallest covering subtree of the referring chunks), but only those its
  syntactic eligibility test accepts: functions, literals and recursively safe
  initializers. A general call initializer is immovable, because the compiler
  cannot know whether delaying it is observable. `CrossChunkMethodMotion`
  moves prototype methods behind stubs; shadow-cljs exposes an option to turn
  it off because it "sometimes creates problems with reify + multiple
  :modules" (`research/shadow-cljs/src/main/shadow/build/closure.clj:229-232`).
- At run time, `ModuleManager` holds a manifest of chunk IDs and
  prerequisites, `ModuleLoader` fetches and `globalEval`s script text, and
  generated code brackets each chunk with `beforeLoadModuleCode(id)` and
  `setLoaded()` so the manager knows when callers may proceed. The manager
  owns deduplication of in-flight loads, dependency ordering, batching, retry
  policy and error dispatch. All of this predates ES modules and reimplements
  what a module loader does.

### ClojureScript `:modules` and `cljs.loader`

Citations are into `research/clojurescript/src/main/`; MG is
`clojure/cljs/module_graph.cljc`, CL `clojure/cljs/closure.clj`, LD
`cljs/cljs/loader.cljs`.

- `:modules` is a map of module keyword to `:entries`, `:output-to` and
  `:depends-on`. An implicit `:cljs-base` module is added, every module
  without dependencies depends on it, and no cycle check exists for
  `:depends-on` (MG:58-86, 282-294).
- `expand-modules` (MG:250-272) assigns inputs, not declarations: each
  module's entries are expanded to their transitive requires, an input
  needed by one module goes there, one needed by several goes to the
  `deepest-common-parent` by depth (MG:169-178, ties unbroken), and orphans
  that no entry reaches go to `:cljs-base` (MG:207-214, 238-247). The test
  `code.split.d`, required by two sibling modules, lands in `:cljs-base`
  (`src/test/clojure/cljs/build_api_tests.clj:592-620`).
- `build-modules` (CL:1279-1346) makes one `JSChunk` per module with
  `addDependency` edges and `optimize-modules` calls `compileChunks`
  (CL:1471). ClojureScript sets no cross-chunk flag, so code motion runs only
  where Closure's `ADVANCED` level turns it on; to let it move method arities
  separately, multi-arity `defn`s are emitted as separate `set!` statements
  with `:top-fn` metadata (`clojure/cljs/core.cljc:3389-3487`, CLJS-1188).
  `:modules` implies `:stable-names true` (CL:2511-2513).
- `cljs.loader` is compiled last, with `MODULE_URIS` and `MODULE_INFOS`
  inlined as constants from the module graph (CL:1253-1277). At run time it
  wraps `goog.module.ModuleManager` and `ModuleLoader` (LD:41-51): `load`
  calls `beforeLoadModuleCode` before fetching (LD:64-76), which marks
  not-yet-loaded parent modules as loaded, so parents must already be on the
  page. `set-loaded!` is written by hand at the end of each module's entry
  namespace; the compiler once injected it (CLJS-2157) and the change was
  reverted (CLJS-2321).
- At `:none` each module file is a list of `document.write` of
  `goog.require` for every namespace assigned to it, and the module URIs
  list every compiled file (CL:1705-1785, 2187-2227; MG:351-369), so lazy
  loading works without Closure, with the development loader.

### shadow-cljs `:modules`, `shadow.lazy` and `:target :esm`

Citations are into `research/shadow-cljs/src/main/shadow/`.

- `:modules` is an explicit map of module id to `:entries`, `:depends-on` and
  `:init-fn`; one module without dependencies is the default. Each module's
  entries are resolved to their full transitive closure independently, and
  `compact-build-modules` (`build/modules.clj:52-175`) moves each source used
  by several modules to the closest common declared ancestor, choosing the
  last in topological order when several exist (`:92-95`). When two modules
  share a source and have no common ancestor, the build throws "no common
  dependency found for src" (`:86-87`); it never creates a shared module. A
  declared entry pulled up into a parent is an error, "Module Entry ... was
  moved out of module" (`:130-152`).
- `shadow.lazy/loadable` (`lazy.clj:22-84`) takes a qualified symbol, looks up
  which module its namespace was assigned to at macro-expansion time, and
  emits `(Loadable. [mods] (fn [] (js* "munged.name")))`: the `js*` keeps the
  compiler from seeing a static dependency. The referenced Var is not checked
  to exist (`:16-20`), and a namespace moving to another module invalidates
  the cached compile of every file that referenced it
  (`build/compiler.clj:982-989`). At run time `shadow.loader` (`loader.js`)
  wraps Closure's `ModuleManager`, fed by a `shadow$modules` global of URIs
  and dependency ids injected into the default module.
- Release builds of several modules wrap each in `(function(){...}).call(this)`
  and rename top-level names onto one shared `$APP` object
  (`build/targets/browser.clj:185-192, 300-312`): that is how definitions in
  one file reach another under advanced renaming.
- Dev builds never create Closure chunks, but lazy loading still works: each
  module file evaluates its sources in order and the loader URIs point at the
  module files (`build/targets/browser.clj:655-684`).
- `:target :esm` emits ES modules, but Closure's own ES module chunk output is
  left commented out "because `binding` re-assigns exported vars"
  (`build/targets/esm.clj:228-237`): an ES module cannot assign an imported
  binding. Instead the default module exports `$APP`, other modules import it
  and `import "./dep.js"` their dependencies, and `shadow$export` calls are
  rewritten into `export const` after optimization. Lazy loading on this
  target is `^:lazy-loadable` metadata on a `def`, which registers an
  accessor by name and module (`build/compiler.clj:1529-1556`), and
  `shadow.esm/load-by-name` does `import("./<module>.js")`
  (`esm.cljs:33-46`). A `:lazy-loadable` def "will also prevent DCE, even if
  never used" (`build/compiler.clj:1540`).

### Bundlers

Rollup, esbuild and Vite take `import()` in the source as the only chunk
boundary. Code is grouped by which entry points reach it: everything reached
only from one dynamic entry becomes that entry's chunk, code reached from two
becomes a chunk both import, and the browser's module map does the loading.
There is no manifest at run time and no loader library. This is the model
light-cljs adopts, with two differences: the boundary names a Var, not a file,
and the grouping runs over definitions and runtime units, not files.

### What carries over

| Closure concept | light-cljs counterpart |
|---|---|
| `--chunk` specification, `JSChunk` | None in the source of truth. Chunks are derived from `loadable` sites; an optional `--chunk` grouping overlays them (Phase 4). |
| `manageDependencies` (file to deepest common prerequisite) | Namespace reach per root (Phase 2). A namespace reached by several roots goes to the chunk of that root set; a shared chunk is created rather than an error raised. |
| `CrossChunkCodeMotion` with syntactic eligibility | Placement of every retained definition by the roots that reach it (Phase 3). Eligibility is the existing inert-initializer scan: a definition that preparation could remove when unread can be delayed. |
| `CrossChunkMethodMotion` stubs | Not planned. Rapid type analysis and inlining already decide what each method costs; stubs are deferred (Phase 5). |
| `ModuleManager`, `ModuleInfo`, `ModuleLoader` | The host's ES module loader: `import()` deduplicates, orders prerequisites, evaluates once and resolves a promise. |
| `beforeLoadModuleCode` / `setLoaded` markers | None. A module's evaluation is synchronous and complete when `import()` resolves. |
| `setAllModuleInfo`, `setModuleTrustedUris` | The `import` statements in the chunk files themselves, plus a JSON manifest for hosts and tests (Phase 2). |
| Synthetic shared chunks | Shared chunks are ordinary chunks named by their root set. |
| `prefetchModule` / `preloadModule` | `modulepreload` from the manifest (host) and `lazy/prefetch` (Phase 4). |
| Retry with backoff, 401/410 policy, subtractive loading | Not provided; `load` rejects with the host's error (Phase 5 notes the module-map caveat). |

## Where light-cljs stands

- **One program.** `compiler::analyze_namespace_entry_with_provider`
  (`src/compiler.rs:1275`) walks the entry's requires depth first
  (`Loader::visit`, `src/compiler.rs:960-1134`), orders the sources, analyzes
  them one after another into one `Program`, and makes `program.root` a `Do`
  of one `Do` per namespace, in dependency order, each registered in
  `namespace_entries` (`src/analyzer.rs:3340-3380`). Every expression and
  global carries its `source_index`. Cross-namespace references are plain
  `Global(GlobalId)` reads; "transitive-only and unknown qualifiers are
  errors", so a read of another namespace's Var always comes with a `require`
  edge. The `Program` does not record the require graph itself; the `Loader`
  has it (`NamespaceSource::requires`).
- **One closure.** `emit_js_mode` (`src/emitter.rs:127-330`) writes
  `(() => { <prelude> <runtime> <constants> let g0; let g1; ... <namespaces in
  order> return <entry value>; })()`. Globals are `let g<n>` declarations at
  the top, assigned by their `Def` statements. With `--esm`, `module_source`
  (`src/emitter.rs:665`) prepends static package imports and writes
  `export default` before that expression. Nothing is exported by name, and
  the emitter writes the whole program in one pass into one `String`.
- **Whole-program optimization over the IR.** `prepare_application`
  (`src/application.rs:291-428`) runs defines, retention, folding, inlining,
  removal, specialization, type inference, rapid type analysis and call
  shapes over the single `Program`. `retain` (`:449`) computes exactly the set
  this design needs for motion: `Retained::candidates`, the discarded
  top-level definitions whose initializer the `Inert` scan (`:608`) accepts,
  which may be removed when nothing reads them. `flatten` (`:522`) lists the
  top-level forms.
- **The runtime is a unit graph.** `src/runtime_manifest.rs` parses
  `src/js_runtime.js` into 1,224 units with `requires:`, `effect:`,
  `install:` and `when:` edges; `runtime_slice::sliced_runtime`
  (`src/runtime_slice.rs:283`) keeps the units reachable from the roots the
  emitter recorded, and the output destructures the bindings it reads from
  the factory expression: `const [rt$keyword, rt$assoc, ...] = (() => {...;
  return [keyword, assoc, ...]})()`. Roots are recorded per emitted site
  (`RuntimeRoots`, with the demanding expression), so what each top-level
  form demands is known.
- **Constants are a table.** `Advanced` output declares each distinct keyword,
  symbol, constant collection and hoisted local method once, after the
  runtime, `const k3 = rt$hashed(rt$arrayMapFromArray([k0, k1, k1, k2]),
  1120004098);`, numbered in first-read order so parts precede composites.
- **Terse names are assigned over the whole output.** `terse::terse_program`
  (`src/terse.rs:278-290`) renames every compiler name by frequency after
  emission; `terse_slice` renames runtime bindings the same way. Names are
  compiler-owned, so any consistent assignment across several files is valid.
- **Reflection is gated.** A program registers namespaces only when it
  reaches reflection (`RuntimeRoots::reach_reflection`), and the emitter
  emits the body twice to find out. The same gate decides below whether
  definitions may move between chunks.
- **Hosts.** The CLI writes one file to stdout. The browser playground calls
  `analyze_namespace_entry_with_provider` with in-memory sources and runs the
  result in a worker (`playground/crate/src/lib.rs`). `--esm` output is tested
  in Node (`tests/compiler/npm_packages.rs`).
- **No notion of laziness.** There is no API to load code later, no multi-file
  output, no named exports, and no assignment of a global from outside its
  closure.

## Design principles

Continuing the seven principles of advanced-optimization.md:

8. **One program, partitioned output.** A chunked build is one compilation of
   one `Program`; chunks are a partition of its emitted items. Every
   whole-program analysis sees every chunk, so lazy code is as optimized as
   eager code and never changes what the compiler may assume. There is no
   separate compilation of a chunk against another build.
9. **The host's module loader is the chunk loader.** Chunks are ES modules;
   loading is `import()`. No manifest is read at run time, no markers are
   emitted, no loader library is shipped.
10. **Lazy edges are in the source and name Vars.** `(lazy/loadable
    app.editor/start)` is resolved by the compiler: the target is analyzed,
    retained and placed, and its export name is known. No string lookups, no
    `js*`, no metadata that keeps a definition alive without a reader.
11. **Removability is movability.** A definition that preparation could drop
    if nothing read it has no observable initialization, so it may run later,
    in the chunk of its first reader. Every other top-level form stays in its
    namespace's chunk, in source order. This is Closure's eligibility test
    done with the IR's purity knowledge instead of syntax.
12. **Placement is by reach from roots.** Each item's chunk is the set of
    roots (startup plus each lazy target) that reach it. Distinct sets are
    distinct chunks, and reach is monotone along references, so the chunk
    graph is acyclic by construction. No size heuristics decide placement;
    grouping and merging are optional overlays.
13. **The single-file program is the oracle.** At every level, the chunked
    build must print what the single-file build prints when the same loads
    are awaited at the same points.

## The model

### Roots, edges and reach

- A **root** is a place code can start running: the entry namespace
  (*startup*) and the target of each `loadable` (a *lazy root*). A lazy root
  is a namespace, named either directly, `(lazy/loadable app.editor)`, or
  through a Var, `(lazy/loadable app.editor/start)`.
- A **static edge** is a `require`. A **lazy edge** is a `loadable` site. The
  `Loader` follows lazy edges to analyze their targets but does not count them
  as dependencies: a lazy target may itself require the namespace that loads
  it, and no cycle is reported through a lazy edge.
- The **static reach** of a root is the namespaces reachable from its
  namespace over static edges only. For the example above, startup reaches
  `app.main` and `app.shared`; `app.editor`'s root reaches `app.editor`,
  `app.shared` and whatever else it requires.

### Items and signatures

An **item** is one of: a top-level form of the prepared program (an entry of
`flatten(program.root)`), a constant table entry, a runtime unit, or an
output-level helper (`truth`, `host`, `pkg`, a `core$` cell).

An item's **signature** is the set of roots whose code reaches it. The chunk
of an item is its signature, except that any signature containing startup is
the startup chunk (startup loads first, so there is nothing to defer).

- A **pinned** item has the signature of its namespace: the set of roots whose
  static reach contains the namespace. Pinned items keep their source order.
- A **movable** item has the union of the signatures of its readers.
  Readers are items whose expressions read one of its globals anywhere,
  including inside function bodies, since a function may run as soon as its
  chunk has loaded, and a type, `reify` or extension whose method names a
  protocol method it defines. Constants and runtime units are movable; their
  readers are the items that use them and the constants and units that
  require them.

Which top-level forms are movable: below `Advanced`, none (Phase 2). At
`Advanced`, exactly the `Retained::candidates` of `retain`, the definitions
whose initializer the `Inert` scan accepts (literals, functions, pure and
allocating core calls on plain data, constructors of program types defined
earlier), plus `DefProtocol` and `DefType` forms, whose load-time work touches
only what they declare. `ExtendType` of a type another form declares,
`defmethod` (an `AddMethod` call), `defmulti` (its initializer builds a
`MultiFn`, which `Inert` does not accept), `set!`, `enable-console-print!`,
printing, a `def` with a `test` or several definitions, and every other form
with an effect stay pinned. So do three kinds of definition that removal
could drop (light-cljs-277): one of a Var that another form defines too,
since two chunks would assign it; the definition of a `loadable`'s target
Var, so that the chunk a loadable loads, its root's namespace's, defines its
target; and a form holding a `loadable` site, which the manifest gives to its
namespace's chunk. A raw `deftype*` or `defrecord*` stays as well, since it
runs its body when it defines the type. A program that reaches reflection
pins everything, so a registered namespace is never split across chunks.

Reach is monotone: if item `a` reads item `b`, every root reaching `a`
reaches `b`, so `sig(b) ⊇ sig(a)`. Consequences, each relied on below:

- A movable item moves only toward dependents, never up: its signature is a
  subset of its namespace's.
- An item's dependencies are in its own chunk or in a chunk with a strictly
  larger signature. Chunk imports therefore point from smaller signatures to
  larger ones, and the chunk graph is a DAG without a check.
- Within a chunk, the original program order restricted to the chunk's items
  is a valid order: nothing in a chunk depends on anything in a chunk that
  loads later.

The computation is one breadth-first walk per root over the item reference
graph (dense `ExprId`/`GlobalId` arrays, a bitset of roots per item, per
`docs/dos-and-donts.md` rule 1): the walk starts from the pinned items of the
root's static reach and from the Vars its `loadable`s name, follows reads of
globals to their defining items, reads of constants to their entries and
recorded runtime demands to units, and marks the root's bit. With `r` roots
and `n` items this is `O(r · (n + edges))`; a program has tens of roots at
most and thousands of items.

### The chunk graph and files

- The startup chunk is named after the entry namespace, `app.main.js`. A chunk
  whose signature is one lazy root is named after it, `app.editor.js`. A
  shared chunk joins its roots' names sorted with `+`,
  `app.editor+app.reports.js`, falling back to `shared-<hash>.js` when the
  name would exceed 80 characters. Bundlers rename chunks anyway; hosts
  without a bundler get stable names.
- A chunk imports the chunks holding the items it reads, by name:
  `import { a, b, c } from "./app.main.js";`. Names are the same in the
  exporting and importing module (one terse renaming over the whole build),
  so no `as` clauses are needed. A chunk exports exactly the names dependents
  import.
- A `loadable` site in chunk `S` whose target has signature `T` emits
  `import("./<T>.js")`. When the target lies in `S` itself or in a chunk `S`
  already imports, the loadable is *resident*: no import, `loaded?` is true
  from the start, and `load` resolves on the next microtask.
- The entry namespace's final value stays the default export of the startup
  chunk, as `--esm` emits today.
- A chunk that would write no code is not a file (light-cljs-299): at
  `advanced`, one that is no root's chunk, owns no global another chunk
  reads, and holds only forms that emit nothing there. Its importers
  import its imports, and startup lists its namespaces.

### What a chunk file contains

```js
// app.editor.js (advanced, pseudo_names; terse output has short names)
import { rt$keywords, rt$hash, rt$assoc, k0, k3, g2, g7$ } from "./app.main.js";
import { g12, k9 } from "./app.editor+app.reports.js";
const [rt$sortedMapFromArray, rt$compare] = ((keywords, hash) => {
  // the runtime units of this chunk's signature, as written in js_runtime.js
  return [sortedMapFromArray, compare];
})(rt$keywords, rt$hash);
const k14 = rt$keyword(null, "editor");          // constants first read here
export let g20, g21;                             // globals defined here that dependents read
let g22;                                         // globals only this chunk reads
g20 = ...;                                       // the chunk's items, in program order
g7$(k14);                                        // a set! of app.main's g7 goes through its setter
...
```

The sections, in order: imports; the chunk's runtime slice as a factory whose
parameters are the runtime bindings of other chunks it references (so unit
text is embedded as written, with no renaming inside unit bodies; a chunk is
a module, so the factory needs no `"use strict"`, and startup's is the single
file's, `(() => { "use strict"; ... })()`; light-cljs-279); constants;
`let` declarations of the globals whose definitions are in this chunk,
exported when another chunk reads them; the items in program order; nothing
after. The startup chunk also carries the prelude helpers it needs and the
output-level cells (`let core$inc = rt$inc;`) for core operations the program
may replace, which stay in startup with setters, since a `set!` of one may
come from any chunk.

### Assignment across chunks

An ES module cannot assign an imported binding, which is why shadow-cljs
abandoned Closure's ES module output for `binding`. Here the owner chunk of a
global that another chunk assigns (`set!`, `binding`, `with-redefs`, a
`defonce` re-check) exports a setter next to the binding, `export const g7$ =
(v) => g7 = v;`, and the assigning chunk calls it; the expression value is
the assigned value either way. The owner's own assignments stay direct.
Setters are compiler names and get short names like everything else. The
analysis that finds assigned globals already exists for call shapes
(`src/call_shapes.rs`, "Known functions"); placement reuses it per chunk.

### The loading API

A first-party namespace, `light.lazy` (the name is an open question), with:

| Form | Meaning |
|---|---|
| `(lazy/loadable ns/var)` | A special form. Resolves `ns/var` as a Var of a namespace that need not be required, makes `ns` a lazy root, and evaluates to a `Loadable` whose target is that Var. |
| `(lazy/loadable ns)` | The same for a namespace with no particular Var: loading runs its top-level effects; the value is `nil`. |
| `(lazy/load l)` | A promise of the target's current value, resolved after the chunk and its prerequisites have evaluated; rejects with the host's error. Always asynchronous. |
| `(lazy/loaded? l)` | Whether the chunk has evaluated (always true for a resident loadable). |
| `@l` | The target's current value; throws when not loaded, as shadow-cljs does. |
| `(lazy/prefetch l)` | Phase 4. |

`Loadable` is the runtime's type (unit `Loadable` of `src/js_runtime.js`,
light-cljs-272), which `light.lazy/Loadable` in
`src/compiler_libraries/light/lazy.cljs` names, holding an import thunk and a
getter; the library is compiled with the program like `clojure.string` is.
Only `loadable` needs the compiler: it lowers to
`ExprKind::Loadable { target }`, which the emitter writes as
`new Loadable(() => import("./app.editor.js"), (m) => m.g20)` in a chunked
build and `new Loadable(null, () => g20)` when the target is resident. The
getter reads the module namespace object, whose properties are live bindings,
so `@l` after a `set!` sees the new value. A loadable is a value: it can be
stored, passed and loaded from anywhere, including from inside another lazy
chunk, and loading it twice is one `import()`.

The target Var is retained as a root, like an explicit export
(`ApplicationOptions::exports`), so rapid type analysis, call shapes and
removal see it as escaping: its dispatcher exists and its function is kept.
Nothing else about the target is special; a `loadable` of a namespace retains
only that namespace's pinned forms.

### A complete load

For the example in the Goal section at `advanced`:

1. The page or bundler loads `app.main.js`. Its runtime slice, constants and
   the items of `app.main` and `app.shared` evaluate; the two loadables are
   constructed with import thunks.
2. A click calls `(lazy/load editor)`: `import("./app.editor.js")`.
3. The host fetches `app.editor.js`, sees its imports of `./app.main.js`
   (already in the module map) and `./app.editor+app.reports.js` (fetched
   now), evaluates the shared chunk, then the editor chunk: its runtime
   factory adds the units startup lacked (sorted maps, say, and the printer's
   `toString` install on startup's collection constructors), its constants,
   then `app.editor`'s forms in order.
4. The promise resolves with `m.g20`, the current value of
   `app.editor/start`; the callback calls it.
5. `(lazy/load reports)` later fetches only `app.reports.js`; the shared chunk
   is already in the module map. A second `(lazy/load editor)` resolves
   without a request.

## Phases

### Phase 0: contract, example and measurement

Scope: fix the semantics and get numbers before writing compiler code.

- Write the **code-splitting contract** section of this document into
  `docs/compiler.md` next to the advanced-mode contract once Phase 2 lands;
  until then it lives here. Done in light-cljs-275: "Code-splitting
  contract" below points to it.
- Add `examples/features`: `app.main` (startup: logs a menu, holds two
  loadables, calls them from a `start` function the host triggers),
  `app.shared` (keyword and map helpers both features use), `app.editor`
  (sorted collections, `str`, printing through `enable-console-print!`) and
  `app.reports` (sets, `reduce`, `clojure.string`). Each feature logs a line
  the Node driver can check. Keep each file under 40 lines so the output is
  readable with `--pseudo-names`.
- Measure with a babashka script, `benchmarks/features.bb`: the single-file
  size at each level today; the size of the application with each feature
  removed by hand (a lower bound for the startup chunk); the size of each
  feature compiled as its own entry (an upper bound for its chunk). Record
  the table in this document under Goal and in `benchmarks/compiler.md`.
- Add the Node driver `tests/support/chunked_execution.mjs`: given a
  directory of chunk files and the startup file name, it writes
  `package.json` with `"type": "module"`, imports the startup chunk, calls its
  default export when that is a function, awaits the microtask queue until
  it is empty, and prints the shared outcome protocol
  (`tests/support/execution.js`). The same driver runs a single-file `--esm`
  output, so the oracle comparison is one function.
  As built (light-cljs-268), the driver awaits a promise the default export
  returns and then waits for Node's `beforeExit`, so timers and the `import()`
  of a chunk count as pending too, not only microtasks. The record's id is the
  startup file name, its trace is empty (the program has no observer), and its
  value must be a host value, since no inspection adapter reaches a runtime
  inside the modules. Since light-cljs-281 the driver binds
  `globalThis.lightObserve`, the per-case observer generated programs call,
  for the whole run, and the trace is what the program passed it (still
  empty for a program that never calls it). `tests/support/chunked_execution.rs` (`run_modules`)
  writes the files to a fresh temporary directory and runs the driver;
  `tests/compiler/features_example.rs` runs the example at every level. By
  hand, into an empty directory `DIR`: `cargo run --quiet -- --compile --entry
  app.main --source-root examples/features --esm > DIR/app.main.js`, then
  `node tests/support/chunked_execution.mjs DIR app.main.js`. Each feature
  logs its own result line. Until `light.lazy` existed, `app.main/start`
  called both features directly; since light-cljs-271 it holds
  `(lazy/loadable app.editor/start)` and `(lazy/loadable app.reports/start)`
  and loads them in sequence, printing the same lines.

Done when: the example compiles today as a single file, the driver runs it,
and the table has numbers.

### Phase 1: lazy references in the language, single file

Scope: `light.lazy` works end to end with every target resident, so programs
can be written against the API before chunked output exists. No multi-file
output.

- **Analyzer.** Recognize `light.lazy/loadable` with a literal qualified
  symbol or namespace symbol. For a Var target, reserve its `GlobalBinding`
  by namespace and name the way existence probes already do
  (`GlobalBinding::declared`, `src/analyzer.rs:2241-2256`), record the site
  in a new `Program::lazy_targets: Vec<LazyTarget { site, namespace, global:
  Option<GlobalId> }>`, and lower to `ExprKind::Loadable { target }`. After
  all namespaces are analyzed, a Var target without a definition is an error
  at the site ("lazy target app.editor/start is not defined").
- **Loader.** Turn the analyze loop of `analyze_namespace_entry_with_provider`
  into a worklist: after analyzing a source, visit each lazy target namespace
  it introduced, which appends the target's unvisited static dependencies and
  the target itself to `ordered`. A lazy target that is already `Loaded` is
  resident. Record the static require graph in the `Program`
  (`namespace_requires: Vec<Vec<usize>>` by source index) for Phase 2.
- **As built (light-cljs-270).** The analyzer and loader above, with the
  form emitted as `null` until the library exists. Details the list leaves
  open:
  - `LazyTarget::namespace` indexes `Program::lazy_namespaces`, the distinct
    target namespace names in first-site order, because a target's source
    index is unknown at the site (the REPL and the macro host analyze sites
    of namespaces no loader orders). Map a name to its source index through
    `Program::namespaces`, which for a whole program is the source order.
    `ExprKind::Loadable { target }` indexes `Program::lazy_targets`;
    `program_fragment` keeps both tables when it snapshots or appends.
  - `light.lazy/loadable` is recognized fully qualified without a require of
    `light.lazy`, like a special form, and through an alias of it; the
    argument must be an unquoted symbol. Errors: "light.lazy/loadable requires
    one literal symbol naming a Var or a namespace", "lazy target app.main/f
    is in the current namespace app.main", "lazy target namespace cljs.core is
    not a source namespace" (also `js`), "lazy target namespace app.nope not
    found: expected app/nope.cljs or .cljc in explicit source roots" (from the
    loader) and, once every namespace is analyzed, "lazy target
    app.editor/start is not defined".
  - Sources reached only through lazy edges follow the entry
    (`Program::namespace_lazy_only`), so they run after it, as their chunks
    would. The program's value stays the entry's: its last top-level form
    stores its value in a global no symbol names (`<entry value>`) and the
    program returns that global after the lazy sources run. Programs without
    such sources are unchanged.
  - `analyze_namespaces` over a fixed slice of sources reports a lazy target
    whose namespace is not among them as "lazy target namespace app.nope not
    found".
  - The REPL (`EvalSession` and the `load-file` callback) records the
    loadables of each form it analyzes and loads each namespace it has
    neither loaded nor entered, as a `require` would, once no namespace is
    loading: before a top-level form runs, or after the outermost namespace
    load when the form belongs to a namespace being loaded, as the loader
    visits targets after their source. So a target may require the namespace
    naming it. Var targets are checked after those loads, at their sites.
- **Library.** `src/compiler_libraries/light/lazy.cljs`: `Loadable` with
  `IDeref`, and the functions `load` and `loaded?`. Register it with the
  bundled first-party libraries in `src/compiler_libraries.rs`, which the
  `Loader` consults after the source roots.
- **Emitter.** `Loadable { target }` emits `new Loadable(null, () => g<n>)`
  (or `() => null` for a namespace target). `load` on a resident loadable
  resolves on the next microtask so timing matches a chunked build as far as
  "asynchronous" goes.
- **As built (light-cljs-271).** The library, emitter and REPL above, with
  `examples/features` switched to the API. Details the list leaves open:
  - `Loadable` has two fields, `importer` (nil while the target is resident)
    and `getter`. `@l` calls the getter and throws "light.lazy: the target of
    this loadable is not loaded" when an importer is set; `loaded?` is
    `(nil? importer)`; `load` is `(.then (js/Promise.resolve) (fn [_] @l))`, so
    it reads the value on a later microtask and rejects with the deref's
    error. Chunk state for a set importer is Phase 2's.
  - `ExprKind::Loadable { target, class }`: `class` is an ordinary
    `ExprKind::Global` read of `light.lazy/Loadable`, reserved by name at the
    site like a lazy Var target, and the only child. So `retain` keeps the
    type, rapid type analysis counts it instantiated (a constructor read), and
    inference types the site as an instance of it, as for `new`. The target is
    no child: the site does not read it until loaded.
  - The type must exist when the site's source runs. The loader makes a
    source whose compiled forms spell `light.lazy/loadable` (syntax quote
    included; quoted data and unselected reader-conditional branches not)
    require `light.lazy` after its own requires, so the namespace lists and
    `namespace_requires` include it; `NamespaceAnalysis::finish` reports a site whose source does not
    follow `light.lazy` ("light.lazy/loadable needs light.lazy loaded before
    app.main: require it there", a macro's site in another namespace, or
    `analyze_namespaces` without it) and a `light.lazy` from a source root
    that lacks `Loadable`. The REPL and the `load-file` callback load
    `light.lazy` before a form holding a loadable runs, even while a
    namespace is loading.
  - The minimum of the Preparation bullet below is in, since the example
    must run at `prepare` and `advanced`: `prepare_application` appends every
    lazy Var target to `options.exports`. Namespace targets, the tests that
    nothing more is rooted, and the size measurements are light-cljs-272's.
  - A target namespace only lazy references reach runs after the entry, so
    `@l` during the synchronous startup reads a Var not yet defined (`nil`)
    where a chunked build would throw; `load` resolves after it either way.
    Under `--eval` and `--repl` the session runs promise jobs after each
    top-level form (light-cljs-295), so there a `load` Promise's reactions
    have run by the next form.
- **As built (light-cljs-272).** The Preparation bullet below, the stubs'
  cost measured in the ratchet, and the type trimmed:
  - The lazy Var targets light-cljs-271 appends to `options.exports` are
    the only roots added, and every pass that reads `exports` gets them
    (`retain`, `call_shapes::analyze` and `closed_functions`, `rta::analyze`):
    a Var only a loadable reads keeps its definition, every arity and what
    it reads, and its parameters take any argument rather than what its
    namespace's own calls pass. A namespace target needs no root: a
    namespace's top-level forms are roots in `retain` whatever reaches the
    namespace, and its definitions nothing reads are removed at `advanced`
    as any namespace's are (`tests/compiler/lazy.rs`).
  - A `deftype` costs an `advanced` program the type machinery: in
    `examples/features` the `Loadable` deftype brought `rt$defineType`, 39
    units and 6,553 bytes, most of them the protocol hooks every core
    operation needs once a program type may implement its protocol. The
    runtime owns `Loadable` instead, a class with `deref()` and `load()`
    (254 bytes terse), and effects give it what a deftype implementing
    IDeref has where a program can observe it: `Loadable.deref` wraps
    `derefProtocol` (`@l`, `deref`, `-deref`); `Loadable.name` sets
    `cljs$lang$ctorStr`, `cljs$lang$type` and the `typeNames` entry where a
    printer reads them; `Loadable.type` calls `defineType` with the IDeref
    marker and method where the program defines types of its own or looks a
    protocol up by mask or slot (`satisfies?`, `implements?`); and
    `Loadable.dispatch` makes `@l` and `load` use the IDeref that `extend-type`
    or `specify!` gives, where `installMethods` is kept. A program can also
    read these by name, `(.cljs$core$IDeref$_deref$arity$1 l l)` or
    `(.-cljs$lang$ctorStr lazy/Loadable)`, which keeps none of those
    operations: rapid type analysis finds the names of `LOADABLE_MEMBERS`
    (`src/runtime_slice.rs`) in reachable code of a program that keeps
    `Loadable`, and the slice roots a unit the effect adding them waits for
    and requires anyway (light-cljs-296). Its instances
    carry no `TypeBrand`, which only the compiler form bridge reads, and it has
    no `->Loadable` or `getBasis`. `deref()` calls the getter as the
    deftype's `(getter)` did, `getter.call(null)`, so an IFn object serves
    as one.
  - `light.lazy/Loadable` is `(def Loadable (light.lazy/loadable-type))`, a
    zero-argument special form for the runtime's class
    (`ExprKind::LoadableType`, inert). The sites still read
    `light.lazy/Loadable`, so light-cljs-271's ordering checks stand;
    inference types a site as any value, and rapid type analysis sees no
    program type in it. `load` is `(.load l)`, so a program that only loads
    keeps no `deref`, and the inliner may turn a call into `l.load()`.
  - Measured with the ratchet's `NAMESPACE_FIXTURES`: `loadable-site`, one
    site with its target in another namespace, is 587 bytes at `advanced`
    against 280 for `direct-site`, the same program reading the target
    directly: one site with the whole type and `load` costs 307 bytes, of
    which the runtime factory and class are 286. In `examples/features`
    (ratchet `features`, 69,484 bytes) the two sites cost 392 bytes, 196 a
    site, against `features-direct`, the same example with direct calls:
    each target read directly from a required namespace and
    `Promise.resolve` for each `load` (69,092 bytes). That meets the exit
    criterion below, and
    `size_ratchet::features_loadable_sites_cost_under_200_bytes_each_at_advanced`
    keeps it. Against the example as it was before light-cljs-271, calling
    both features from `start`, the growth is 596 bytes: the targets now
    escape as functions instead of being inlined into `start`, and `start`
    chains two promises.
- **Preparation.** Lazy Var targets join `options.exports` as retention and
  escape roots everywhere `exports` is consulted (`retain`, `call_shapes`,
  `rta`).
- **Tests.** `tests/compiler/lazy.rs`: analysis errors (unknown namespace,
  undefined Var, non-literal argument, a target that requires the loader
  namespace back), REPL behavior (the REPL loads the target like a `require`
  and `load` resolves), and execution at every level through the Node driver
  of Phase 0. Phase-oracle cases for `loadable` in macro-generated code.

Done when: `examples/features` runs at every level with its loads resolving
in one file, and the ratchet shows the loadable stubs cost under 200 bytes
each at `advanced`.

### Phase 2: chunked ES module output at every level

Scope: multi-file output with namespace-granular placement, correct at
`none`, `prepare` and `advanced`. The whole runtime (below `Advanced`) or the
whole-program slice (at `Advanced`) and every constant stay in startup;
Phase 3 splits them.

- **As built (light-cljs-274): the chunk files.** `PreparedApplication::emit_chunks()`
  returns `ChunkedOutput { files }`, one `ChunkFile { name, source, imports,
  namespaces, roots }` per chunk of `chunks::plan` in plan order; the
  manifest and `--out-dir` are light-cljs-275's. The writer is
  `emitter::emit_chunked` (`src/emitter/chunked.rs`), beside `emit_js_mode`
  rather than a mode of it, sharing its parts (`Emitter::new`, the
  registration, the application roots, the helpers):
  - One `Emitter` writes every chunk's forms in plan order, walking
    `program.root` and writing the forms the plan gives the chunk and the
    entries of its namespaces, so the constant table, the runtime roots, the
    `used` set and the reflection decision (both passes) are the program's.
    A chunk is a module: its imports; the prelude helpers it calls (`truth`,
    `host`, `pkg`, each chunk its own); in startup the runtime (`const rt =
    ...` below `Advanced`, `const [rt$add, ...] = ...` at `Advanced`), the
    `core$` cells, the constant table and `lazy$`; its `let`
    declarations, `export let` for the globals another chunk reads; the
    setters others call; an `export { ... }` of its other names others read;
    and its forms in `(() => { ... })()` as the single file runs them,
    startup's with the namespace registration first and as `export default`.
  - What a chunk imports is read off its text: each compiler name it reads
    (not after a `.`) that another chunk declares is imported from that
    chunk, which exports it. The plan's edges without a name read become
    `import "./x.js";`, so a required namespace's effects still run first;
    reads of startup's runtime, cells and constants add edges to startup,
    which the plan does not list. Import statements follow the order in
    which the single file loads the imported chunks' first namespaces.
  - An assignment of a binding another chunk owns, a global (`set!`,
    `binding`'s install and restore, and so `with-redefs`) or a `core$` cell
    outside startup, is `g7$(v)` or `core$inc$(v)`; the owner declares
    `const g7$ = (v) => g7 = v;` and exports it. `g7$`, `core$inc$` and
    `lazy$` are compiler names (`terse::compiler_name`).
  - Hoisted methods (light-cljs-228) are declared in their function's chunk,
    whose globals they read; every other constant stays in startup.
  - A loadable is resident when its target's chunk is its own, startup, or
    one its chunk imports transitively: `new Loadable(null, () => g20)` as in
    the single file. Otherwise it is `lazy$("./app.editor.js", new
    Loadable(() => import("./app.editor.js"), (m) => m.g20))`, or `() =>
    null` for a namespace target, and the target chunk exports `g20`.
    Startup's `lazy$` keeps, per chunk, the module namespace object once an
    importer has imported it, and the loadables of chunks not imported yet.
    It makes a loadable of an imported chunk resident at once, a getter of
    the module's binding and no importer, and gives any other a
    non-enumerable `load` that calls its importer; when the import
    resolves, every loadable of that chunk becomes resident, so `loaded?`
    is whether the chunk has been loaded, and `@l` reads the same fields as
    a single file's loadable. The runtime's `Loadable` and `light.lazy/load`
    do not change, so no single file changes either, a program with
    loadables included. Importing in
    `light.lazy/load` made `examples/features` 223 bytes longer at
    `Advanced` (307 a site against `features-direct`, past Phase 1's 200,
    which `size_ratchet` caught), and importing in the class would have
    changed every `none` and `prepare` output, which embed the whole runtime.
  - Renaming covers all chunks at once. At `Advanced` `terse::terse_texts`
    lexes each chunk's two texts (module-level declarations and forms),
    counts every compiler name over all of them for one table, and rewrites
    each, so a name has one short name everywhere; when one text cannot be
    followed, every chunk takes the hygiene prefix that no chunk's raw text
    contains. Below `Advanced`, a program with raw text prefixes every
    chunk's names with that one prefix. The import, `let` and export lines
    are written afterwards from the final names.
  - `examples/features` is three files at every level, `app.main.js`,
    `app.editor.js` and `app.reports.js`, raw / `gzip -9` bytes: 441,976 /
    107,628, 1,295 / 423 and 13,101 / 2,612 at `none` and `prepare`, against
    455,109 / 109,611 for the single file; 69,587 / 21,575, 276 / 224 and
    832 / 517 at `advanced`, against 69,500 / 21,646. Startup is 87 bytes
    over the single file at `advanced`: `lazy$`, whose names terse output
    keeps, and the export lines outweigh the features' forms.
  - Tests (`tests/compiler/chunks.rs`): the example and runnable versions of
    the planner's fixtures (a shared helper with a top-level effect, a
    feature loading a sub-feature, a feature requiring the namespace that
    loads it beside a resident loadable, `set!`, `binding` and
    `with-redefs` of startup Vars, `with-redefs` of a core function, a probe
    of a sourceless Var, a namespace loadable that only adds methods,
    reflection, raw `js*` with and without the hygiene fallback, a program
    without namespace sources, an entry without forms, loadables of one
    chunk made before and after it loads, a package only a feature
    imports, which only its chunk imports) run as chunks and as the single `--esm` file
    through the Node driver at every level and print the same; a module
    importing every chunk statically links in Node, where a missing export
    is a link error; and each name an import line names is in the export
    lines of the chunk it names, after terse renaming too.
- **As built (light-cljs-275): `--out-dir` and `chunks.json`.**
  `--compile --entry NS --out-dir DIR` writes the files of `emit_chunks`
  and `chunks.json` into `DIR` (docs/compiler.md, "Chunked builds", which
  documents the manifest's shape and the loading API):
  - `--out-dir` implies `--esm`, creates `DIR` and writes only the files
    the manifest names. It works at every level; `--size-report` prints the
    single file's one report, and the source map flags are refused. With
    `--reloadable` it names the dev build's directory instead
    (light-cljs-282).
  - The manifest is a method, `ChunkedOutput::manifest()`, rather than a
    stored field, so it always describes the files. `ChunkFile` gained
    `loadables`, the sites whose target it holds (`LoadableSite { namespace,
    target }`), which the planner collects while it walks the items
    (`Chunk::sites`): the namespace is the one whose top-level form holds
    the site, so it names the chunk that issues the import.
  - A plan of one chunk is written by `emit_esm`: the chunk writer declares
    globals at module level, where the single file declares them inside its
    function, so the two texts differ. A program without a loadable whose
    target another chunk holds is therefore one file, byte for byte its
    `--esm` output.
  - examples/features is the startup, editor and reports files plus
    `chunks.json` at every level, and no shared file: at namespace
    granularity `app.shared` is startup's, since `app.main` requires it.
    The `app.editor+app.reports.js` of "Goal" waits for Phase 3's
    definition-level placement.
  - The playground's WebAssembly module gained `compile_chunks`, which
    returns the file names, their sources and the manifest as values.
  - Tests (`tests/compiler/chunks.rs`): the CLI writes examples/features
    into a directory it creates, at every level, beside a file it leaves
    alone and over a stale chunk it replaces; the file set and contents are
    the library's, the manifest is checked in full, and the directory run
    by the Node driver prints what the single `--esm` file prints. The
    namespaces example, without loadables, writes one file equal to its
    `--esm` output and the same `--size-report`. The source map flags are
    refused before anything is written. The manifests of the sub-feature,
    resident-target and namespace-target fixtures are checked at every
    level.
- **As built (light-cljs-276): the fixture matrix and the two hosts.**
  - `tests/fixtures/chunks/` holds the eight fixtures of "Tests" below, a
    directory each: `features` (examples/features, whose sources stay
    there), `diamond`, `sub-feature`, `requires-loader`, `shared-effect`,
    `startup-vars` (`set!`, `binding` and `with-redefs` of startup Vars,
    read back in startup), `package` and `methods`. Each holds a program of
    entry `app.main`; `stdout.txt`, what it prints; and `membership.tsv`,
    its chunks in manifest order with their namespaces, roots, and imports
    below `Advanced` and at `Advanced`. The imports differ where `Advanced`
    code reads nothing of a chunk: the diamond's shared chunk imports
    nothing there, and the shared-effect features import only the shared
    chunk. A fixture's `packages/` is copied into `node_modules` for its
    runs.
  - `chunks::matrix` runs a fixture at every level, one test per fixture.
    The CLI's `--out-dir` writes the chunks, which are read back through
    `chunks.json`: the directory holds exactly the files it names, of the
    sizes it gives; each file's import lines name its imports in order,
    and the chunks they name export every name imported; each file's
    `import()`s are the chunks of the loadables its namespaces hold that it
    does not reach statically; the namespaces partition the program's, with
    each root and target in its chunk; every import names an earlier chunk
    whose signature, derived as the roots whose chunks reach it through
    imports, strictly contains the importer's; and the rows are
    `membership.tsv`'s. Run by the Node driver, the chunks print
    `stdout.txt` and return nil, as the single `--esm` file does, and a
    module importing every chunk links. The planner's `check` runs on each
    program too. The runnable programs of light-cljs-274 that became
    fixtures left their record and file assertions to the matrix and keep
    what it does not check: the full manifests with their loadable sites,
    the resident loadable's single `import(` and the package's one
    importer.
  - The `--esm` package tests gained a chunked variant
    (`npm_packages::bundler_chunks_import_packages_only_in_the_feature_chunk`):
    `npm.split` loads `npm.bundle` lazily, and at every level `npm.bundle`'s
    chunk imports the packages its own `--esm` module imports, startup
    imports none, and the chunks print the module's line in Node.
  - `examples/features/index.html` loads the chunks beside it in a browser
    without a bundler, served by `site/build.bb`'s server from files staged
    in `site/dist`, and is Vite's entry for the same files; both print the
    single file's three lines at every level ("Verification"). That
    meets "Done when" below for examples/features as three files.
- **Placement, namespace level.** New module `src/chunks.rs`:
  `plan(program, prepared) -> ChunkPlan`. Roots from `lazy_targets`; static
  reach per root over `namespace_requires`; every top-level form pinned to
  its namespace's signature; chunk ids by distinct signature with startup
  folded in; import edges from the require graph and from reads of globals
  across chunks (the latter are redundant at this phase but the code path is
  the one Phase 3 needs); the setter set per owner chunk; a stable name per
  chunk. Debug-assert monotonicity on every edge.
- **Emitter.** `emit_js_mode` gains a chunk filter and per-chunk declaration
  lists. One `Emitter` instance emits all chunks in order so the constant
  table, runtime roots and `used` set are shared; each chunk's text is a
  separate `String` with its own `let` block. A read of a global owned by
  another chunk adds it to that chunk's export list; an assignment adds the
  setter. Startup exports `rt` (below `Advanced`) or its `rt$` bindings (at
  `Advanced`) that other chunks read, and the `core$` cells with setters.
  `Loadable` sites emit `import("./<name>.js")` or the resident form.
- **Terse.** `terse_program` renames over the concatenation of all chunk
  texts and rewrites each; import and export lines are written afterwards
  from the final names. Raw `js*` text in any chunk joins the identifiers the
  short names avoid (light-cljs-294); only a program that falls back to the
  hygiene prefix (a hole inside a raw string or comment) keeps it.
- **Output.** `PreparedApplication::emit_chunks() -> ChunkedOutput { files:
  Vec<ChunkFile { name, source, imports, namespaces, roots }>, manifest }`,
  with the manifest as JSON (chunks, files, imports, member namespaces, root
  namespaces, loadable sites, bytes). CLI: `--out-dir DIR` after `--compile
  --entry`, which implies `--esm`, writes the files and `chunks.json`; without
  it the output stays one file with every loadable resident, so nothing
  existing changes. Library callers and the playground get the files as
  values.
- **Tests.** `tests/compiler/chunks.rs`: for fixtures in
  `tests/fixtures/chunks/` (the features example; a diamond; a feature that
  loads a sub-feature; a lazy target that requires the loading namespace back;
  a shared namespace with a top-level effect; `set!` and `binding` of a
  startup Var from a feature; a package used only by a feature; a loadable of
  a namespace with only `defmethod`s), at every level: the chunked run prints
  what the single-file run prints; membership of each namespace matches an
  expected table; every import edge targets a strict superset; Node links
  every chunk (a missing export is a link error there); and the manifest
  lists what the files contain. The `--esm` package tests gain a chunked
  variant: a feature's package import appears only in its chunk.

Done when: `examples/features` loads as three or four files in Node and in a
browser without a bundler, and in Vite with the files as the entry, with
identical output to the single file at every level.

### Phase 3: definition-level placement at `advanced`

Scope: the startup chunk of an application with lazy features is the size of
the application without them. Motion of inert definitions, constants and
runtime units toward their first readers; per-chunk runtime factories.

- **As built (light-cljs-277): definitions move toward their readers.**
  - `PreparedApplication::removable(id)` surfaces the `Retained::candidates`
    of the last `retain` at `Advanced` (none below). `chunks::plan` walks
    the flattened root once with `program_fragment::children` and records
    per item the globals it reads (every global reference, in function
    bodies too, and the dispatcher of each program protocol method that a
    method of a type, `reify` or extension names by key, since a `deftype`'s
    method table may write the dispatcher's binding), the globals it defines
    with a value (`application::globals` of a `def` with a value, a
    `defprotocol` or a `deftype`), the globals it assigns and its `loadable`
    sites.
  - An item is movable when it is a removal candidate or a
    `DefProtocol` or `DefType` form (not a raw `deftype*`, whose body runs
    when it defines the type), defines a global, is the only item that
    defines each global it defines, defines no `loadable` target and holds
    no site ("Items and signatures"). A `declare` defines nothing, so a
    forward-declared function moves.
  - Signatures are bitsets per item: one walk per root from the pinned
    items of its reach (startup's also from those of namespaces no root
    reaches), following each read to its one defining item when that is
    movable. A movable item that no walk reaches, one that only
    `ApplicationOptions::exports` keeps, is pinned, and the walks run again.
    A chunk is a distinct signature of a namespace or a movable item, so a
    chunk may hold moved definitions alone, without a namespace or a root:
    `app.editor+app.reports.js` of examples/features holds
    `app.shared/documents` only. A global belongs to its definition's chunk
    (`ChunkPlan::global_chunks`), so the setters of light-cljs-274 carry
    over: a feature's `set!` of a definition that moved into the shared
    chunk calls the shared chunk's setter. Debug builds also assert, on
    every read, that the reader's signature is within its movable
    definition's.
  - Items keep program order within a chunk. A definition precedes in the
    single file every item that reads it while the program loads (an inert
    initializer reads globals only by constructing types defined earlier),
    so this order puts a moved definition before the first item of its
    chunk that reads it; hoisting it further, above a forward reference in
    a function body, could run it before the type it constructs. A chunk's
    import lines follow its imports' first namespaces, and a chunk of moved
    definitions alone, which have no effect, its first definition's
    namespace. A namespace chunk whose forms all moved, or emit nothing
    at `Advanced`, is still written, as at Phase 2 (the `diamond`
    fixture's shared chunk is `(()=>{})();`); light-cljs-299 drops such
    chunks.
  - The reflection gate needs the written forms: `reach_reflection` is a
    property of the runtime names emission records. `emit_chunks` writes
    the definition-level plan; when that first write reaches reflection and
    the plan moved a definition, `emit_chunked` returns `None` and
    `emit_chunks` writes `chunks::namespace_plan` with the namespaces'
    registration, the second write such a program costs the single file
    too. `chunks::plan` itself does not know about reflection.
  - `ChunkFile::definitions` and `"definitions"` in `chunks.json` list the
    definitions a chunk holds of namespaces another chunk holds, by the
    first global each defines, so the manifest still describes the whole
    partition.
  - A `defmulti` stays pinned ("Open questions"), and its `defmethod`s in
    their namespaces' chunks: the pellet's "a `defmethod` stays in its
    namespace's chunk although its `defmulti` moved" holds for the methods
    only.
  - examples/features at `advanced`, terse, raw / `gzip -9` bytes, before
    (namespace level, light-cljs-274) and after:

    | File | Namespace level | Definition level |
    | --- | ---: | ---: |
    | `app.main.js` | 69,587 / 21,575 | 69,539 / 21,551 |
    | `app.editor+app.reports.js` | none | 63 / 81 |
    | `app.editor.js` | 276 / 224 | 361 / 255 |
    | `app.reports.js` | 832 / 517 | 874 / 531 |

    The shared definitions only the features read are `app.shared/documents`
    and `app.shared/index-by` (`pluck` is inlined into `app.reports`), 48
    bytes of statements in the namespace-level startup chunk, `(N=aq);` and
    `(au=function(av,Q){return k(l(av,Q),Q)});`; startup shrinks by those 48
    bytes. Their two names in startup's `export let` line (5 bytes) are
    offset by the two names startup now exports to them (5 bytes): the
    constant `documents` holds and the runtime's `zipmap`, which leave
    startup with constants (light-cljs-278) and runtime units
    (light-cljs-279). The rest of startup is the whole-program runtime slice
    and every constant, as at Phase 2. `label` stays, since the menu reads
    it.
  - Tests (`tests/compiler/chunks.rs`): `membership.tsv` gained a column,
    the definitions each chunk holds at `Advanced` of namespaces other
    chunks hold, and a chunk only `Advanced` has gives `n/a` as its imports
    below it. Two fixtures joined the matrix: `definitions`, where
    `app.shared/fmt`, which both features read, lands in their shared chunk,
    a helper and a protocol only the editor reads land in the editor's
    chunk with the types implementing it (the dispatcher defined before
    them), and each feature's `defmethod` stays in its own chunk while the
    `defmulti` stays in startup; and `reflection`, whose editor looks
    namespaces up, so its helper stays in startup although the plan alone
    would move it. The planner's `check` accepts a movable item in a chunk
    whose signature its namespace's contains, and only at `Advanced`. A
    program run at every level moves a record with the definition
    constructing it, a forward-declared helper, and a Var a feature assigns
    (through the shared chunk's setter), and keeps a protocol a startup
    `extend-type` names, a Var defined twice and a function holding a
    loadable in their namespace. A raw `deftype*` whose body prints stays in
    startup and prints first, as in the single file; a definition only
    `ApplicationOptions::exports` keeps stays in startup with the helper it
    shares with a feature. examples/features plans the shared chunk at
    `Advanced` and its namespace plan stays the three files of Phase 2.

- **As built (light-cljs-281): generated programs with lazy edges.**
  - `lazy_program(seed)` (`tests/support/generated.rs`) draws a namespace
    program of four to six namespaces from the streams `namespace_program`
    uses and turns one or two namespaces, none of them the entry, the
    protocol's or the first, into lazy roots. Edges into a root are dropped
    before any form is drawn, so the forms differ from `namespace_program`'s
    for the seed, whose text stays as it was. Each root defines a function
    last, the target of a loadable in the entry, and the entry's value is a
    function that loads the roots in order, then calls each root's function
    and observes its value, then computes the entry's usual result. Loading
    every root before the first observation makes the traces comparable: the
    single file runs the lazy sources right after the entry, the chunks when
    they load, and nothing is observed in between.
  - Every definition but the protocol holds a sentinel string: a function or
    method body observes it, and a `def` is `(do (identity "sentinel ...;")
    value)`, which leaves a `def` of a literal one removal could drop. The
    generator records the globals each form names and places each sentinel
    (`Placement`): `LazyNamespace` when only lazy roots reach its namespace;
    `LazyReaders` for a movable definition (a `defn`, a `def` of a literal
    or a keyword, a `defrecord` or `deftype`) of a namespace startup holds
    that only code lazy roots reach reads; `Startup` otherwise. Drawn forms
    seldom leave such a definition, since startup's top-level forms read
    most of what their namespaces define, so the first root also requires a
    namespace startup loads (the entry requiring it if nothing else does)
    that ends with a `def` of a literal, a `deftype` when it sees the
    protocol and a function reading both, which only the root's function
    calls. `resolve` reaches reflection, which keeps every definition in its
    namespace's chunk, and 11 of the 12 fixed seeds called it, so a lazy
    program may call it in one seed of four (`LazyEdges::reflective`) and
    reads through `var` otherwise.
  - `generated_optimization::lazy_seed_00` ... `lazy_seed_11` compile each
    fixed seed at every level with `emit_chunks` against `emit_esm`.
    `chunks::check_manifest` checks the files and `chunks.json`, now also
    that every chunk holds a namespace or a definition (its code may still
    be `(()=>{})();` at `Advanced` until light-cljs-299). Startup holds no
    namespace only lazy roots reach and no `LazyNamespace` sentinel at any
    level, and at `Advanced`, without `resolve`, no `LazyReaders` one, while
    another chunk holds one; at `None` every sentinel is in some chunk; and
    through the one Node driver the chunked record (trace, stdout, outcome)
    equals the single file's, which is the same at every level and shows
    each root loading. All twelve seeds run, each test in about 1.2 s against
    1.6-2.4 s for a seed of the garbage properties, so the chunked variant
    keeps every seed; `LIGHT_CLJS_GENERATED_COUNT=480` passed too. Seven of
    the fixed seeds do not call `resolve`, and their two or three
    lazy-reader definitions each leave startup at `Advanced`.

- **As built (light-cljs-278): constants follow their readers.**
  - The emitter records, per entry of the `Advanced` constant table, the
    signatures of the chunks whose code reads it (`Emitter::constant_reference`
    while the chunk is written) and the entries its initializer reads (the
    parts of a composite or a shape, which are earlier entries). After the
    chunks are written, `place_constants` (`src/emitter/chunked.rs`) walks the
    entries from the last down and adds each one's signature to its parts', so
    an entry's signature is the union over its reading chunks and the
    composites holding it. Per-chunk recording is enough: a chunk's signature
    is its items', except startup's, which holds every root, and an entry
    read in startup is startup's either way. A hoisted method (light-cljs-228)
    keeps its function's chunk, whose globals its body reads; its body's
    reads count as that chunk's.
  - `ChunkPlan::place` gives each signature its chunk: startup when it holds
    startup's root, else the chunk of that signature, which it adds, holding
    no item, when the plan has none. Added chunks are named as shared chunks
    are and sorted among the plan's by size, after those of their size, so
    every import still precedes its importer; chunk ids change, and
    `emit_chunked` returns the plan it wrote, which `ChunkedOutput::new`
    describes. A chunk of constants alone has no namespace, no definition
    and no forms function, only its declarations and its exports, and its
    importers list it last, after the chunks whose namespaces they load,
    since only their code needs it. `ChunkFile::imports`, and so the
    manifest's `imports`, now follow the import lines rather than chunk ids:
    a chunk of constants alone of three roots precedes a namespace chunk of
    two in the plan, but follows it in its importers' lines. Placement
    happens after writing, which decided each loadable's residency from the
    plan's imports: an import a constant adds reaches only chunks whose
    namespaces every root of the importer reaches statically, so at worst a
    loadable of such a chunk stays an `import()` of a chunk that has loaded,
    which resolves at once.
  - Each chunk declares its entries in the table's declaration order (index
    order, except that a hoisted method follows the entries its body
    declared): the parts of a composite come before it, in its chunk or in a
    chunk it imports, whose signature contains its own. Names are the table's
    `k<n>` and `c<n>`, renamed with the rest, and read off the text as
    before: a reader imports an entry from its chunk, which exports it. Debug
    builds check that every constant a chunk imports comes from a chunk whose
    signature strictly contains its own. Keyword interning is one table at
    run time and each keyword is declared once.
  - examples/features at `advanced`, terse, raw / `gzip -9` bytes, before
    (light-cljs-277) and after; `none` and `prepare`, which write no table,
    are byte for byte as before:

    | File | Before | After |
    | --- | ---: | ---: |
    | `app.main.js` | 69,539 / 21,551 | 69,000 / 21,242 |
    | `app.editor+app.reports.js` | 63 / 81 | 632 / 397 |
    | `app.editor.js` | 361 / 255 | 361 / 258 |
    | `app.reports.js` | 874 / 531 | 874 / 531 |

    Startup keeps the two keywords its menu reads, `:editor` and
    `:reports`; the table of `app.shared/documents` (eleven keywords, four
    strings, four sets, four maps and the vector) moves to the shared chunk
    with it, and both features import the keywords they read from there.
    The rest of startup is the whole-program runtime slice, which
    light-cljs-279 splits.
  - Tests (`tests/compiler/chunks.rs`): a `constants` fixture joined the
    matrix, whose editor and reports share `app.common` and whose editor and
    settings share only a keyword. A keyword only the editor reads is in the
    editor's chunk alone, one both features read in their shared chunk
    alone, once; a map literal of that keyword read by both lands with it,
    and one only the editor reads in the editor's chunk, which imports the
    keyword; the keyword the editor and the settings share is a chunk of
    constants alone, `app.editor+app.settings.js`, which only `Advanced` has
    (`n/a` in `membership.tsv`), and so is the one all three features read,
    `app.editor+app.reports+app.settings.js`, which the editor imports after
    `app.common`'s chunk. Hoisted methods, map shapes and a lazy cell
    of 1,101 items are placed by their readers too, and print what the
    single file prints at every level. The features example's startup
    declares only its menu's keywords. No other fixture's membership
    changed: the features example already imported its shared chunk, and no
    other fixture's chunk gained an import.
    `chunks::check_manifest`, which since light-cljs-281 asserts that every
    chunk holds a namespace or a definition, accepts a chunk of neither
    only at `Advanced`, with no root or loadable, exporting a name another
    chunk imports.

- **As built (light-cljs-299): chunks that hold no code are dropped.**
  - At `Advanced` a chunk can hold no code: removal dropped its
    namespaces' definitions (the `diamond` fixture's `app.format/line` and
    `app.text/shout` are inlined into both features), they moved toward
    their readers (a namespace two features require whose `words` only the
    editor reads and `shout` only the reports read), or they are
    `declare`s. Such a chunk was written as `(()=>{})();` with its import
    lines, and every chunk below it imported it: a request and a module for
    nothing.
  - `chunks::plan` decides from the items, not the written text: at
    `Advanced` (`plan_with` with definitions moving), after the imports are
    known, `ChunkPlan::drop_silent_chunks` drops each chunk other than
    startup that holds no root, owns no global that an item of another
    chunk reads or assigns, and whose items are all `nil` (what removal
    leaves of a definition) or definitions without a value, which
    `Emitter::emit_expr` skips at `Advanced` in output that registers no
    namespace. An importer of a dropped chunk imports its imports instead,
    in id order, so a chain of dropped chunks resolves; each has a
    signature strictly containing the dropped chunk's and so the
    importer's, and the graph stays acyclic with every edge to a strict
    superset. A dropped chunk holds no root, so no loadable loads it, and
    no setter, since no other chunk assigns its globals.
  - Startup lists the dropped chunk's namespaces and holds their items,
    which write nothing there; `ChunkPlan::drops_chunks` is whether startup
    holds a namespace only lazy roots reach. Startup is the one chunk every
    root that reaches the namespaces loads, and the first: a namespace's
    chunk still has a signature containing the namespace's, so a definition
    of it that moved still moved down, out of a chunk that does not import
    its new one, and startup stays first in every importer's lines, which
    follow their imports' first namespaces. A feature's chunk, which some
    roots of the namespace never load, would contradict both.
  - Reflection: output that registers namespaces writes each namespace's
    switch of `*ns*` in its chunk, so a dropped chunk would not be empty
    there. `emit_chunked` returns `None` for forms that reach reflection
    when the plan moves a definition or drops a chunk, and `emit_chunks`
    writes `chunks::namespace_plan`, which drops nothing.
  - Constants are placed after the plan: a keyword both features of a
    dropped chunk read still gets a chunk of its signature, a chunk of
    constants alone (light-cljs-278) of the dropped chunk's name, holding
    no namespace.
  - The `diamond` fixture at `advanced`, terse, raw / `gzip -9` bytes,
    before and after; `none` and `prepare`, and every file of
    examples/features, whose shared chunk holds `app.shared/documents`,
    are byte for byte as before:

    | File | Before | After |
    | --- | ---: | ---: |
    | `app.main.js` | 23,593 / 7,998 | 23,593 / 7,998 |
    | `app.editor+app.reports.js` | 12 / 32 | none |
    | `app.editor.js` | 143 / 132 | 105 / 115 |
    | `app.reports.js` | 153 / 139 | 115 / 121 |

    Each feature loses its `import "./app.editor+app.reports.js";` line
    (38 bytes), and loading one feature is two requests instead of three.
  - Tests (`tests/compiler/chunks.rs`): `membership.tsv` gives `n/a` as
    the imports at `Advanced` of a chunk `Advanced` drops, and startup's
    row there gains its namespaces; the `diamond` fixture's shared chunk is
    such a chunk, and its features import startup alone.
    `chunks::check_manifest` rejects at every level a chunk that is no
    root's whose code is `(()=>{})();` and import lines, so the fixture
    matrix and the generated lazy programs hold no such chunk. The
    planner's `check` requires every namespace in the chunk of its
    signature, except at `Advanced` one startup holds whose items there are
    all silent. A shared namespace whose definitions all move is no file,
    its definitions listed by the features that hold them; an importer of a
    dropped chunk imports the chunk of the namespace it required, whose
    top-level effect still prints before the feature runs; a keyword the
    features of a dropped chunk share gets a chunk of constants alone; a
    chunk that only declares a Var one feature assigns and another reads is
    kept, with the binding and its setter; and a program that reaches
    reflection keeps the chunk. Two plan tests whose
    helper is inlined at `Advanced` changed: the diamond plan drops its
    shared chunk there, and the hashed-name test's helper prints, so its
    shared chunk keeps code at every level. The generated lazy programs'
    check that startup lists no lazy-only namespace allows, at `Advanced`
    only, one the plan dropped into startup with silent items; no fixed
    seed, nor 480 seeds, drops a chunk.

- **As built (light-cljs-279): runtime units follow their roots.**
  - While the chunks are written, `Emitter::runtime_root` records each
    runtime name the output reads twice: in the program's `RuntimeRoots`, as
    before, which decide reflection and stay the single file's, and in
    `Chunks::runtime_root`, under the chunk being written or the constant
    whose initializer is being written (`Initializer`), which is placed with
    the constant. Startup records the namespace registration and the units
    kept for what the program names rather than reads
    (`record_application_roots`: protocol masks, `Loadable` members,
    upstream fields); the latter is conservative, since rapid type analysis
    keeps only the first site naming each, so a unit a feature alone names
    stays in startup until it records every site. An output-level core cell, `let core$str =
    rt$str;`, is declared in startup for every chunk, so startup records its
    binding as a call; each chunk reading the cell records it with its own
    use, a call when the cell is the callee of a plain `.call(null, ...)`, so
    the variadic arity of `str`, which `clojure.string` makes a cell, is
    installed only where some chunk passes the value on. The single file
    still counts the cell's value as escaping.
  - `RuntimeManifest::signatures` gives each unit the roots that need it, a
    bitset per unit, from each chunk's demands with the chunk's signature
    and each constant's with the constant's (a signature holding startup's
    root is every root). It is a fixpoint over the manifest graph: requires
    and `effect:` units take their unit's roots, and an `install:` unit the
    roots whose code holds its target's value (a `Value` demand, or a unit
    that requires the target); one root's demands alone keep exactly the
    units with its bit, as `reached_by_uses` keeps them, and all demands the
    single file's slice. Two rules add roots without keeping more units. An
    `effect: ... when:` unit whose condition some root needs that needs none
    of its targets takes every root of its targets: a value one feature
    builds can reach code another loads, so `makeVector.seq` goes with the
    feature that builds vectors when another only walks them (per-root reach
    alone would drop it when both are loaded). And a unit that shares state
    through another unit's binding (`RuntimeManifest::sharers`: it assigns
    the binding, as the wrappers `lookup.trie` and `seq.protocol` do, or
    reads a binding a function assigns after loading, as `hash` does
    `identityCounter`) takes that unit's roots, so both are in one chunk: a
    chunk receives other chunks' bindings as values, which an assignment
    cannot reach. `js_syntax::analyze` reports the assignments (`Unit::assigns`,
    `Unit::call_time_assigns`); the `runtime_manifest` suite lists the 44
    pairs of the runtime.
  - `ChunkPlan::place` places the constants and the units together; a unit
    of a signature no chunk has gets a chunk of its own, named as a shared
    chunk is, holding no namespace. Each chunk's slice
    (`runtime_slice::ChunkSlice`) is its units, the bindings they require
    that other chunks declare (`factory_parameters`, by declaring unit) and
    the bindings it returns: those its own code and constants read and those
    later chunks read or take as parameters, by declaring unit.
    `chunk_factories` writes startup's slice as the single file's factory and
    every other as `((keywords, hash) => { ... return [compare]; })`, called
    with `rt$keywords, rt$hash`, which the chunk imports from their owners;
    terse slices are named by one table over every chunk's units
    (`terse::slice_names`), so a parameter has its binding's name in the
    chunk declaring it, and no other binding of the chunk has that name. A
    slice without returned bindings is an expression statement. Each
    parameter's owner has a signature strictly containing the chunk's
    (debug-asserted), as does every chunk a runtime read imports.
  - A chunk holding an `effect:` or `install:` unit, which no chunk reads, is
    imported, for its effect when no binding of it is read, by every chunk
    of a strictly smaller signature that does not reach it through its
    imports: all its roots need it, and code loaded with it may build the
    values it handles. Startup no longer needs to be every chunk's import: a
    feature that reads nothing of startup's imports only the chunks it reads
    (the `diamond`, `package` and `sub-feature` fixtures' features at
    `Advanced`).
  - `--size-report` with `--out-dir` at `Advanced` prints each chunk's units
    (`ChunkedRetention`, from `PreparedApplication::emit_chunks_reported`):
    a summary line for the build and per chunk, the roots that add units to
    the chunk, its own and other chunks' (with their file), and every unit
    with the root and expression that first demanded it, taking the chunks'
    roots in plan order. Every unit of the build is listed once.
  - examples/features at `advanced`, terse, raw / `gzip -9` bytes, before
    (master `ebeb14e`, which the pellet was rebased onto) and after; `none`
    and `prepare`, and every single file, are byte for byte as before:

    | File | Before | After |
    | --- | ---: | ---: |
    | `app.main.js` | 69,528 / 21,361 | 9,046 / 3,373 |
    | `app.editor+app.reports.js` | 632 / 397 | 54,195 / 17,412 |
    | `app.editor.js` | 361 / 258 | 5,453 / 2,459 |
    | `app.reports.js` | 878 / 534 | 4,513 / 2,096 |
    | all four | 71,399 / 22,550 | 73,207 / 25,340 |

    Startup's 64 units are the 38 the application without its features
    keeps (the lower bound's, 4,184 bytes here), `Loadable` (255) and 25
    units only the features need (2,879 bytes): `lookup.trie` and
    `lookup.members`, which assign `lookup`, the hash-trie lookup and
    hashing they require ("Open questions"). The shared chunk holds the
    collections of `app.shared/documents` and what both features do with
    them (seqs, `into`, `reduce`, the printer), the editor the sorted
    collections, `juxt`, `zipmap` and `println`, the reports `set`, `sort`,
    `filter`, `mapv`, `comp` and `+` with its variadic arity. Loading one
    feature costs its file and the shared chunk, 59,648 / 19,871 for the
    editor and 58,708 / 19,508 for the reports, against the upper bounds of
    64,975 / 20,151 and 64,458 / 19,943 on the same master. The build is
    1,808 bytes larger than before (2,790 gzip): the factories' parameter,
    argument, return and destructuring lists cost 1,573 / 983 ("Open
    questions", measured before the rebase, at `3abc7ad`), and the
    `str` cell's variadic arity, which no chunk reads but to call, is left
    out (`str.variadic`, the one unit of the single file's slice no chunk
    holds).
  - Tests (`tests/compiler/chunks.rs`): in examples/features each unit is
    in one chunk, the sorted collections only in the editor's, the set
    operations only in the reports', the documents' collections in the
    shared chunk, and startup holds exactly the lower bound's units,
    `Loadable` and what the `lookup` wrappers reach; an install that one
    feature needs (`apply str`) runs when that feature loads while startup
    keeps `str`, and one both need is in their shared chunk; the printer of
    collections goes with the collections though startup calls `str`; a
    vector one feature builds and another walks keeps `makeVector.seq` in
    the builder's chunk; and `--size-report` lists every unit once. Four
    expectations changed on purpose: the `diamond` fixture's features import
    a chunk of the runtime both need, named as the dropped shared chunk
    (a file can have two rows in `membership.tsv`, one per side), and its,
    `package`'s and `sub-feature`'s chunks no longer import startup where
    they read nothing of it; so do the features of three plan tests.
    `chunks::check_manifest` accepts a chunk of runtime units that another
    chunk imports, by name or for its effect. The `runtime_manifest` suite
    checks the signatures on small manifests and lists the runtime's
    sharers; the fixture matrix and the generated lazy programs run as
    before.

- **Item signatures.** Extend `chunks::plan` with the item reference graph
  over `flatten(program.root)`: per item, the globals it reads (one walk
  with `program_fragment::children`), the globals it defines
  (`application::globals`), and whether it is movable (`Retained::candidates`,
  surfaced from `prepare_application`, plus `DefProtocol`/`DefType`). One
  BFS per root; signatures as `Vec<u64>` bitsets.
- **Constants.** Record, per emitted top-level item, which constant entries
  it reads (the emitter knows the current item: `site`). Constant signatures
  are unions over readers and over the composites that contain them; each
  chunk declares the entries with its signature, in index order, and imports
  the rest.
- **Runtime units.** Per root, compute `RuntimeManifest::reached_by_uses`
  from the roots demanded by the items and constants that root reaches,
  giving a unit signature; each chunk's slice is the units with its signature,
  emitted by a generalization of `sliced_runtime` that takes the bindings
  available from prerequisite chunks and writes them as factory parameters.
  `effect:` and `install:` units follow the same rule: an install for `+`
  used as a value only in the editor goes to the editor chunk and runs when
  it loads; the uses that need it are all there or in chunks loaded after.
  Terse slices rename per chunk with one frequency table.
- **Reflection gate.** When `reach_reflection` holds, pin every top-level
  form (namespace-level placement as in Phase 2); constants and units still
  split.
- **Size report.** `--size-report` prints one table per chunk, each unit with
  the root and expression that first demanded it in that chunk, and a
  summary line per chunk.
- **Tests.** The membership tables of Phase 2 fixtures gain definitions
  (`app.shared/fmt` in the shared chunk, a helper used only by the editor in
  the editor chunk, a `defmethod` in its namespace's chunk although its
  `defmulti` moved). A "startup equals the application without its
  features" check: compile the features example with its loadables replaced
  by `nil` and the feature namespaces absent, and require the startup chunk
  of the chunked build to be within a fixed slack of that size. Per-chunk
  ceilings in `size_ratchet`. Generated programs (`tests/support/generated.rs`)
  gain lazy edges: one or two namespaces of a seed become lazy roots, with
  sentinel strings per definition, and the property is that a sentinel of a
  definition read only from a lazy root's reach never appears in startup,
  while the trace stays identical to the single-file run.

Done when: the startup row of the Goal table meets its target on the
features example, and the generated-program property holds for the fixed
seeds.

### Phase 4: shaping and tooling

Scope: controls for the chunk graph and support in the hosts, each a small
independent step.

- `--chunk NAME:ns,ns`: merge the lazy roots of the named namespaces into one
  root with the given name, so their features load together. A namespace that
  is not a lazy root is an error. This is the only place an explicit chunk
  graph enters, and it is an overlay on inferred roots.
- `--chunk-min-bytes N`: after placement, merge a chunk smaller than `N` bytes
  into the smallest chunk whose signature strictly contains it (startup when
  none does). Merging up is always safe, since it only loads code earlier.
- *Loader dominance*, an optional refinement for chains: when every
  `loadable` of a root `q` sits in chunks of root `p`, `q` can only ever load
  after `p`, so a chunk with signature `{p, q}` may merge into `{p}` at no
  cost. Implement as a post-pass over signatures; measure before keeping it.
- `(lazy/prefetch l)`: fires the `import()` without awaiting it, and the
  manifest lists each chunk's file and prerequisites so a host can emit
  `<link rel="modulepreload">` for a feature it expects.
- Host-driven entries, `--chunk-entry NS`: a chunk the host loads with its
  own `import()`, exporting the namespace's public Vars by name, for pages
  whose HTML decides what to load. Equivalent to a `loadable` of every public
  Var without a site in the program.
- Playground: show the chunk files as tabs, run them in the worker through
  blob URLs with import specifiers rewritten bottom-up (prerequisites first),
  and show the per-chunk size report.
- Documentation: the loading API and `--out-dir` in `docs/compiler.md` and
  the README; the contract below in `docs/compiler.md`.

### Phase 5: deferred

- **Script-format chunks** for hosts without ES modules: each chunk a classic
  script that registers its exports on one object and resolves a promise, a
  loader helper of a few lines in startup that inserts script tags in
  prerequisite order. The partition and the setters are the same; only the
  chunk writer differs. Not needed for browsers, Node or bundlers.
- **Retries.** A failed `import()` is recorded in the module map by browsers,
  so a second `import()` of the same specifier fails without a fetch; a retry
  needs a cache-busting query, which conflicts with bundler-rewritten
  specifiers. `load` rejects; the application decides.
- **Method stubs** across chunks (`CrossChunkMethodMotion`): a method of a
  startup type used only by a feature could live in the feature behind a
  stub. Rapid type analysis already drops methods no instance can run, and
  the remaining cases are small; revisit with measurements.
- **Separate compilation** of a chunk against a published runtime, source
  maps, web worker chunks and HMR.

## Code-splitting contract

The contract, the deliberate semantic differences between a chunked build
and the single file, lives in
[docs/compiler.md, "Code-splitting contract"](compiler.md#code-splitting-contract),
after "Application preparation", whose levels cite the advanced-mode
contract. light-cljs-275 moved it there when `--out-dir` made chunked builds
available; docs/compiler.md, "Chunked builds", documents `--out-dir`,
`chunks.json` and the loading API.

## Verification

- Differential execution: every chunk fixture and every generated program
  with lazy edges, chunked against single-file, at every level, through one
  Node driver.
- Structural checks in the compiler: monotone import edges (debug assertion),
  no global read from a chunk that does not import its owner (a link error in
  Node makes this fail loudly in tests too), every lazy target defined.
- Membership tables per fixture, which only tighten; per-chunk size ceilings
  in `size_ratchet`; the "startup equals the application without its
  features" check with fixed slack.
- Per-chunk retention reports checked in under `benchmarks/results/`.
- Startup time of the startup chunk against the single file in the `--levels`
  benchmark of `benches/compiler.rs`.
- Compile time of the chunked build against the single-file build on the
  features example and on the largest generated seed.

As built (light-cljs-276), `chunks::matrix` is the differential execution
and the membership tables for the fixtures of `tests/fixtures/chunks/`
(Phase 2, "As built (light-cljs-276)"), with the edge check made again from
`chunks.json`. Since light-cljs-281 generated programs with lazy edges run
chunked against the single file at every level, with the same manifest
checks (Phase 3, "As built (light-cljs-281)"); per-chunk ceilings and the
rest of the list are still to come.

The two hosts are checked by hand, at every level, on examples/features. Its
`index.html` sits beside the chunks, appends each line the program logs to
`#log`, and sets `#status` to `done` when `start`'s promise resolves, so
headless Chrome's `--dump-dom` shows the run. The example is not part of
the site: `site/build.bb` never copies it into `site/dist`, which Pages
publishes. The Vite project is a temporary directory outside the
repository; each level's directory is a Vite root whose `index.html` is the
entry, so Vite bundles startup and turns the features' `import()`s into its
own chunks. Run from the repository root on October 15, 2026 (macOS, Node
24.18.0, Chrome 155.0.8059.79), every page below printed the three lines of
`tests/fixtures/chunks/features/stdout.txt` and `done`.

Through the site server: `bb site/build.bb serve` serves `site/dist` under
`/light-cljs/` and needs only its `index.html`. `site/dist` is ignored by
git, and `bb site/build.bb build` deletes it and assembles it again from its
inputs, so files staged there for the check are never published. http-kit
listens on every interface by default; the check binds it to 127.0.0.1
without changing `site/build.bb`:

```sh
test ! -e site/dist   # with a built site/dist, stage under it and remove only the staged files
mkdir -p site/dist && cp site/index.html site/dist/
for level in none prepare advanced; do
  cargo run --quiet -- --compile --optimize $level --entry app.main \
    --source-root examples/features --out-dir site/dist/features/$level
  cp examples/features/index.html site/dist/features/$level/
done
bb -e '(require (quote org.httpkit.server))
       (alter-var-root (var org.httpkit.server/run-server)
                       (fn [run] (fn [handler options] (run handler (assoc options :ip "127.0.0.1")))))
       (binding [*command-line-args* ["serve" "8790"]] (load-file "site/build.bb"))' &
server=$!
sleep 3
for level in none prepare advanced; do
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
    --user-data-dir="$(pwd)/target/chrome-profile" --virtual-time-budget=5000 \
    --dump-dom "http://127.0.0.1:8790/light-cljs/features/$level/" 2>/dev/null |
    sed -n '/<pre id="log">/,/<p id="status">/p' || true
done
kill $server
rm -r site/dist target/chrome-profile
```

Without a bundler and through Vite, from a temporary directory and a
throwaway server on 127.0.0.1 that sends `.js` as `text/javascript`, as
`site/build.bb` does:

```sh
dir=$(mktemp -d)
for level in none prepare advanced; do
  cargo run --quiet -- --compile --optimize $level --entry app.main \
    --source-root examples/features --out-dir "$dir/$level"
  cp examples/features/index.html "$dir/$level/"
done
(cd "$dir" && npm init -y >/dev/null && npm install --prefer-offline --silent vite@8.0.8 &&
  for level in none prepare advanced; do npx vite build $level --base ./ --logLevel warn; done)

bb -e '(require (quote [org.httpkit.server :as http]) (quote [babashka.fs :as fs]))
       (let [[dir port] *command-line-args*
             root (fs/canonicalize dir)
             types {"html" "text/html" "js" "text/javascript" "json" "application/json"}]
         (http/run-server
          (fn [{:keys [uri]}]
            (let [file (fs/canonicalize (fs/path root (str "." uri (when (.endsWith uri "/") "index.html"))))]
              (if (and (fs/starts-with? file root) (fs/regular-file? file))
                {:status 200 :headers {"Content-Type" (types (fs/extension file) "application/octet-stream")}
                 :body (fs/file file)}
                {:status 404 :body "not found"})))
          {:ip "127.0.0.1" :port (parse-long port)})
         @(promise))' "$dir" 8790 &
server=$!
sleep 2

# The chunks without a bundler, then Vite's build of them.
for page in none/ prepare/ advanced/ none/dist/ prepare/dist/ advanced/dist/; do
  echo "== $page"
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
    --user-data-dir="$dir/profile" --virtual-time-budget=5000 \
    --dump-dom "http://127.0.0.1:8790/$page" 2>/dev/null |
    sed -n '/<pre id="log">/,/<p id="status">/p' || true
done

kill $server
rm -r "$dir"
```

Vite kept `app.editor` and `app.reports` as chunks of their own behind
`import()` and split startup into two files, a 0.07 kB one the page imports
and the rest; at `advanced` it reported the rest as 65.18 kB after its own
minification, against the 69,587 bytes `--out-dir` wrote.

## Open questions

- **Namespace name** for the API: `light.lazy`, `light-cljs.lazy`, or a
  `cljs.loader`-compatible shim (`load` by module keyword plus `set-loaded!`)
  on top of it for porting code. The compiler-owned part is one special form
  either way.
- **Readable per-chunk runtime factories versus module-level units.**
  Settled by light-cljs-279 for module-level units, which light-cljs-303
  writes; until then chunks keep the factory with parameters. The factory
  keeps `sliced_runtime` and the terse unit text as they are; units at
  module level remove one closure per chunk and its lists, at the cost of
  naming the runtime's bindings in the program's table. Measured on
  examples/features at `advanced` at `3abc7ad` (terse, the four files
  72,675 / 25,228 bytes raw / `gzip -9`; 73,207 / 25,340 on `ebeb14e`): the factories' parameter, argument, return and
  destructuring lists and their wrappers are 1,573 / 983 bytes (startup 273
  / 170, the shared chunk 636 / 410, the editor 352 / 216, the reports 312
  / 187); one name table over the program's compiler names and the
  runtime's bindings (a read `rt$x` and the binding `x` one name) gives the
  names outside the lists 3,751 bytes against 3,597 with two tables, so
  module level writes about 1.4 KB fewer raw bytes (2.0% of the build) and,
  since 154 bytes of longer names cannot cost much more than that
  compressed, about 830 fewer gzip bytes (3.3%). Measured with throwaway
  scripts over the terse output: the lists cut out, and the names counted
  as the terse renaming counts them. Units that assign another unit's binding stay with it either
  way: an imported binding is read-only, and a parameter is a copy. The
  same lists cost the single file (advanced-optimization.md, "Open
  questions") once, its return and destructuring lists, a few hundred
  bytes for an application of this size.
- **Units that assign another unit's binding.** Eleven `effect:` units wrap
  an operation by assigning its binding, `lookup = (operation => ...)(lookup)`
  (`lookup.trie`, `lookup.members`, `copyEntries.trie`, and the protocol
  wrappers of `seq`, `first`, `rest`, `next`, `count`, `nth`, `reduce` and
  `derefProtocol`), and a few functions assign state another unit declares
  after loading (`hash` its `identityCounter`). A chunk receives other
  chunks' bindings as values, so since light-cljs-279 such a unit shares the
  binding's chunk (`RuntimeManifest::sharers`): in examples/features the
  menu's keywords reach `lookup`, and the two hash-trie wrappers with the
  lookup and hashing they need, 25 units and 2,879 bytes only the features
  need, stay in startup. Hook installs instead of wrappers
  (light-cljs-302) would let them move, but grow `lookup` in every single
  file that keeps it, which the ground rules leave to the user.
- **Multimethods and motion.** A `defmulti` stays pinned at `Advanced`
  (light-cljs-277): its initializer builds a `MultiFn` from the dispatch
  function, the options and the global hierarchy, which `Inert` does not
  accept, so removal never drops it and only what removal could drop moves.
  A multimethod of a shared namespace that only features call therefore
  keeps its definition in startup, and once the runtime splits per chunk
  (light-cljs-279) the `MultiFn` units with it. Accepting `defmulti` with a keyword or `fn` dispatch
  and the default hierarchy as inert would let it move with its readers
  (light-cljs-300); its `defmethod`s stay pinned in their namespaces either
  way.
- **Granularity of the reference graph for motion.** Items are top-level
  forms; a `defn` with several methods is one item. Finer motion (per method
  entry `g3_1`) is possible with call shapes but has no measured case yet.
- **Loaded state of a chunk another chunk imported.** `loaded?` of a
  chunked build's loadable turns true when a loadable of its chunk has been
  loaded (light-cljs-274), not when the chunk evaluates because another
  chunk imports it statically: chunks carry no load markers, and only an
  `import()` hands `lazy$` the module namespace object a getter reads. A
  `load` then resolves without a request. A chunk could announce itself
  with an object of getters of its targets, at the cost of a marker the
  design leaves out; whether `loaded?` needs that is open.
- **Many lazy roots.** Distinct signatures can fragment shared code into
  many small chunks. `--chunk-min-bytes` and loader dominance are the
  intended answers; whether a default threshold should be on needs numbers
  from a larger application than the example.
- **Bundler interplay.** Vite and Rollup regroup dynamic imports with their
  own heuristics; the compiler's manifest then no longer matches the served
  files. Whether to offer a mode that emits one chunk per lazy root with
  duplicated shared code, for hosts that prefer fewer requests, is open.
  In the Vite 8.0.8 check of light-cljs-276 ("Verification") each feature
  stayed a chunk of its own, under a hashed name, and startup became two
  files.
