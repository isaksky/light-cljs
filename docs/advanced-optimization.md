# Advanced optimization design

Status: design, October 2026. Phase 0's optimization levels, size ratchet,
differential test, runtime retention report and startup benchmark are
implemented (see "Phase 0 extension points"), and so are Phase 1's unit
manifest and advanced-mode runtime slicing (see "Phase 1 manifest extension points" and "Phase 1 slicing
extension points") and Phase 2's core operation table and type inference (see
"Phase 2 core operation table extension points" and "Phase 2 type inference
extension points"); the rest is not. The work items are
tracked as Pellets in the `advanced-optimization` group; this document is the
shared context those pellets cite.

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

| Program | Upstream ceiling | Upstream measured, `:advanced` / lite | light-cljs today | Target |
|---|---:|---:|---:|---:|
| `(js/console.log (+ 1 2))` | 10,240 | 2,584 / 2,135 | 350,927 | < 1,000 |
| `(println "hi")` | 10,240 | 88,554 / 37,284 | 350,814 | < 5,000 |
| `[1 2 3]` printed | 92,160 (lite mode: 16,384) | 88,582 / 37,300 | 350,839 | < 15,000 |
| `{}` printed | 92,160 (lite mode: 32,768) | 88,552 / 56,119 | 350,829 | < 20,000 |
| map/filter/partition pipeline | 34,000 (lite mode) | 105,760 / 32,508 | 351,158 | < 25,000 |

Every program that uses one collection, keyword or core function today
embeds the full 350,414-byte runtime (84 KB gzipped), so the current numbers
are all the same number. The rows are the first five fixtures of
`tests/support/ratchet_fixtures.rs`, measured at `prepare`: the printing rows call
`(enable-console-print!)` and then `println`, and the pipeline row logs
upstream's `trivial/core6.cljs` expression with `js/console.log`.

The measured column compiles these same programs with the pinned clone at
`:optimizations :advanced`, and with `:lite-mode` and `:elide-to-string`
added (`cargo bench --bench compiler -- --levels --upstream
research/clojurescript`; see "Startup and steady state by optimization level"
in `benchmarks/compiler.md`). The ceilings are for slightly different
programs: the 10,240-byte one is upstream's `(. js/console (log "Hello!"))`,
so `(println "hi")`, which brings in the printer, measures far above it; and
the vector and map ceilings log an empty collection with `console.log`.

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
  entries or core aliases. Any retained `js*` disables both. (Since Phase 2
  steps 6 and 7, `Advanced` also removes pure-call initializers and treats
  `js*` as an opaque leaf.)
- `src/emitter.rs:105-134` includes the whole runtime when any used
  expression is a keyword, symbol, collection, core reference, reader
  literal, `with-meta`, var, protocol or type form, a `case` with keyword or
  non-strict tests, a message-bearing `assert`, a rest-arity fn, or when any
  namespace entry exists. Every `--entry` compile therefore embeds it.
  (Since step 2 of Phase 1 this is `reads_runtime`, and holds below
  `Advanced` only.)
- Calls emit `(callee).call(null, ...)` including core operations; every
  `def` initializer is wrapped in an IIFE; every `if` test is wrapped in
  `truth()` unless it is a `BooleanHint`; keywords allocate per evaluation;
  properties are emitted as bracketed string literals. (Since step 7 of
  Phase 1 the runtime interns literal keywords at every level, and `Advanced`
  declares keyword and symbol literals once in a constant table. Since step 3
  of Phase 2, `Advanced` uses `truth()` only for tests that may be numbers,
  strings or host values, and writes arithmetic, comparisons and typed `=`,
  `count`, `nth` and record lookups without calling the runtime.)
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
  unreachable from value construction. (Since step 3 of Phase 1 the hub only
  lists runtime bindings, and only the shared runtime keeps it. Since step 4
  every load-time effect is a unit attached to the constructor or operation
  that needs it. Since step 5 the compile-time machinery is marked in the
  manifest, and an application keeps it only when it reaches reflection.)
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
  attributes bytes after the fact. Implemented for runtime units (see "Phase
  1 slicing extension points"); retained globals are not listed yet.
- Startup benchmark: parse-plus-execute time in Node and QuickJS for the
  fixture set, recorded under `benchmarks/`, with steady state for a
  sequence pipeline and a record/protocol workload and upstream's
  `:advanced` byte size beside ours.

#### Phase 0 extension points

The first three items and the startup benchmark are implemented, and the
retention report covers runtime units. Later phases plug in here:

- **Level.** `OptimizationLevel` in `src/application.rs` is ordered
  `None < Prepare < Advanced`. `level.prepares()` gates today's conservative
  passes; gate release-only work on `level.advanced()`. Never change what
  `None` emits: it is the oracle.
- **Passes.** `prepare_application` marks where `Advanced` passes run, after
  conservative preparation. Emission that depends on the level (runtime
  slicing, the constant table, terse emission) reads the level that
  `PreparedApplication` passes to `emit_application` in `src/emitter.rs`;
  today runtime slicing and direct runtime reads do. `Advanced` preparation
  also removes more definitions (pure initializers) and ignores the `js*`
  escape (Phase 2 steps 6 and 7), so its output is `Prepare` output minus
  those definitions, reading `rt$<binding>` instead of `rt.<export>`, with a
  smaller runtime factory.
- **CLI.** `--optimize` is parsed in `src/main.rs` for single-file `--compile`
  and `--compile --entry`, with or without `--esm`. Namespace builds go through
  `compiler::analyze_namespace_entry` and `prepare_application`, so the CLI
  holds the `PreparedApplication`; level-dependent reports go next to it, as
  `--size-report` (the retention report, on stderr) does. `--eval` and
  `--repl` have no level.
- **Ratchet.** `tests/support/ratchet_fixtures.rs` holds `FIXTURES`: the five
  Targets rows, `(+ 1 2)`, `(fn [& xs] xs)`, a keyword `case`, a
  `defrecord` with a protocol call, unused pure definitions and retained
  `js*`, each with `ceilings: [none, prepare, advanced]` in bytes of
  `emit_js()` output and `removed: [none, prepare, advanced]` definitions;
  `tests/compiler/size_ratchet.rs` checks them. When output shrinks, lower the ceiling to the new size
  rounded up to the next 100 bytes (next 1,000 from 10,000 up) and record
  the sizes in `benchmarks/compiler.md`. Add rows for new fixtures there.
  `--no-capture` prints the size table.
- **Differential.** `size_ratchet.rs` also runs every fixture in Node at
  every level and compares against `None` and a pinned expected output.
  Observations must not depend on compiler-owned ABI names (functions print
  as `#function`). `both` and the per-level loops in
  `tests/compiler/optimization.rs` run the preparation regressions at every
  level, and `npm_packages::bundler_modules_run_at_every_optimization_level`
  runs a bundler module built with `--esm --optimize LEVEL` for each level.
- **Benchmark.** `cargo bench --locked --bench compiler -- --levels --output
  DIR` (`benches/support/levels.rs`) times every ratchet fixture at every
  level: parse plus first execution in fresh Node processes and fresh
  QuickJS `Evaluator`s, and steady-state calls of the `STEADY` workloads
  (the seq-pipeline and record-protocol fixtures as functions of `n`).
  `--upstream research/clojurescript` adds upstream's `:advanced` and
  lite-mode byte sizes, which needs Java and the Clojure CLI. Rounds
  interleave the levels, so compare levels within one run; to quote a
  pellet's before/after, run it on both commits and compare medians and
  minimums with the recorded load in mind. A fixture added to the ratchet is
  measured automatically; the method and baseline are in
  `benchmarks/compiler.md` ("Startup and steady state by optimization
  level").
- **Generated programs.** `tests/compiler/generated_optimization.rs` checks
  the levels on seeded programs from `namespace_program` in
  `tests/support/generated.rs`: 2-6 namespaces in a `:require` DAG with
  definitions, a protocol with records, types and `extend-type number`, and
  escapes (`var`, `with-redefs`, `set!`, `defonce`, `resolve`, functions passed
  as values), plus garbage that is dead by construction and tagged with why
  (unreferenced namespace, unused literal or pure def, unused defn, unused
  keyword, uninstantiated type, uncalled protocol method). Each fixed seed is
  one test. It checks five properties: every level records the same trace,
  value or error category and stdout, and adding the garbage changes none of
  them; adding one tag's garbage at a time, each step's advanced output is
  compared with the previous step's and the tag classified as `removed` (no
  sentinel string left and at most `SLACK` more bytes once compiler numbering
  is ignored), `residue` (code gone, bytes left) or `survives`; garbage that
  the escape variant wires to a reachable `var`, `resolve`, `with-redefs` or
  protocol call on a constructed instance survives at every level;
  Prepare's `removed_definitions` and `specialized_calls` never fall as
  garbage is added; and, since 218, the program behaves identically at every
  level when its collection literals are rendered with their first item
  behind `identity`, which no level pools (`Literals::Built`). The literals
  come from a stream of their own: repeated and nested constants, reader
  metadata, partially constant vectors holding a local, read through
  `count`, `pr-str`, `hash`, `=`, transients and `with-meta`, each read also
  checking that the literal itself is unchanged. Observations are host values only (numbers
  and `pr-str` strings), so the harness does not depend on the runtime anchor
  and keeps working when advanced output slices the runtime.

  `EXPECTED` in that file is the expected-survivor list: one class per tag
  with the pellet that should improve it. Since 206, literal, pure-call and
  keyword defs, defns and unreferenced namespaces are `removed`: a program
  without reflection registers no namespace and a removed definition leaves no
  `null;` statement. Uninstantiated types and uncalled methods survive until
  211. The generator writes garbage and escape text on the line before it,
  without line breaks of its own, so adding garbage moves no program form and
  leaves source metadata (`:line`, `:column`) unchanged. Every
  seed checks every tag exactly, so a pellet that removes more garbage fails
  the test until it moves that row toward `removed` in the same commit. If a
  tag's items stop sharing one class, split the tag in the generator rather
  than loosening the row. Pellets that add constructs (208 and 218 to 221 for
  constant literals) extend the generator, keeping the program text without
  garbage independent of the garbage stream. Replay a failure with
  `LIGHT_CLJS_GENERATED_SEED=<seed>`; `LIGHT_CLJS_GENERATED_COUNT=<n>` runs more
  seeds locally.

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
   Implemented; see "Phase 1 slicing extension points" below.
3. **Dismantle the `core` hub.** Emit direct references to unit bindings
   instead of `rt.core.X`. `set!` on a core operation is visible
   program-wide; only mutated operations fall back to an indirection cell.
   `binding` on `*print-length*` and similar dynamic vars keeps a cell.
   Implemented; see "Phase 1 hub extension points" below.
4. **Make top-level effects demand-driven.** Prototype patching loops,
   `installMethods`, `variadicCore` registrations, `EMPTY` constants and
   factory assignments move into install functions required by the
   constructors or operations that need them.
   Implemented; see "Phase 1 effects extension points" below.
5. **Separate compile-time machinery.** Form bridge, declaration demand,
   `inspect`, macro reader context, `compilerInput*`, and the namespace
   registry become units reached only by compile-time emission modes or by
   user code that calls reflection (`ns-publics`, `resolve`, `*ns*`,
   `read-string` with auto-resolve). `registerReaderNamespace` and
   `currentNs` emission become conditional on that reachability.
   Implemented; see "Phase 1 compile-time extension points" below.
6. **Break the toString edge.** The print unit installs collection
   `toString` on load. A program that never reaches `str`, `pr-str`,
   `print*` or `with-out-str` gets collections without `toString`. This is
   the advanced-mode counterpart of upstream `:elide-to-string` and must be
   documented as such.
   Implemented; see "Phase 1 print extension points" below.
7. **Constant table.** Hoist keyword and symbol literals into one interned
   table at the top of the output. `case` on keywords switches on identity.
   Collection literals with only constant parts may be hoisted the same way
   when their construction is proven inert.
   Implemented for keywords and symbols, the first slice of the constant
   pool; see "Phase 1 constant table extension points" below. Keyword `case`
   switches on the keyword's spelling rather than its identity, because
   keywords built at run time match by spelling. Collections are "Constant
   collection literals".

#### Phase 1 manifest extension points

Step 1 is implemented (light-cljs-202). Later steps build on it:

- **Format.** `//@unit <name>[ compile-time][ mutator][ effect[: <bindings>]][ requires: <bindings>]`
  headers in `src/js_runtime.js`, with `//@ ` continuation lines and a final
  `//@exports` section; the module docs of `src/runtime_manifest.rs` give the
  rules. A unit that declares bindings is named after its first one (today
  each is one top-level declaration statement). `requires:` lists exactly the
  other units' bindings the body references.
- **Effects.** A unit with top-level statements other than declarations is
  marked `effect`. Bare `effect` mutates its own bindings; `effect: a, b`
  belongs to other units' bindings, is named `<target>.<what>` by convention
  (`ListValue.members`, `core.seal`), and is kept whenever a unit declaring a
  target is kept. Since step 4, calling a `mutator` while loading is an effect
  too, and `install: a, b` marks effects needed only where a target's value
  escapes (`add.variadic`); see "Phase 1 effects extension points".
- **API.** `RuntimeManifest::embedded()` parses the compiled-in runtime once.
  `unit_of(binding)` maps a runtime name to its `UnitId` (dense, in file
  order); `dependencies`, `attached_effects` and `reachable(roots)`, which
  returns a `Vec<bool>` over units, give the slicing closure
  (`attached_installs` and `reached_by_uses` since step 4, and
  `core_dispatcher(key)` for the dispatcher table). A sliced runtime
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
  and review the rewritten `requires:` lists. Step 5 keeps the compile-time
  machinery exported, because the shared path needs it, and marks it
  `compile-time` instead; a pellet that stops exporting some of it lists its
  units in `UNREACHABLE_FROM_EXPORTS` there. Per-unit sizes and reach are in
  `benchmarks/compiler.md` (the `print_runtime_unit_sizes` ignored test
  prints them).

#### Phase 1 slicing extension points

Step 2 is implemented (light-cljs-203) in `src/runtime_slice.rs`:

- **Roots.** The emitter writes every runtime read through `Emitter::rt`,
  which records a `RuntimeName` (`rt.<export>` or `rt.<export>.<member>`,
  such as `rt.keyword` or `rt.core.add`) with the innermost expression being
  emitted. Core operations, the hardcoded constructors and helpers, and the
  namespace registration (`registerReaderNamespace`, `currentNs = theNs(...)`)
  all go through it, so emission reports its roots as a set. Debug builds
  check that no `rt.` read in the output (outside raw `js*` text) escaped the
  record, so every test that emits code checks the roots. New emission that
  reads the runtime must use the helper; at `Advanced` it resolves and
  records direct reads (see "Phase 1 hub extension points").
- **Slice.** A root keeps the unit declaring its binding. At `Advanced`,
  application output (`emit_js` and `emit_esm`) embeds
  `RuntimeManifest::sliced_bindings_text(reachable(roots), bindings)` (since
  step 4 the closure is `reached_by_uses`, with each root's `NameUse`): the
  prologue, the kept unit bodies in runtime order and `return [binding, ...];`
  for the bindings the program reads (step 2 returned an export object;
  `sliced_text` still builds that form). The anchor text, the shared path's
  `const rt =` seam and the exclusion of the runtime from `hygienic_js` are
  unchanged. Below
  `Advanced`, and for the shared runtime of the REPL, macro host and
  deferred units, the whole runtime is embedded as before, so `None` and
  `Prepare` output is byte-identical. `Advanced` embeds a runtime only when
  some root exists.
- **Retention report.** `PreparedApplication::retention()` returns a
  `RetentionReport`: the roots in first-demand order (the program body in
  emission order, then the namespace registration), each embedded unit with
  its body bytes, and the index of the first root whose closure reaches it
  (`RuntimeManifest::reached_by`), so a later root lists only the units it
  adds. Its `Display` is the table `--compile --size-report` prints to
  stderr: a summary line, a root table (units and bytes each root adds,
  demanding expression as source index and byte span) and a unit table.
  Below `Advanced` it lists the whole runtime and attributes what the roots
  reach.
- **Measured.** A program that reads the runtime keeps at least the 665 of
  691 units (334,504 of 349,584 unit bytes) that `core` reaches, because it
  reads `rt.core.*`, if only through the namespace registration. The 26 units
  `core` does not reach are the declaration demand and patch machinery,
  `setMacroexpandServices`, `compilerExceptionMessage`, the protocol, type,
  record and reify constructors, and four literal constructors; a program
  that uses none of them is about 15.7 KB (4.5%) smaller, and the
  `defrecord` fixture keeps 672 units. These were step 3's before numbers;
  see `benchmarks/compiler.md`.

#### Phase 1 hub extension points

Step 3 is implemented (light-cljs-204). The `core` map still exists for the
shared runtime, but no application slice keeps it:

- **Core table.** Every core operation is a runtime binding or a member of
  one. The numeric and predicate objects and the inline members of the old
  `core` literal became one unit per operation (named after the operation, or
  `<name>Protocol` / `<name>Core` where the name was taken, keeping the
  public function name, which printing shows). The reader is the `reader`
  unit, types were members of `builtinTypes` (since step 4 each type is the
  binding of its constructor, `PersistentVector: PersistentVectorValue`), and
  positional factories are units. The `core` unit is `Object.assign(coreCells, {name, name: binding,
  name: binding.member, ...})` and must keep that flat shape:
  `RuntimeManifest::parse` reads it, with the keys of the `coreCells`
  literal, into `RuntimeManifest::core_reference(runtime name) ->
  CoreReference::{Binding, Member, Cell}`. A new `CoreFnId` needs a hub entry;
  `runtime_references::every_core_operation_resolves_through_the_core_table`
  checks every `runtime_name`.
- **Cells.** `coreCells` holds the dynamic Vars (`*print-length*`,
  `*print-fn*`, `*ns*`, ...), the host hooks and the operations the runtime
  itself reads through a cell so that `set!` and `with-redefs` reach it
  (`random-sample` reads `rand`, `newline` reads `flush`). The hub extends
  this object, so `rt.core.printFn = f` on the shared path and the runtime's
  `coreCells.printFn` reads share one property. Inside the runtime every other
  former `core.X` read is a direct binding. Make a new runtime read through a
  cell only when redefinition must reach it, and add the key to `coreCells`.
- **Emission.** At `Advanced`, `Emitter::rt` resolves `rt.core.<name>`
  through the table and records the direct root (`RuntimeName::direct`,
  spelled `rt$add`, `rt$coreCells.printFn`, `rt$reader.readString`). The
  slice returns `[add, ...]` and the output destructures it into `rt$<binding>`
  constants, so the names are compiler-owned: `hygienic_js` renames `rt$` and
  `core$` tokens, and the debug scan checks each `rt$`/`core$` read against
  the record. A call whose callee is a core operation held by a binding of
  its own, with a function value in `src/core_operations.tsv`, is a plain
  call, `rt$add(1, 2)`; members, cells and replaced operations keep
  `.call(null, ...)` for the call-shape step (Phase 2 step 5).
- **Replacement.** The program-wide scan that pure-initializer removal
  trusts (`Inert` in `src/application.rs`, light-cljs-213) also decides
  indirection: a core operation the program may replace, because it is a
  `set!`, `binding` or `with-redefs` target (directly or through a property)
  or a `js*` hole that raw text could assign, is listed by runtime name in
  `PreparedApplication::replaced_core()`. Each gets an output-level cell,
  `let core$inc = rt$inc;`, that every read and write of the operation
  uses, so a hole emits the assignable cell and the assignment reaches every
  read. Cells of `coreCells` need none.
- **Measured.** `(js/console.log (+ 1 2))` drops from 335,242 to 152,535 bytes
  at `Advanced` (316 of 906 units). It does not reach the few kilobytes this
  step was hoped to reach, for two reasons outside the hub: the variadic
  arity effect `add.variadic` requires `foldSeq` and so `seq`, and the
  namespace registration needs `symbol`. Every value constructor goes through
  `builtinValue` to `builtinTypes`, whose attached effects patch every type's
  prototype, and that reaches the collection, hashing and printing machinery
  (`symbol` alone keeps 299 units, 147 KB). Steps 4 (demand-driven effects)
  and 5 (conditional namespace registration) remove those edges. Per-fixture
  sizes are in `benchmarks/compiler.md`.

#### Phase 1 effects extension points

Step 4 is implemented (light-cljs-205). The runtime runs no effect while
loading that the manifest does not attach to the units needing it:

- **Builtin types.** The `builtinTypes` registry, `publicConstructors` and
  `builtinValue` are gone. Each builtin type is the binding of its
  constructor (`ListValue`, `Cons`, `treeMapConstructor`; anonymous
  constructors are `NodeSeqType`, `MapEntryType`, `UUIDType` and the four
  `Transient*Type`s), and values are `Object.create(ListValue.prototype)`. A
  type's effects are units attached to that binding: `<binding>.type` sets the
  prototype chain right after its base prototype (`builtinType(ListValue,
  sequencePrototype)`), before any value of the type exists; `.members` adds
  the public fields (`publicFields`), methods and iterators where the old
  `builtinTypes.prototypes` loop did; `.statics` sets `EMPTY`, `fromArray`,
  `fromArrays` and `HASHMAP_THRESHOLD` where `builtinTypes.statics` did. A
  program keeps the members of the types it can construct, and the empty
  singletons of tier 1 below are `PersistentVectorValue.EMPTY` and so on,
  kept with their type. The hub lists the constructors directly, so core
  operation table rows name them (`PersistentVector` is
  `PersistentVectorValue`).
- **Installs.** `install: <binding>` marks an effect that only adds
  properties to a value for code that receives it: the variadic arity ABI of
  the 24 `variadicCore` operations, the comparisons (`comparisonVariadic`) and
  the bit operations. `RuntimeManifest::reached_by_uses` keeps an install
  once its target escapes: it is a `RootUse::Value` root, or a kept unit
  requires it (conservatively, since runtime code may pass it on). The emitter
  records a plain call `rt$add(1, 2)` as a `NameUse::Call` root, so a program
  that only calls `+` keeps the 234-byte `add` unit and not `foldSeq` and the
  sequence machinery; `(apply + xs)` or any other value read keeps the ABI.
  The retention report prints call roots as `rt$add(...)`.
- **Mutators.** `mutator` marks a function whose calls mutate their arguments
  or a registry (`builtinType`, `installMethods`, `variadicCore`, `coreMethod`,
  `createNs`, `extendType`, the declaration patch helpers). The validator
  computes each unit's load-time calls (outside every function body) and
  rejects a unit that calls a mutator while loading without being an `effect`
  or `install` unit, or one marked `effect` with neither statements nor such
  calls. Mark a new function `mutator` when calling it has a lasting effect;
  the check is one level deep, so a load-time call of a function that calls a
  mutator needs the callee marked too.
- **Dispatchers.** Each `coreMethod("<key>", ...)` declarator registers a core
  protocol dispatcher in `coreDispatchers`, where `extendType` looks it up by
  key to patch native extensions and `builtin` reads them. The dispatcher
  units are bare `effect` units, and the manifest lists every registration
  (`RuntimeManifest::core_dispatcher(key)`; any other use of `coreMethod` is
  a parse error). At `Advanced` an `extend-type` of a core protocol method
  records the method's dispatcher as a `NameUse::Keep` root: the slice keeps
  it although the output does not read it, so dispatch through a dispatcher
  no kept code calls still finds the patch. The report prints such roots in
  parentheses.
- **Left for later.** `namespaces.cljsUser` and `namespaces.cljsCore` stay
  effects of the namespace registry, which only the namespace registration
  and reflection reach (step 5), and so does `compilerForms`. (Since step 5
  an application slice keeps neither without reflection.) Generic
  operations (`seq`, `withMeta`, `conj`, `=`, `hash`, the printer behind
  `toString`) reference every collection constructor, so any program that
  builds a value keeps them and their members, about 326 units (132 KB); step
  6 and rapid type analysis (Phase 2) shrink that core. (Since step 6 the
  printer is not part of it: about 300 units, 119 KB.)
- **Measured.** `(js/console.log (+ 1 2))` drops from 152,535 to 136,918
  bytes at `Advanced`. Its `add` root now keeps only `add`; the rest is the
  namespace registration (`registerReaderNamespace` reaches 338 units through
  `symbol`), which step 5 makes conditional. Every other fixture drops by
  6-15 KB; `[1 2 3]` and `{}` alone drop from 151,912 and 151,896 to 136,565
  and 136,549 bytes. The shared runtime grows by 1,800 bytes. Per-fixture
  sizes are in `benchmarks/compiler.md`.

#### Phase 1 compile-time extension points

Step 5 is implemented (light-cljs-206). `Advanced` application output keeps
no compiler machinery unless the program reaches reflection:

- **Compile-time units.** The header flag `compile-time`
  (`Unit::compile_time`) marks the machinery of the compiler, the macro host
  and the REPL: the compiler form bridge (`compilerFormBridge`,
  `compilerForms`), declaration demand and transactions
  (`setDeclarationDemand`, `declarationReceiver`, `declarationTransaction` and
  the patch helpers), the namespace registry (`namespaces` with its
  `cljs.user` and `cljs.core` effects, `createNs`, `registerReaderNamespace`,
  `findNs`, `theNs`, `allNs`, `readerCoreNames`), the macro reader context
  (`macroReaderFunction`, `withMacroReaderContext`, `compilerMacroVar`),
  `inspect`, `setMacroexpandServices`, `compilerExceptionMessage` and the
  `eval` stub (`evalCore`). The shared runtime still exports them, and the
  shared path is unchanged.
- **Reflection.** The `compile_time_units` suite in `tests/compiler/` pins
  the core operations whose units reach a compile-time unit: `*ns*` (the
  `currentNs` cell, `runtime_slice::REGISTRY_CELLS`), `find-ns`, `the-ns`,
  `all-ns`, `create-ns`, `eval`, `compiler-macro-var*`, and every operation
  of the reader (`read-string`, `read`, `read-line`, and the `cljs.reader`
  and `clojure.edn` operations), because the reader is one unit and requires
  `readerCoreNames` and `inspect`. The emitter's other roots reach none,
  except the namespace registration, macro functions defined in the program
  (`macroReaderFunction`), quoted `#js` literals (`compilerForms`) and the
  `declarationReceiver` of compile-time emission. `resolve`, `ns-publics`,
  `ns-interns`, `ns-imports` and `ns-refers` expand to Vars at compile time
  and are not reflection. A core operation or emitter root that starts
  reaching a compile-time unit fails that suite until it is listed there.
- **Registration.** `RuntimeRoots::reach_reflection` decides from the roots
  of the program body whether the slice keeps a compile-time unit or the
  program reads, sets or binds a registry cell. Only then does `Advanced`
  output register its namespaces (`registerReaderNamespace`, `currentNs =
  theNs(...)`) and switch `*ns*` at each namespace entry. `emit_js_mode`
  emits the body without the registration first, and emits it again with
  the registration when those roots reach reflection, so only a reflective
  program pays a second emission. The registration's `bindings` then list
  only retained definitions (see "Advanced-mode contract"). Below `Advanced`,
  and on the shared path, every output registers as before. At `Advanced` a
  discarded `nil`, such as a definition that preparation removed, emits no
  `null;` statement.
- **Hooks.** Application paths call `demandProtocol`, `demandMethod` (from
  `directMethod`, which no longer slices the protocol key on every call) and
  `demandMulti`, which are empty functions until `setDeclarationDemand`
  replaces them. They also call `patchDeclaration` and `setDeclaration`,
  which define a property or set a store entry until an open
  `declarationTransaction` replaces them with journaling versions; it restores
  them when it closes. `installMethods`, `multiState` and `addMethod` read
  `declarationJournal`, the open transaction's undo journal, which stays null
  in an application. The reader builds `#js` literals through
  `readerJsLiterals`, which the `compilerForms` unit (a bare `effect`) points
  at the bridge's builders that mark literals for the macro host, so a reader
  program keeps no bridge. A new hook follows the same pattern: an
  application unit with the plain behavior, replaced by a compile-time unit.
- **Left for later.** The reader is one unit, so EDN reading counts as
  reflection and keeps `readerCoreNames` (9.4 KB), `inspect` and the
  `compilerInput*` window; splitting the EDN reader from the code reader would
  separate them. The registration needs `symbol` and `Namespace`, so a
  reflective program keeps the generic core (about 330 units) until step 6
  and rapid type analysis shrink it.
- **Measured.** `(js/console.log (+ 1 2))` drops from 137,360 to 713 bytes at
  `Advanced`: the 234-byte `add` unit, the runtime prologue and the program.
  `(+ 1 2)` is 553 bytes and the `raw-js` fixture 1,404. Every fixture that
  builds a value drops by 4.8-5.3 KB, the registry and the registration
  itself. The shared runtime grows by 583 bytes. Per-fixture sizes are in
  `benchmarks/compiler.md`.

#### Phase 1 print extension points

Step 6 is implemented (light-cljs-207). An `Advanced` program keeps the
printer only when it can print:

- **The printer and its install.** The printer is the `printWriterImpl` unit
  and what it requires. Its install, the effect unit
  `printWriterImpl.install`, travels with it: it sets
  `ExceptionInfo.prototype.toString` and fills in `printHooks`, the printer
  as seen from code that must not keep it. The frozen collection base
  prototypes (`sequencePrototype`, `vectorPrototype`, `mapPrototype`,
  `setPrototype`, `transientPrototype`) define `toString` as
  `printHooks.collection(this)`, which is `print(value, true)` once the
  printer is installed and `Object.prototype.toString`'s `[object Object]`
  before. The install cannot add `toString` to the prototypes themselves:
  they are frozen, and naming them would keep every collection type in a
  program that prints.
- **What reaches it.** The `print_install` suite in `tests/compiler/` pins
  the core operations whose units reach the printer: the print functions,
  `str` (a collection's text is its printed form), `-pr-writer`, `clj->js`
  (map keys that are not scalars print), `gensym` and `munge` (they `str`
  their argument), tagged literals (`TaggedLiteral` prints through `pr-str`)
  and the reflection that keeps the reader or the namespace registry.
  `with-out-str` reaches it only through what its body prints, and
  `*print-fn*` is a cell. A core operation that starts reaching the printer
  fails that suite until it is listed there.
- **Error messages.** Error paths format values without the printer.
  `displayStr` is `str` of one value with a collection formatted through its
  `toString`; the runtime's `No value supplied for key`, `Duplicate key`,
  `Missing required key`, `Cannot compare` and `Cannot create iterator from`
  messages use it, and so do the emitter's `case` no-match (`condp` expands
  to it) and `assert` messages through the `displayStr` export. Multimethod
  and hierarchy errors, which print values non-readably, call
  `printHooks.text`: before the install it formats nil, booleans, numbers,
  strings, keywords and symbols as `print` does and any other value with
  `String`. Where the printer is kept, every message is the one `none`
  gives; see "Advanced-mode contract".
- **Writers.** The `-pr-writer` methods of records and eductions call the
  printer through `printHooks.writer` and `printHooks.sequentialWriter`,
  which only the install sets: only the printer and the `-pr-writer`
  dispatcher, which requires it, call such a method. A new runtime
  `IPrintWithWriter` implementation, `toString` or message that formats a
  value follows the same pattern instead of requiring the printer.
- **Left for later.** `TaggedLiteral` keeps `pr-str` for its `toString`, so
  `tagged-literal` and `tagged-literal?` keep the printer; only the reader,
  which keeps it anyway, builds tagged literals implicitly. A program that
  builds a value keeps about 300 units (119 KB): `seq`, `count`, `=`,
  `hash`, `meta`, `conj` and `assoc` dispatch over every builtin collection,
  so each references every collection constructor, whose prototype and
  members reference the generic operations back. That is one strongly
  connected component of the unit graph, which rapid type analysis
  (Phase 2) has to cut.
- **Measured.** `[1 2 3]` drops from 132,231 to 119,567 bytes at `Advanced`
  (327 to 299 units); `(pr-str [1 2 3])` grows from 132,919 to 133,434. Every
  fixture that builds a value and does not print drops by 10.1-12.7 KB, the
  record and defmulti programs included. A program that prints grows by
  about 500 bytes: `printHooks`, `printNumber`, `displayStr` and the install,
  less `str`, which the error paths no longer keep. The shared runtime grows
  by 1,084 bytes. Per-fixture sizes are in `benchmarks/compiler.md`.

#### Phase 1 constant table extension points

Step 7 is implemented for keyword and quoted symbol literals
(light-cljs-208). It is the first slice of the constant pool: collection
literals (Pellets 218-221) extend the same table.

- **Table.** At `Advanced`, `ConstantTable` in `src/emitter.rs` declares each
  distinct constant the emitted code reads once, `const k<n> =
  <initializer>;`, and each site reads `k<n>`. The declarations follow the
  runtime slice and the core operation cells and precede the namespace
  registration, the `let g<n>;` declarations and the program, in `emit_js`
  and `emit_esm` alike. Entries are numbered and declared in first-use order.
  Only emitted sites add entries, so the table needs no reachability of its
  own: a constant that no retained code reads never appears. `None` and
  `Prepare` output has no table.
- **API.** `Emitter::constant_entry(key, lazy, initializer)` returns the
  entry number of a `ConstantKey`, declaring the entry on first use, and
  `Emitter::constant_reference(n)` writes a site's read of it. `initializer`
  writes the value expression into the emitter's output, which
  `constant_entry` moves into the table line, so runtime reads go through
  `Emitter::rt` as usual. The first site is the demand site that the
  retention report names. Keys compare emitted structure:
  `ConstantKey::Keyword` and `ConstantKey::Symbol` hold the UTF-16 namespace
  and name, and `ConstantKey::Composite` (218) the runtime constructor and
  the parts (see "Constant collection literals"). `Emitter::named_entry` also
  caches each keyword and symbol entry number by `NamedLiteralId` in a dense
  array, so repeated sites skip the hash. An entry is numbered after its
  initializer has run, so an initializer that reads other constants (a
  vector of keywords) declares them first. Initializers run once, in table
  order, before any definition. An entry may read only the runtime and
  earlier entries, never a program global. A function that only references
  globals in its body is fine, because the body runs later; this covers the
  arity methods that 212 may hoist. With `lazy` set, a new entry is a lazy
  cell instead (218): `let c<n>; const k<n> = () => c<n> ??= <initializer>;`,
  read as `k<n>()`, which builds the value on the first read.
- **Names.** `k<n>` and `c<n>` names are compiler-owned. `hygienic_js`
  renames `k<digits>` and `c<digits>` tokens along with its other numbered
  prefixes. `optimization::raw_js_names_resolve_only_to_holes_and_host_globals`
  checks this, because its probe program reads keywords and fills a lazy
  cell before its other probes run. A pellet that introduces another prefix
  adds it there. Reusing `k<n>` for every entry needs no change.
- **Keyword interning.** The runtime's `keyword(namespace, name)` interns.
  Every literal site calls it, per evaluation below `Advanced` and once from
  the table at `Advanced`. It keeps one keyword per namespace and name in a
  `Map` (`internedKeywords`) for the life of the runtime. That holds at every
  level, and across REPL inputs and deferred units, which share the session
  runtime. The runtime's own constant keywords (`:no-test`, record field
  keys, option names) go through it too, so they are identical to the
  program's literals. Keywords built from data are fresh objects, as
  upstream:
  - the `keyword` function (`freshKeyword`, or `makeKeyword` for its
    one-argument forms) and symbol conversion;
  - the reader (`read-string`, EDN) and keywordized `js->clj` keys;
  - keys made for namespaced-map and `#js` printing;
  - the positional constructor (`Keyword.`, `->Keyword`), which returns a
    fresh object, like `new`.

  The table therefore grows only with literals, never with data. Interning
  data keywords as well was measured and rejected. Weak references give no
  reuse in QuickJS, which frees a keyword when its last reference drops. Each
  dead keyword then also leaves a `FinalizationRegistry` job that the
  sessions never run. In a QuickJS micro-benchmark, building transient
  keywords that way took about twice as long as allocating them. A strong
  table would grow with the data.
- **Equality and hashing.** Keyword and symbol equality already checks
  identity first (`equalPair`, `keywordIdentical`, `symbolIdentical`), and
  hashes are cached per object (`constructorHashes`). An interned literal
  therefore compares and hashes by identity after its first use. Keyword `=`
  compares spellings (upstream's `fqn`), which is the structural fallback
  for every keyword built at run time.
- **What later passes may rely on (210).** At every level, two literal
  keywords whose namespace and name contain no `/` are `=` exactly when they
  are `identical?`. A keyword built at run time is never identical to a
  literal. Some built keywords spell like a literal without having its parts:
  a `/` inside a part (`(keyword nil "a/b")` is `=` to `:a/b`), or the
  positional constructor, which takes any spelling. Such a keyword can equal
  a literal while hashing differently. Upstream fixtures pin these cases. So
  `===` decides `=` only between two literals, a comparison a pass can fold
  at compile time.

  The sound lowering of `(= x :k)` is `x === k<n> || rt$keywordFqn(x) ===
  "<spelling>"`. It applies when `x` is known to be a keyword, or when the
  literal comes first and no program extends `IEquiv` to `Keyword` (the
  condition from 209). The identity check succeeds for literals. The
  fallback is a `WeakMap` read and a string comparison, never the protocol
  dispatch of `equal`. Since light-cljs-210 `Advanced` emits this lowering,
  also when `x` can be no program type or no program type implements
  `IEquiv`, since `=` then runs only the runtime's implementations (see
  "Phase 2 type inference extension points").

  The `constant_table` suite in `tests/compiler/` pins every identity above
  at every level, across REPL inputs and against `read-string`.
- **`case`.** At `Advanced`, keyword `case` switches on
  `rt$keywordFqn(caseValue)`, which is the keyword's spelling, or null for a
  value that is not a keyword. The no-match message reads the same value.
  This replaces `isKeyword` and `str`, which reach the printer. `case` does
  not switch on the constants' identity, because keywords built at run time
  match by spelling.
- **Symbols.** Quoted symbols are hoisted but not interned. At `Advanced`
  equal symbol literals are one value, which is the contract's "equal
  constant literals may be `identical?`". Below `Advanced` each evaluation
  builds a new symbol. A symbol with metadata keeps its per-site `with-meta`
  around the shared symbol.
- **Measured.** The shared runtime grows by 590 bytes, and every `Advanced`
  slice that reaches `keyword` grows by 442; at the time that was every
  slice, through the namespace registration (since step 5, only slices that
  reach `keyword` otherwise). In small programs the table is about
  size-neutral. The steady-state workload `keyword-lookups` (keyword lookups
  in literal maps plus a keyword `case`) takes half as long in Node at every
  level. QuickJS gains 10% below `Advanced` and 14% at it. Startup does not
  change beyond noise. Sizes and timings are in `benchmarks/compiler.md`.

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

#### Phase 2 core operation table extension points

Step 2 is implemented (light-cljs-209). Later steps read it:

- **Table.** `src/core_operations.tsv` has one row per `CoreFnId`, grouped
  by kind: the variant, its Clojure name, the type of the core value, call
  signatures by arity (`[seqable number] [seqable number any]`, rest as one
  type after `&`), the result type within those signatures and otherwise,
  purity (`pure`, `allocates`, `throws`, `effect`), the code the arguments
  supply that it may run (`-`, `dispatch`, `invoke`) and its runtime units.
  `src/core_operations.rs` defines the semantics; its module docs are the
  contract, including the program-wide conditions a pass must check (no
  `set!` of the operation, no extension of core protocols to runtime or host
  types). `build.rs` derives `CoreFnId::ALL` and an exhaustive index from the
  first column, so a new variant does not compile without a row; an unknown
  row (`-`, `any`, `effect`, `invoke`) is safe.
- **Lattice.** `core_operations::Type` is a bit set over fifteen classes
  (`nil boolean number string keyword symbol fn seq vector map set list
  record object host`); union is the join. `Type::parse` and `Display` use
  the table syntax. Collection classes mean the runtime's own types; user
  types are `object` or `record`.
- **API.** `CoreFnId::operation()` returns the row. `Operation::call(&[Type])`
  gives the result type, purity and calls for argument types: the row's
  values when the arguments are within a signature, else the `otherwise`
  type with `effect`/`invoke`. Inference (step 3) reads `returns`; pure
  initializer removal (step 6) drops a call that is `pure` or `allocates`
  when no argument can run code (`-`, or `dispatch` over plain data, or
  `invoke` with pure function arguments); folding (step 8 and the constant
  folder) needs `pure`. `Operation::runtime_units(manifest)` maps the units
  column to `UnitId`s, the roots of a direct reference for slicing.
- **Maintenance.** The `core_operations` suite in `tests/compiler/` requires
  every named unit to exist and to be the unit that defines the operation's
  value in the runtime `core` object (read from the core table,
  `RuntimeManifest::core_reference`, since Phase 1 step 3), so moving an
  operation to another unit, install functions (step 4) or compile-time
  units (step 5) fail it until the table follows: run
  `LIGHT_CLJS_UPDATE_CORE_OPERATIONS=1 cargo test --test compiler core_operations::`
  and review the `units` diff. The same suite calls every row with
  signatures on sample arguments in QuickJS (functions that throw when
  called, lazy sequences and sorted collections that throw when their code
  runs) and fails on a result outside the annotated type, a `pure` or
  `allocates` call that throws, a fresh result from a `pure` call, or
  argument code run by a row that does not admit it. The table was seeded
  from upstream's `^boolean` functions and `bool-expr` macros and the `:tag`
  metadata in `src/core_var_metadata.tsv`. Every `^boolean` operation with a
  signature returns `boolean` here, and every `number`-tagged one a number
  (`alength` may also return `nil`; `unchecked-char` is the identity in this
  runtime). Only the protocol method dispatchers (`-equiv`,
  `-contains-key?`, `-count`, ...) stay unknown, since they return whatever
  an implementation returns.

#### Phase 2 type inference extension points

Step 3 is implemented (light-cljs-210) in `src/inference.rs`, at `Advanced`
only. `prepare_application` infers types over the prepared program, after
definition removal and call specialization, and emission reads them. `None`
and `Prepare` output does not change.

- **API.** `PreparedApplication::inference()` is the `Inference` of the
  program `PreparedApplication::program()` emits, built by
  `Inference::new(program, ProgramFacts::new(program, top))`.
  `value(expr)` is an `Inferred`: a `core_operations::Type` and a `Known`
  representation that refines one of its classes. The representations are
  `Function(fn expr)` for the functions a `fn` makes, `Instance(type global)`
  for instances of a program type defined once, `Vector` for flat persistent
  vectors (literals and `vector`, never map entries or subvectors), and
  `Array` for JavaScript arrays (`#js [...]`, `array`). An expression's value
  is the one it has where it is evaluated, after narrowing. Further methods:
  - `local(l)`: the type a local is bound to;
  - `returns(program, fn)`: what a `fn` returns;
  - `callee(program, call)`: the `fn` a call invokes;
  - `uses(l)`: how many expressions read a local;
  - `facts()`: the program-wide conditions.

  Rapid type analysis (211) reads `Known::Instance` and
  `ProgramFacts::implementors` for receivers. Call shapes (212) read
  `callee`, and the folder (221) reads operand types. `Inferred::truth_is_js`
  says whether JavaScript truthiness agrees with Clojure's: the type has no
  number, string or host class.
- **Sources.**
  - Literals, collection literals and `fn` have their own types.
  - Core calls go through the table: `Operation::call` for the argument
    types, joined with `returns`, so results grow monotonically.
  - A call of a known `fn` returns the union of its method bodies.
  - `BooleanHint` and calls of `^boolean` globals are `boolean`. These tags
    are trusted, as upstream trusts them.
  - Positional, map and `new` constructors of program types build instances
    of their type.
  - A global that is defined once and never assigned has its `def` site's
    type. A later top-level form reads the initializer's type; every other
    read may see `nil` first.
  - Locals bound by `let`, `loop` and `letfn` join their initializer and
    their `recur` arguments. Parameters, `catch` and `this-as` bindings are
    `any`.
- **Narrowing.** In an `if`'s branches, a local the test examines is
  narrowed. This is modeled on upstream's `infer-if` and `and_or` pass. The
  tests are:
  - truthiness;
  - the exact class predicates, in both branches: `nil?`, `some?`, `string?`,
    `number?`, `keyword?`, `symbol?` and `boolean?`;
  - `true?`, `false?` and `array?`, in the then branch;
  - the protocol predicates `vector?`, `map?`, `set?`, `seq?` and
    `record?`, in the then branch while core protocols are intact. Program
    types pass them too;
  - `instance?` of a program type;
  - `satisfies?` of a program protocol, which narrows to the classes of its
    implementations, and to the type when one program type is its only
    implementation.

  Tests combine through `not`, through the temporary of `and` and `or`, and
  through a local bound to a test. Some locals are `any` and never narrowed:
  type fields, locals that `set!`, `binding` or a `js*` hole may assign, and
  the parameters of functions that read `(js-arguments)`, which aliases them.
- **Fixed point.** Locals and expressions start at `never` and grow until
  nothing changes. Every transfer is monotone over a finite lattice, and
  calling `nil` gives `never`, so recursive functions converge. After
  `ROUND_LIMIT` rounds, everything is `any`.
- **Program-wide conditions.** `ProgramFacts` is the scan `Inert` used,
  moved here. It records:
  - replaced core operations, whose calls are `any`;
  - whether core protocols are extended beyond the program's types. If they
    are, `dispatch` and `invoke` calls are `any`;
  - sole definitions;
  - `ILookup` implementations per program type;
  - whether any program type implements `IEquiv`;
  - the implementations of each program protocol.
- **Lowerings.** At `Advanced` the emitter:
  - tests a value directly instead of with `truth` when its type allows it.
    Output that never calls `truth` does not declare it;
  - writes `(and a b)` as `(a && b)` and `(or a b)` as `(a || b)` when `a`'s
    type allows it and only the expansion reads its temporary. In value
    position the expansion's function scope goes away, so neither operand may
    read `arguments` or hold raw JavaScript;
  - writes `+ - * /`, `< <= > >=`, `inc`, `dec`, `identical?`, `nil?`,
    `some?`, `zero?`, `pos?` and `neg?` as JavaScript operators, whatever the
    operand types, because their runtime definitions are those operators. A
    comparison chain needs operands that can be read twice;
  - writes `(not x)` as `!x` when `x`'s type allows it;
  - writes `(= a b)` and `not=` as `===` and `!==` when a non-nil primitive
    decides it. For a keyword literal it uses 208's lowering, `x === k<n> ||
    rt$keywordFqn(x) === "k"`, or only the spelling test when `x` cannot be
    read twice. The literal may come second only when `a` is no program type
    or no program type implements `IEquiv`;
  - writes `==` on numbers as `===`;
  - writes `(count x)` as `.length` of a string or array, or of a flat
    vector's payload;
  - writes `(nth v i)` on a flat vector with a number index as the runtime's
    direct path, `rt$vectorNth(rt$vectors.get(v), i, rt$missing)`;
  - writes `(:k rec)` and `(get rec :k)` as the field's property when `rec`
    is a record of a known type with a field `k` and no program `ILookup`.

  Except for the operators, these lowerings need core protocols intact. A
  replaced operation keeps its call.
- **Table rows.** The `otherwise` column now gives the classes a call outside
  the domain can return for `seq`, `next`, `rest`, `contains?`, `empty?`,
  `every?`, `not-every?`, `not-any?` and `distinct?`. So `(if (seq xs) ...)`
  tests directly whatever `xs` is. The `core_operations` suite checks these
  rows on samples.
- **`invoke` rows.** `Inert` finishes step 6's `invoke` rows. `mapv`,
  `filterv`, `every?`, `not-every?`, `not-any?`, `some`, `group-by` and
  `run!` are inert when their collections are plain data and their function,
  a `fn` literal or core operation, is inert on the elements. `Shape` now
  carries the element types of literal collections.
- **Tests.** The `type_inference` suite in `tests/compiler/` runs every
  lowering at every level on adversarial values (`0`, `-0`, `NaN`, `""`,
  `undefined`, BigInts, host objects, built keywords, records with their own
  `ILookup`, programs that extend core protocols), and checks the lowered text.
- **Measured.** `(js/console.log (+ 1 2))` needs no runtime at `Advanced`
  and is 192 bytes; `raw-js` is 856. See `benchmarks/compiler.md`.

#### Phase 2 pure initializer and `js*` extension points

Steps 6 and 7 are implemented (light-cljs-213) in `src/application.rs`, at
`Advanced` only; `Prepare` output is unchanged except for the hygiene renames
under `js*` below.

- **Inert initializers.** `Inert` decides which unread top-level definitions
  `prepare_application` may drop. Below `Advanced` it accepts what `inert`
  always did (literals, `fn`, core values, lists and vectors of inert values,
  maps and sets with scalar keys). `Advanced` adds local reads, `let`, `if`
  and `do` over inert parts, `with-meta` through its row, calls of core
  operations whose row is `pure` or `allocates` for the argument types and
  whose arguments run no code (`-` calls, or `dispatch` over plain data:
  scalars and literal collections of plain data, never a call's collection
  result), the `hash-map` macro's expansion `PersistentHashMap.fromArrays`
  through the `hash-map` row, and `new T` or `->T` of a type defined once by a
  top-level `deftype` or `defrecord` that runs earlier and is never
  reassigned. `throws` and `effect` rows, calls outside a row's domain,
  global reads, `map->T` and everything else stay effectful. Since
  light-cljs-210, element-wise `invoke` rows are inert when their function
  argument is (see "Phase 2 type inference extension points"). Each
  expression gets a `Shape` (lattice type, plain-data flag and, for literal
  collections, element type).
- **Program-wide conditions.** `Inert::new` scans every expression once
  (since light-cljs-210 through `inference::ProgramFacts`, which inference
  shares): a
  core operation that is the target of `set!` or `with-redefs`, whose property
  is assigned, or that appears as a `js*` hole (raw text may assign its holes)
  is not trusted, and an `extend-type` or `specify!` of a core protocol (a
  `Builtin` method or marker) on anything but a program type makes no core
  call inert. Types and factories count their definitions (`Def`, `DefType`,
  `DefProtocol`) and assignments the same way.
- **Table rows.** `hash-map` and `array-map` list their even arities up to
  eight arguments with `allocates`, since only an odd count throws; longer
  calls stay unknown. Refining a row widens what is removable; the
  `core_operations` suite checks it.
- **`js*`.** Below `Advanced`, a retained `JsTemplate` still roots every
  top-level form and disables specialization. At `Advanced` it is an opaque
  leaf with `effect` purity: never inert itself, its holes ordinary children,
  and a global in a hole counts as escaped for specialization (raw text may
  assign it). `optimization::raw_js_names_resolve_only_to_holes_and_host_globals`
  in `tests/compiler/` checks the hygiene claim from raw text in every scope
  the emitter creates, trying each identifier of the output; it found and
  now guards `caught`, `caseValue` and `installed<n>`, which `hygienic_js`
  in `src/emitter.rs` renames since. A pellet that makes emitted code bind a
  new name must add it there and give the test a probe in that scope.
- **Measured.** The ratchet's `pure-defs` fixture removes 6 definitions at
  `Advanced` (1 at `Prepare`) and `raw-js` 1 (none below), and the generated
  programs' `unused-pure-def` garbage moves from `survives` to `residue`; see
  `benchmarks/compiler.md`.

### Constant collection literals

Tracked as Pellets 218-221 after keyword interning (208), whose constant
table they extend (see "Phase 1 constant table extension points"). Tiers 1
and 2 are implemented (218, below). Before them every literal was rebuilt per
evaluation: `[1 2 3]` copies, freezes and wraps a fresh array; `{:a 1}`
allocates the map each time (its keyword is interned since 208); `[]`
allocated although the runtime has `EMPTY` singletons. Below `Advanced`
non-empty literals still are. No hash cache exists for vectors, maps or
sets. Sharing is safe because payload arrays are frozen, `withMeta` returns
a new or unchanged object, and transients copy.

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
`js-obj` (mutable); regex (`lastIndex`); `#inst` (`Date` is mutable). As
implemented, a set literal whose members are not all scalars, keywords or
symbols is not constant either: its constructor throws on members that are
equal at run time (`#{[1] '(1)}`), and hoisting would move that throw to
load time. `#queue` is left out, and the reader has no record literals, so
no constant depends on a type yet.

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

#### Constant pool extension points

Tiers 1 and 2 are implemented (light-cljs-218).

- **Empty singletons (tier 1).** At every level, and in the REPL, macro host
  and deferred units, which share the emitter, `[]`, `{}`, `#{}` and `()`
  (quoted too) read `rt.core.PersistentVector.EMPTY`,
  `rt.core.PersistentArrayMap.EMPTY`, `rt.core.PersistentHashSet.EMPTY` and
  `rt.core.List.EMPTY` (`rt$PersistentVectorValue.EMPTY` and so on at
  `Advanced`) instead of allocating, as upstream does. They are the values
  the constructors build for no items, so only identity changes:
  `(identical? [] [])` is true at every level. A quoted `'#{}` keeps its
  positional constructor. Empty literals are inline reads, never table
  entries (`EmptyCollection` in `src/emitter.rs`).
- **Classification.** `constant_nodes` in `src/emitter.rs` classifies an
  expression of the prepared IR as above and memoizes the result per
  `ExprId` in a dense array (`ConstantClass`), so classifying a program is
  linear; a constant's class is its size in nodes. `composite` names the
  runtime constructor and items of a collection literal, reader metadata or
  `#uuid` literal, and `Emitter::construct` writes the call. Sites that build
  their value use the same two functions, so a pooled value is built exactly
  as a site builds it below `Advanced`.
- **Pool (tier 2).** At `Advanced` a constant collection literal, reader
  metadata of a constant on a constant collection or quoted symbol, and a
  `#uuid` literal read a table entry, which `Emitter::constant_part` declares
  bottom-up: scalars stay inline (`ConstantPart`), keywords and symbols are
  their 208 entries, and each nested composite is an entry of its own, keyed
  by `ConstantKey::Composite { constructor, parts }`. Hash-consing on these
  keys gives subterm sharing: `[[1 2] {:k [1 2]}]` declares `[1 2]` once and
  reads it twice. Keys are emitted structure, never runtime equality, so
  `[1 2]` and `'(1 2)` are two entries, and `#{1 1.0}` (a `LiteralSet` of two
  reader-distinct members) is not `#{1}`. A literal with a non-constant item
  is built at its site from its pooled constant items. So is a literal whose
  construction reads a core operation the program may replace
  (`PreparedApplication::replaced_core`): `with-meta`, `uuid`, or the type of
  an empty literal, whose `set!` later evaluations must see. A discarded
  constant emits nothing and declares nothing.
- **Lazy cells.** A constant of at least 1,024 nodes
  (`LAZY_CONSTANT_NODES`) is a lazy cell,
  `let c<n>; const k<n> = () => c<n> ??= <initializer>;`, read as `k<n>()`,
  and the entries first declared for its parts are lazy too, so a large
  constant that no evaluation reads is never built. The threshold is
  measured (`benchmarks/compiler.md`, "Constant collection pool"): a lazy
  cell saves QuickJS the construction of a constant that startup does not
  read, and costs V8 a second parse of the cell's body when startup does
  read it; below about a thousand nodes neither engine gains. The
  parenthesized form `(() => ...)`, which V8 compiles eagerly, makes QuickJS
  scan the whole body twice to find the arrow, so it is not used. A
  type-dependent constant would also need a lazy cell, because the table
  precedes the definitions, but the reader has no record literals yet.
- **Report.** `RetentionReport::constants` (`ConstantPool`) counts the
  entries, the composite entries and the lazy cells and the bytes of the
  declarations, and `--size-report` prints
  `constant pool: N entries (C composite, L lazy), B bytes` under its
  summary line. Below `Advanced` it is empty.
- **For 219 (hash caching).** A composite entry's key holds its constructor
  and parts, and parts precede their entries, so a Rust port of the runtime
  hash can compute each entry's hash from its parts' hashes in entry order
  (keywords and symbols from their spelling) and pass it to the entry's
  initializer. Pool values are built by the ordinary constructors, so the
  runtime hash cache applies to them unchanged.
- **For 220 (key shapes).** A map literal with constant keys and dynamic
  values is built at its site, with its keys read from the table. A shape is
  another `ConstantKey` variant (the keys' parts), declared with
  `constant_entry` from the map site in `value_expr`; `constant_nodes` tells
  which items are constant. The generator's `Literal` in
  `tests/support/generated.rs` is where shaped maps with colliding keys go.
- **For 221 (folding).** `constant_nodes` and `constant_part` give the value
  of any constant expression as parts, which a folder can evaluate `get`,
  `count`, `nth` and `contains?` against, keeping the throws of out-of-range
  arguments.
- **For 212.** Hoisted functions are entries under a new key variant;
  `constant_entry`'s initializer may reference globals only inside function
  bodies, and a function entry should never be lazy.
- **Tests.** `tests/compiler/constant_pool.rs` pins the empty singletons at
  every level and in the REPL, the table of a nested program, reader-distinct
  entries, metadata, transients, equality and hashes on pooled values,
  identity across sites, what stays at its site (`#js`, `js-obj`, regexes,
  `#inst`, throwing sets, partially constant literals), lazy cells, and the
  allocation fixture: the calls of the literal constructors in a loop of
  1,000 iterations, counted by a hook the test inserts into the emitted
  runtime functions, and the pool's size. Generated programs check property
  5 (see "Generated programs"), and the size ratchet has a
  `constant-literals` fixture, also a steady-state workload of `--levels`.

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

- A program registers its namespaces with the runtime only when it reaches
  reflection: when it reads, sets or binds `*ns*`, or calls `find-ns`,
  `the-ns`, `all-ns`, `create-ns`, the reader (`read-string`, `read`, the
  EDN readers) or `eval`, or uses a macro defined in the program or a quoted
  `#js` literal. Otherwise nothing can observe the registry. `resolve`,
  `ns-publics` and `ns-interns` expand to Vars at compile time and are not
  affected.
- A registered namespace lists only retained definitions. The reader's syntax
  quote resolves an unqualified symbol to the current namespace when it names
  one of its definitions; when that definition was removed and the name is
  also a `cljs.core` name, it resolves to `cljs.core` instead.
- Collection `toString` exists only when printing is reachable: when the
  program calls `str`, `pr-str`, `prn`, `print`, `println` or another
  operation that prints (light-cljs-207; see "Phase 1 print extension
  points"). Otherwise a collection's `toString` gives `[object Object]`, as
  `Object.prototype.toString` does, and `ExceptionInfo`'s is
  `Error.prototype.toString` (`Error: message`). This is the counterpart of
  upstream `:elide-to-string`. Error messages follow it: a message that
  shows a collection (`No value supplied for key: [1]`, `No matching clause:
  [1]`) shows `[object Object]` instead, and a multimethod or hierarchy error
  shows a value that is not nil, a boolean, number, string, keyword or
  symbol with its `toString`. Where printing is reachable every message is
  the one `optimize: false` gives.
- Equal constant literals may be `identical?` because they share one pooled
  value, and each evaluation of a constant literal is the same value. This
  holds for quoted symbols since light-cljs-208 and for constant
  collections, reader metadata on them and `#uuid` literals since 218.
  Keyword literals are interned at every level, and empty collection
  literals are the runtime's `EMPTY` singletons at every level, as upstream,
  so their identity does not differ. Equality, hashing, metadata and
  transients are unaffected. Mutating a pooled value through host interop
  (`set!` of a property, `reset-meta!`) is visible at every site that reads
  it.
- `set!` on a core operation forces indirection for that operation only.
- `js*` is opaque: raw text can name only its `~{}` holes and host globals,
  because the hygiene pass renames every name the compiler binds away from it.
  Raw text that reaches compiler state another way (building a renamed
  identifier for `eval`, patching host or runtime prototypes so that core
  operations run program code) is unsupported, and a definition only such
  text reads may be removed.
- `^boolean` tags are trusted, as upstream trusts them: a call of a function
  tagged `^boolean` is tested without `truth`, so one that returns a
  JavaScript-falsy value other than `false` (`0`, `""`) takes the other
  branch at `Advanced`. Since light-cljs-210.
- Everything else must produce results identical to `optimize: false`.

## Verification

- Differential execution of the full corpus at every level.
- Ratchet tests with ceilings per fixture and level.
- Generated multi-namespace programs with tagged dead code, checked by
  metamorphic properties at every level, with an expected-survivor list per
  garbage tag that only tightens.
- Retention report checked in under `benchmarks/results/` so size
  regressions name their cause (the first is
  `benchmarks/results/retention-2026-10-12/`).
- Startup and steady-state benchmarks in Node and QuickJS (`--levels` of
  `benches/compiler.rs`; the baseline is `benchmarks/results/levels-2026-10-13/`).
- The manifest validator in the test suite, so the runtime cannot regain
  hidden edges.

## Open questions

- Whether record field names should be mangled under an opt-in flag, given
  keyword lookup can go through a field table.
- Whether the sliced runtime should be emitted as top-level `const`s in the
  output closure or as a nested factory, considering QuickJS and V8 parse
  costs for large closures.
- How deep type inference should go before diminishing returns. Step 3
  infers booleans, numbers, strings, collections and program types within a
  function. Parameters stay `any`, so arithmetic lowers without types and
  typed `=`, `count` and `nth` mostly apply to locals. Parameter types from
  call sites need the escape analysis of call shapes (212).
