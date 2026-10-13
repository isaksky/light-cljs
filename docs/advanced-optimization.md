# Advanced optimization design

Status: design, October 2026. Phase 0's optimization levels, size ratchet and
differential test are implemented (see "Phase 0 extension points"), and so is
Phase 1's unit manifest (see "Phase 1 manifest extension points"); the rest is
not. The work items are tracked as Pellets in the `advanced-optimization`
group; this document is the shared context those pellets cite.

## Goal

Add a release emission mode whose output is smaller and starts faster than
what upstream ClojureScript or shadow-cljs produce through Google Closure
Compiler's `ADVANCED_OPTIMIZATIONS`, for ClojureScript programs. The claim
is scoped: generic JavaScript minification of arbitrary npm code is out of
scope and stays outside the whole-program unit, as the npm design already
assumes.

Targets are emitted bytes before gzip. Upstream bounds are the ratchet
ceilings in ClojureScript's own `build_api_tests.clj` (lines 710-821 of the
pinned clone), which are the only published size facts in that repo.

| Program | Upstream ceiling | light-cljs today | Target |
|---|---:|---:|---:|
| `(js/console.log (+ 1 2))` | 10,240 | 350,927 | < 1,000 |
| `(println "hi")` | 10,240 | 350,814 | < 5,000 |
| `[1 2 3]` printed | 92,160 (lite mode: 16,384) | 350,839 | < 15,000 |
| `{}` printed | 92,160 (lite mode: 32,768) | 350,829 | < 20,000 |
| map/filter/partition pipeline | 34,000 (lite mode) | 351,158 | < 25,000 |

Every program that uses one collection, keyword or core function today
embeds the full 350,414-byte runtime (84 KB gzipped), so the current numbers
are all the same number. The rows are the first five fixtures of
`tests/compiler/size_ratchet.rs`, measured at `prepare`: the printing rows call
`(enable-console-print!)` and then `println`, and the pipeline row logs
upstream's `trivial/core6.cljs` expression with `js/console.log`.

## What upstream does

Research sources are the clones under `research/`. Citations use
`file:line` within those clones.

### ClojureScript emits for Closure; it does not optimize

- Under `:advanced`, `cljs.closure/build` binds `*cljs-static-fns*` true,
  defaults `:emit-constants`/`:optimize-constants` on, and forces Closure
  `:check-types` off (`src/main/clojure/cljs/closure.clj:2515-2572,
  3021-3064`). Type checking off means Closure's only type-based passes,
  DisambiguateProperties and AmbiguateProperties, never fire on CLJS output.
- Known fn-var calls emit `f.cljs$core$IFn$_invoke$arity$N(...)`; unknown
  callees emit the ternary `f.arityN ? f.arityN(a) : f.call(null,a)`
  (`src/main/clojure/cljs/compiler.cljc:1200-1314`). Non-trivial arguments
  are rebound first to avoid exponential duplication (`analyzer.cljc:4034-4077`).
- Keywords are emitted as `new cljs.core.Keyword(ns,name,fqn,hash)` per
  occurrence, or hoisted into a `cljs.core.constants` file under
  `:emit-constants` (`compiler.cljc:368-411, 1918-1940`). Keywords are not
  interned; equality compares `fqn` strings (`core.cljs:3485-3525`).
- `if` tests are wrapped in `cljs.core.truth_` unless the tag is `boolean`
  or `seq` or the region is `*unchecked-if*` (`compiler.cljc:687-707`).
  Tags come from literals, `^boolean` return tags, predicate narrowing and
  the `and-or` pass (`analyzer.cljc:1534-1678, 1733-1855`). `core.cljs` has
  86 `^boolean`, 24 `^clj`, 17 `^not-native` and 11 `^seq` hints in total.
- Protocols: each method has a slot `ns$P$_m$arity$N`; the dispatch fn tests
  `x.slot`, then metadata, then a string-keyed table by `goog.typeOf`
  (`core.cljc:2291-2336`). Native types are extended by writing
  `P["string"]=true; ns._m["string"]=fn` (`core.cljc:1499-1509, 1644-1653`).
  About 50 core protocols get bit positions in `cljs$lang$protocol_mask$`
  (`core.cljc:978-1002`). Direct `x.slot(null,...)` calls are emitted only
  for `^not-native`, known type tags or inside `deftype` bodies
  (`compiler.cljc:1206-1222`).
- Multi-arity top-level fns emit a `switch(arguments.length)` entry plus one
  `f.cljs$core$IFn$_invoke$arity$N` per arity, plus `maxFixedArity` and
  `applyTo` (`core.cljc:3357-3523`). `apply` unrolls to 20 arguments.
- Documented DCE blockers and the fixes they prompted: `defonce` guards,
  forward `declare`s (CLJS-1483), `str` in generated arity errors
  (`4d135560`), `IPrintWithWriter` on natives (`00e15752`), `getBasis` on
  reify, `sorted-map` called at load time (`4fbbfe6f`), Murmur3 changes
  reverted for DCE regressions (`d2d03160`). The experimental `:lite-mode`
  and `:elide-to-string` flags exist because chunked seqs and collection
  `toString` pulling in the printer dominate output size (commit `f9a6856d`:
  a map literal is ~6 KB brotli with the flags vs ~17 KB without).

### Closure ADVANCED is name-based where ClojureScript needs types

All paths are under `research/closure-compiler/src/com/google/javascript/jscomp/`.

- Pass inventory and order: `CompilationLevel.java:167-219` (what ADVANCED
  enables), `DefaultPassConfig.java:836-1143` (early passes, main loop) and
  `519-734` (finalizations: cross-chunk motion, ambiguate, rename
  properties, coalesce, denormalize, rename vars, late peephole).
- Only DisambiguateProperties, AmbiguateProperties and InlineProperties read
  type information, and all three are gated on `checkTypes`
  (`DefaultPassConfig.java:616-619, 1002, 1095`). Everything else, including
  RemoveUnusedCode, DevirtualizeMethods, PureFunctionIdentifier and
  RenameProperties, works on names and AST shape.
- RemoveUnusedCode keeps a prototype property if its name is read anywhere
  on any receiver, or appears as any object-literal key
  (`RemoveUnusedCode.java:527-559, 848-863, 1407-1437`). Once a protocol
  dispatcher is reachable, every type's implementation of that slot stays.
- DevirtualizeMethods requires that every definition of a property name be
  AST-identical, top-level, free of `arguments`, and that every reference be
  a plain call (`DevirtualizeMethods.java:107-151, 241-377`). Protocol
  methods with two implementations never qualify.
- PureFunctionIdentifier keys side-effect summaries by name, not by
  function; unknown callees such as `f.call` receivers get all flags
  (`PureFunctionIdentifier.java:63-66, 152-154, 353-357`).
- RenameProperties renames every unquoted `.foo` in the program to one short
  name; externs are the only protection; quoted access is never renamed and
  never pins the unquoted form (`RenameProperties.java:46-55, 85-86, 303-386`).
  This is the mechanism behind every "advanced mode broke my interop" bug.
- `P["string"]=…` writes are side effects and unremovable. GETELEM reads do
  not keep properties alive and are assumed getter-free
  (`AstAnalyzer.java:426-432`).
- RenameVars assigns names by descending reference count and hands globals
  a same-length batch in source order to help gzip
  (`RenameVars.java:298-309, 466-498`). InlineFunctions uses an explicit
  cost model (`FunctionInjector.java:910-1080`).

### shadow-cljs is upstream plus runtime patches and a separate npm unit

All paths are under `research/shadow-cljs/src/main/shadow/build/`.

- Closure configuration: `closure.clj:125-240, 974-1061`. Type and variable
  checks are off; only `UNDEFINED_VARIABLES` warns, to spot missing externs.
- Patches in `cljs_hacks.cljc:1275-1360`: invoke-shape selection
  (`865-1102`), `defonce` becomes `def` in release (`752-760`), `reify` is
  hoisted to a top-level `deftype` (`774-819`), keyword and symbol constants
  are hoisted per chunk by a custom Closure pass
  (`closure/ReplaceCLJSConstants.java`), `goog-define` carries `@define`.
- Externs inference records every property that crosses the JS boundary
  (`cljs_hacks.cljc:206-254, 479-617`) and emits them as
  `ShadowJS.prototype.p;` so they are never renamed (`closure.clj:383-420`).
- npm code never enters the advanced unit. It is wrapped as
  `shadow$provide[id]=function(require,module,exports)`, optimized under
  SIMPLE, and concatenated before the advanced output; the seam uses
  string-keyed access only (`closure.clj:1975-2088`, `output.clj:298-352`,
  `js.js:43-110`). The compiler comments say SIMPLE is kept because "some
  patterns really require the DCE done by :simple".
- The build report attributes final bytes to source files through the
  source map after the fact (`shadow/cljs/build_report.clj`,
  `closure/SourceMapReport.java`). Its best documented size win gated
  top-level `s/def` registrations behind a `goog-define` because top-level
  effects are never removable (commit `6c9c5`).

## Where light-cljs stands

- `src/application.rs` is the only optimizer: removal of discarded top-level
  defs with provably inert initializers (literals, `fn`, core values, scalar
  collections) and specialization of eager top-level calls to fixed arity
  entries or core aliases. Any retained `js*` disables both.
- `src/emitter.rs:105-134` includes the whole runtime when any used
  expression is a keyword, symbol, collection, core reference, reader
  literal, `with-meta`, var, protocol or type form, a `case` with keyword or
  non-strict tests, a message-bearing `assert`, a rest-arity fn, or when any
  namespace entry exists. Every `--entry` compile therefore embeds it.
- Calls emit `(callee).call(null, ...)` including core operations; every
  `def` initializer is wrapped in an IIFE; every `if` test is wrapped in
  `truth()` unless it is a `BooleanHint`; keywords allocate per evaluation;
  properties are emitted as bracketed string literals.
- `src/js_runtime.js` is one closure: 659 top-level declarations, about 97
  top-level effect statements, a 693-key `core` hub object that is
  `Object.seal`ed and mutable through `set!`, eager prototype patching and
  `EMPTY` constants, an eager `compilerFormBridge()`, and an embedded EDN
  reader. A lexical over-approximation of its reference graph puts
  `keyword`, `vectorFromArray`, `hash`, `equal` and `str` each at about 590
  of 730 declarations (~246 KB) because collection `toString` reaches the
  printer and the printer reaches protocol dispatch. Compile-time-only
  machinery (form bridge, declaration demand, `inspect`, namespace
  registry, macro reader, reader input window) is about 60 KB and is
  unreachable from value construction.
- The IR already carries what Closure lacks: `CoreFnId` with 688 variants
  and per-variant runtime names, `Program.protocols` with arities,
  `record_constructors`, `GlobalBinding` with `constant`, `boolean_tag`
  and metadata, `ExprKind` variants that separate host-facing access
  (`Property`, `MethodCall`, `JsIndex`, `JsGlobal`, `JsTemplate`) from
  compiler-owned names, and dense integer IDs for everything.
- 37 test files patch the runtime anchor text `" = // Original bounded
  runtime"` and 51 reference `"const rt ="`. The REPL, macro host and
  deferred units receive the runtime through the `shared` parameter.

## Design principles

1. **Never emit the problem.** Closure exists to recover structure from
   JavaScript. We have the structure; emit the terse form directly and do not
   build a JavaScript-to-JavaScript peephole optimizer.
2. **Host-facing names are never renamed, by construction.** Only the five
   host-facing `ExprKind`s carry host property names. Compiler-owned ABI
   names (arity entries, `cljs$lang$applyTo`, protocol slots, runtime unit
   names) may be mangled freely. There is no externs concept.
3. **The runtime is a library of units with declared dependencies**, not a
   blob. Reachability runs over user IR plus the unit graph.
4. **Tables over inference where tables suffice.** Purity, return type and
   runtime dependencies are annotated once per `CoreFnId`, not rediscovered.
5. **Rapid type analysis over protocols.** Instantiated types times
   reachable methods decides which implementations exist.
6. **Dense IDs, linear passes, budgets on anything that expands code**, per
   `docs/dos-and-donts.md`.
7. **Release is a separate emission mode.** The REPL and macro host keep the
   full shared runtime; `optimize: false` remains the oracle.

## Phases

### Phase 0: measurement

- `ApplicationOptions` gains a level: `None`, `Prepare` (today's default),
  `Advanced`. CLI: `--optimize none|prepare|advanced`. `emit_js` keeps
  `Prepare` as default so existing contracts hold.
- Ratchet tests: a fixed fixture set with byte ceilings per level, failing
  when exceeded, mirroring upstream's `build_api_tests.clj:710-821`.
- Differential execution: every corpus program runs at all levels with
  identical results. Several tests already do this for `optimize` on/off;
  generalize it.
- Retention report (after Phase 1's manifest exists): for each retained
  runtime unit and global, the expression and source span that first
  demanded it. This is the tool upstream lacks; shadow's build report only
  attributes bytes after the fact.
- Startup benchmark: parse-plus-execute time in Node and QuickJS for the
  fixture set, recorded under `benchmarks/`.

#### Phase 0 extension points

The first three items are implemented. Later phases plug in here:

- **Level.** `OptimizationLevel` in `src/application.rs` is ordered
  `None < Prepare < Advanced`. `level.prepares()` gates today's conservative
  passes; gate release-only work on `level.advanced()`. Never change what
  `None` emits: it is the oracle.
- **Passes.** `prepare_application` marks where `Advanced` passes run, after
  conservative preparation. Emission that depends on the level (runtime
  slicing, the constant table, terse emission) reads
  `PreparedApplication::level()`; thread it into `emit_application` in
  `src/emitter.rs` when the first such pass lands. Until then `Advanced`
  output is byte-identical to `Prepare` output.
- **CLI.** `--optimize` is parsed in `src/main.rs` for single-file `--compile`
  and `--compile --entry`, with or without `--esm`. Namespace builds go through
  `compiler::analyze_namespace_entry` and `prepare_application`, so the CLI
  holds the `PreparedApplication`; add level-dependent reports such as a
  retention report next to it. `--eval` and `--repl` have no level.
- **Ratchet.** `tests/compiler/size_ratchet.rs` holds `FIXTURES`: the five
  Targets rows, `(+ 1 2)`, `(fn [& xs] xs)`, a keyword `case` and a
  `defrecord` with a protocol call, each with `ceilings: [none, prepare,
  advanced]` in bytes of `emit_js()` output. When output shrinks, lower the
  ceiling to the new size rounded up to the next 100 bytes (next 1,000 from
  10,000 up) and record the sizes in `benchmarks/compiler.md`. Add rows for
  new fixtures there. `--no-capture` prints the size table.
- **Differential.** The same file runs every fixture in Node at every level
  and compares against `None` and a pinned expected output. Observations must
  not depend on compiler-owned ABI names (functions print as `#function`).
  `both` and the per-level loops in `tests/compiler/optimization.rs` run the
  preparation regressions at every level, and
  `npm_packages::bundler_modules_run_at_every_optimization_level` runs a
  bundler module built with `--esm --optimize LEVEL` for each level.

### Phase 1: runtime partitioning

Largest win, largest refactor. Expected to take a hello world from 350 KB
to under 1 KB and beat every upstream ceiling on its own.

1. **Unit manifest.** Split `js_runtime.js` into units, one per top-level
   declaration or tightly coupled declaration group, each with declared
   `requires`. A build-time lexical scan validates that no unit references
   an identifier outside its declared set plus ECMAScript globals; the test
   fails on drift. The full runtime remains concatenable from the units for
   the shared/REPL path, so the anchor text stays stable.
   Implemented; see "Phase 1 manifest extension points" below.
2. **Slice in advanced mode.** Roots are the runtime names referenced by
   emitted expressions (every `CoreFnId::runtime_name`, the emitter's
   hardcoded `rt.*` references, protocol/type/reify constructors). Emit only
   the reachable units.
3. **Dismantle the `core` hub.** Emit direct references to unit bindings
   instead of `rt.core.X`. `set!` on a core operation is visible
   program-wide; only mutated operations fall back to an indirection cell.
   `binding` on `*print-length*` and similar dynamic vars keeps a cell.
4. **Make top-level effects demand-driven.** Prototype patching loops,
   `installMethods`, `variadicCore` registrations, `EMPTY` constants and
   factory assignments move into install functions required by the
   constructors or operations that need them.
5. **Separate compile-time machinery.** Form bridge, declaration demand,
   `inspect`, macro reader context, `compilerInput*`, and the namespace
   registry become units reached only by compile-time emission modes or by
   user code that calls reflection (`ns-publics`, `resolve`, `*ns*`,
   `read-string` with auto-resolve). `registerReaderNamespace` and
   `currentNs` emission become conditional on that reachability.
6. **Break the toString edge.** The print unit installs collection
   `toString` on load. A program that never reaches `str`, `pr-str`,
   `print*` or `with-out-str` gets collections without `toString`. This is
   the advanced-mode counterpart of upstream `:elide-to-string` and must be
   documented as such.
7. **Constant table.** Hoist keyword and symbol literals into one interned
   table at the top of the output. `case` on keywords switches on identity.
   Collection literals with only constant parts may be hoisted the same way
   when their construction is proven inert.

#### Phase 1 manifest extension points

Step 1 is implemented (light-cljs-202). Later steps build on it:

- **Format.** `//@unit <name>[ effect[: <bindings>]][ requires: <bindings>]`
  headers in `src/js_runtime.js`, with `//@ ` continuation lines and a final
  `//@exports` section; the module docs of `src/runtime_manifest.rs` give the
  rules. A unit that declares bindings is named after its first one (today
  each is one top-level declaration statement). `requires:` lists exactly the
  other units' bindings the body references.
- **Effects.** A unit with top-level statements other than declarations is
  marked `effect`. Bare `effect` mutates its own bindings; `effect: a, b`
  belongs to other units' bindings, is named `<target>.<what>` by convention
  (`add.variadic`, `builtinTypes.statics`, `core.seal`), and is kept whenever
  a unit declaring a target is kept. Step 4 replaces these units with install
  functions that constructors and operations require.
- **API.** `RuntimeManifest::embedded()` parses the compiled-in runtime once.
  `unit_of(binding)` maps a runtime name to its `UnitId` (dense, in file
  order); `dependencies`, `attached_effects` and `reachable(roots)`, which
  returns a `Vec<bool>` over units, give the slicing closure. A sliced runtime
  is the prologue, the kept bodies (`&source()[unit.body_range]`) in `UnitId`
  order, so initialization order is preserved, and a replacement for the
  export section (`exports.names` lists today's exports). `runtime_text()` is
  the full shared runtime.
- **Embedding.** `build.rs` strips every header line, so
  `runtime_manifest::RUNTIME` (`evaluator::RUNTIME_SOURCE`, the emitter's
  shared runtime) is the file without headers, and the anchors and the size
  ratchet do not see them.
- **Maintenance.** The `runtime_manifest` suite in `tests/compiler/` fails
  on drift; after editing the runtime run
  `LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST=1 cargo test --test compiler runtime_manifest::`
  and review the rewritten `requires:` lists. When step 5 stops exporting
  compile-time machinery, list its units in `UNREACHABLE_FROM_EXPORTS`
  there. Per-unit sizes and reach are in `benchmarks/compiler.md` (the
  `print_runtime_unit_sizes` ignored test prints them).

### Phase 2: whole-program analysis over the IR

All passes are arrays indexed by `ExprId`/`GlobalId`; none allocate maps
keyed by names.

1. **Normalize.** Compact the arena to reachable nodes; record sole,
   non-dynamic, non-escaped initializations for every global (generalizing
   `specialize`); record closure captures and escape of fn values.
2. **Core operation table.** Annotate every `CoreFnId` with parameter and
   return types in a small lattice, purity, and the runtime units it needs.
   This replaces upstream's 86 `^boolean` hints with complete coverage.
3. **Type lattice inference.** Flow-insensitive over locals and def-site
   returns: literals, predicate narrowing in `if`, `^boolean` return tags,
   the core table, record constructors and field types. Consumers: drop
   `truth()` when boolean or seq; `+`/`-`/`<`/`=` on known numbers or
   strings become operators; `(count s)` on strings and arrays becomes
   `.length`; `(not x)` becomes `!x`; `(:k rec)` on a known record becomes
   a field read; `(nth v i)` on a known vector becomes an index.
4. **Rapid type analysis.** Instantiated types are `DefType`, `Reify` and
   reachable runtime constructors; reachable protocol methods come from
   calls and from `satisfies?`. Implementations of uninstantiated types and
   unreached methods are dropped. Calls whose receiver type set is a
   singleton become direct calls. Closure can do neither for protocols.
5. **Call shape.** Direct `f(a, b)` for known targets with no `.call(null)`;
   arity entry chosen at compile time at every call site; no
   `arguments.length` dispatcher when the fn value never escapes; variadic
   calls to known callees build the rest seq directly; no IIFE around `def`
   initializers; local multi-arity fns hoisted when captures permit.
6. **Pure initializer removal.** Extend `inert` to initializers that only
   call pure core operations on inert arguments, so `(def table (into {}
   ...))` disappears when unused.
7. **Re-examine `js*` as a global escape.** The hygiene pass already renames
   compiler identifiers away from raw segments, so raw text can only
   reference its holes and host globals. If that holds, raw templates are
   opaque leaves and do not disable optimization.
8. **Budgeted inlining and folding.** Inline small single-use or tiny pure
   functions under an explicit cost budget with early rejection; fold
   constant arithmetic and string concatenation; eliminate dead branches on
   known booleans; support build-time defines so debug code vanishes.

### Constant collection literals

Tracked as Pellets 218-221 after keyword interning (208). Every literal is
rebuilt per evaluation today: `[1 2 3]` copies, freezes and wraps a fresh
array; `{:a 1}` allocates the keyword and the map each time; `[]` allocates
although the runtime has `EMPTY` singletons. No hash cache exists for
vectors, maps or sets. Sharing is safe because payload arrays are frozen,
`withMeta` returns a new or unchanged object, and transients copy.

Precedents: upstream hoists only keywords and symbols and maps empty
literals to `EMPTY`. shadow-cljs has an opt-in `:shadow-optimize-constants`
pass hoisting fully constant vector/map/set literals into per-namespace
defs, optionally in a delay (`compiler.clj:262-347` in the clone); its
author doubts its value under Closure, which cannot fold against the
result. Here constants are what later passes fold against, and per-site
allocation is paid on every evaluation.

**Constant classification.** nil, booleans, numbers, strings, keywords,
quoted symbols, UUIDs, nested constant collections including the
reader-distinct `LiteralMap`/`LiteralSet`, and `with-meta` of a constant on
a constant. Record literals qualify when fields do but depend on the type.
Not constant: anything with locals, globals, calls or `js*`; `#js` and
`js-obj` (mutable); regex (`lastIndex`); `#inst` (`Date` is mutable).

**Tiers.**

1. Empty singletons at every level.
2. A program-wide hash-consed pool, deduplicated on emitted structure and
   metadata rather than runtime equality; subterm sharing falls out. Eager
   initialization for runtime-only constants, lazy cells for type-dependent
   or large ones, threshold by measurement. Pool entries are inert for
   reachability.
3. Hash caching on persistent collections, then precomputed hashes for
   pool entries via a Rust port of the runtime hashing, proven equal on the
   corpus and on generated programs.
4. Key shapes for maps with constant keys and dynamic values: pooled key
   array, precomputed key hashes and trie layout; sites emit only values.
   Constant-core sets plus `conj`. Vector prefixes are out of scope while
   vectors are flat arrays.
5. Folding of `get`, keyword lookup, `nth`, `count`, `contains?`, `seq`,
   destructuring and `case` membership against pool constants, preserving
   throws.

**Contract change.** Equal literals may be `identical?`. Equality, hashing,
metadata and transients are unaffected.

**Limit.** Sharing between constants and runtime-derived values depends on
the runtime's structural sharing. Vectors are flat frozen arrays with
linear-time update, so `conj` on a constant copies regardless. Trie vectors
are a separate runtime-performance item.

**Testing.** The generated-program harness (217) gains metamorphic
properties for each tier: behavior with the optimization equals behavior
without it at every level, with the generator producing repeated and nested
literals, metadata on literals, transients from literals, shaped maps with
colliding keys, and lookups with missing and out-of-range arguments. Fixed
fixtures pin per-evaluation allocation counts and pool bytes.

### Phase 3: terse emission

- One frequency-ordered renamer for globals, locals and runtime unit
  bindings, using Closure's same-length-batch trick for globals.
- Mangle compiler-owned ABI property names. Record field names stay as
  written by default because host code may read them; `^:export` and an
  option control the rest.
- Strip whitespace and comments from runtime units at build time with a
  small Rust lexer; no Node dependency. Decide later whether local renaming
  inside runtime units is worth its own pass or whether units are simply
  written tersely.
- String table for repeated long strings when it pays after gzip.

### Phase 4: deferred

ES module output, chunk splitting with cross-chunk motion, and the npm
seam as designed separately. None are prerequisites for the above.

## Advanced-mode contract

These are the deliberate semantic differences, stated up front the way
upstream states that advanced mode is unsafe for certain patterns.

- Dynamic `resolve`, `ns-publics`, `all-ns` and `find-var` see only
  retained definitions unless the program statically reaches reflection or
  names the definitions in `exports`.
- Collection `toString` exists only when printing is reachable.
- Equal constant literals may be `identical?` because they share one pooled
  value.
- `set!` on a core operation forces indirection for that operation only.
- `js*` is opaque; raw text referencing compiler identifiers is already
  unsupported by the hygiene pass.
- Everything else must produce results identical to `optimize: false`.

## Verification

- Differential execution of the full corpus at every level.
- Ratchet tests with ceilings per fixture and level.
- Retention report checked in under `benchmarks/results/` so size
  regressions name their cause.
- Startup and steady-state benchmarks in Node and QuickJS.
- The manifest validator in the test suite, so the runtime cannot regain
  hidden edges.

## Open questions

- Whether record field names should be mangled under an opt-in flag, given
  keyword lookup can go through a field table.
- Whether the sliced runtime should be emitted as top-level `const`s in the
  output closure or as a nested factory, considering QuickJS and V8 parse
  costs for large closures.
- How deep type inference should go before diminishing returns; start with
  booleans, numbers, strings and record types, measure, then decide.
