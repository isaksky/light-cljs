# Advanced optimization design

Status: design, October 2026. Phase 0's optimization levels, size ratchet,
differential test, runtime retention report and startup benchmark are
implemented (see "Phase 0 extension points"), and so are Phase 1's unit
manifest and advanced-mode runtime slicing (see "Phase 1 manifest extension points" and "Phase 1 slicing
extension points") and Phase 2's core operation table, type inference,
rapid type analysis of program types, call shapes, and folding, inlining
and build-time defines (see "Phase 2 core operation table extension
points", "Phase 2 type inference extension points", "Phase 2 rapid type
analysis extension points", "Phase 2 call shape extension points" and
"Phase 2 folding and inlining extension points"), and every tier of constant
collection literals, folding against them included (see "Constant pool
extension points" and "Constant folding extension points"), and Phase 3's
renaming, runtime stripping and terse syntax (see "Phase 3 terse emission
extension points"); the rest (ABI property mangling, a string table) was
measured and is not worth building (light-cljs-232, light-cljs-242). The work items are
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
  `js*` as an opaque leaf, since step 4 `src/rta.rs` drops the methods
  and types of a program that no instance can use, and since step 8 it folds
  constants and dead branches and inlines single-use and small pure
  functions.)
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
  `count`, `nth` and record lookups without calling the runtime. Since step 5
  of Phase 2, `Advanced` calls known functions through the method each call
  selects, defines a dispatcher only for functions whose value escapes, and
  writes a `do` in value position, such as a `def` initializer, as a comma
  expression. Since Phase 3, `Advanced` output has short names, dotted
  properties where the name is an identifier, fewer parentheses and no
  whitespace or comments, in the program and in the runtime slice.)
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
  checking that the literal itself is unchanged. Since 219 a read also
  compares a literal with the same or another literal built at its site and
  throws if they are `=` but hash apart, and the sizes ignore the digits of
  the hashes constant declarations embed, which follow the source paths in
  var metadata. Since 220 a fourth stream adds reads of shaped map and set
  literals (keys that hash alike, nil keys and values, one local at several
  keys, set members equal at run time), each compared in the program with
  the literal built with its first key behind `identity` and returning
  order-dependent hashes, which the levels compare. Observations are host
  values only (numbers
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

- **Format.** `//@unit <name>[ compile-time][ mutator][ effect[: <bindings>[ when: <bindings>]]][ requires: <bindings>]`
  headers in `src/js_runtime.js`, with `//@ ` continuation lines (since
  light-cljs-227 any list of the header may wrap) and a final
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
  escapes (`add.variadic`); see "Phase 1 effects extension points". Since
  Phase 2 step 4, `effect: a when: b, c` is kept only where one of the `when:`
  bindings is kept as well; see "Phase 2 rapid type analysis extension
  points".
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
  printer is not part of it: about 300 units, 119 KB. Since light-cljs-227 a
  program keeps the branches of the types it builds; see "Phase 2 builtin
  types extension points".)
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
  (Phase 2) has to cut. (Light-cljs-227 cut it; see "Phase 2 builtin types
  extension points".)
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
  methods of local functions that light-cljs-228 hoists. With `lazy` set, a
  new entry is a lazy cell instead (218):
  `let c<n>; const k<n> = () => c<n> ??= <initializer>;`, read as `k<n>()`,
  which builds the value on the first read.
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
   a field read; `(nth v i)` on a known vector becomes an index. The
   parameters of functions only the program calls join their calls'
   arguments (light-cljs-229).
4. **Rapid type analysis.** Instantiated types are `DefType`, `Reify` and
   reachable runtime constructors; reachable protocol methods come from
   calls and from `satisfies?`. Implementations of uninstantiated types and
   unreached methods are dropped. Calls whose receiver type set is a
   singleton become direct calls. Closure can do neither for protocols.
   Implemented for program types and protocols; see "Phase 2 rapid type
   analysis extension points" below, and for the runtime's builtin types
   "Phase 2 builtin types extension points".
5. **Call shape.** Direct `f(a, b)` for known targets with no `.call(null)`;
   arity entry chosen at compile time at every call site; no
   `arguments.length` dispatcher when the fn value never escapes; variadic
   calls to known callees build the rest seq directly; no IIFE around `def`
   initializers; local multi-arity fns hoisted when captures permit.
   Implemented; see "Phase 2 call shape extension points" below.
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
   Implemented; see "Phase 2 folding and inlining extension points" below.

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
  column to `UnitId`s, the roots of a direct reference for slicing. Since
  light-cljs-243 an arity may narrow the result (`[fn]:fn`), a `fn`
  parameter may carry a callback shape, and `returns` may give the elements
  of the collections a call returns; see "Phase 2 callback parameter
  extension points".
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
  `Inference::new(program, ProgramFacts::new(program, top), &closed)`, where
  `closed` is `call_shapes::closed_functions(program, exports)` (since
  light-cljs-229; see "Phase 2 parameter type extension points").
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
  - `facts()`: the program-wide conditions;
  - `rounds()` and `typed_parameters()`: how many rounds the fixed point took
    and how many parameters of closed functions are narrower than `any`
    (`ApplicationStats::typed_parameters`).

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
    their `recur` arguments. The parameters of closed functions join the
    arguments of their calls and their method's `recur` arguments (229).
    Other parameters, `catch` and `this-as` bindings are `any`.
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
  calling `nil` gives `never`, so recursive functions converge. Rounds walk
  the top-level forms forward and backward in turn: values flow forward from
  a definition to later reads, and parameter types backward from a call to
  the earlier definition it calls. After `ROUND_LIMIT` rounds, inference
  starts over without parameter types, and if that does not converge either,
  everything is `any`.
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

#### Phase 2 rapid type analysis extension points

Step 4 is implemented (light-cljs-211) for program types and protocols in
`src/rta.rs`, at `Advanced` only. `prepare_application` runs it after type
inference, which it reads, and rewrites the prepared program that emission
reads. The runtime's builtin types are light-cljs-227 ("Phase 2 builtin types
extension points").

- **Analysis.** `rta::analyze` walks the prepared program from its root, as
  emission will. It enters a method body of a `deftype`, `defrecord`, `reify`
  or `extend-type` only once the method can run, and iterates to the least
  fixed point. Two types whose methods only construct each other therefore keep
  neither.
  - A program type is *instantiated* when reachable code constructs it
    (`new T`, `->T`, `map->T`). It also is when code reads its constructor in
    any way other than as the class of `instance?` or the target of
    `extend-type`, because a value read escapes. Assignments count as such
    reads, and so does a `deftype` form whose value is used. A raw `deftype*`
    and an exported global always count.
  - A program protocol method is *dispatched* when reachable code reads its
    global, and *called* when it is dispatched or a call of it is
    devirtualized. A protocol is *observed* when code reads it or tests it
    with `satisfies?` or `implements?`. Code that names a slot as a string,
    property or method, or in `js*` text, calls its method and observes its
    protocol.
  - A method entry stays when it can run:
    - on a program type when the type is instantiated and the method called
      (its marker: the protocol observed);
    - on a `reify` or a `specify!` target when the method is called;
    - on a host type, `default` or a runtime type when it is dispatched.
  - Core protocol, `Object` and `IFn` methods stay with their instantiated
    type. Methods are tracked by name, so a called method keeps every arity.
- **Rewrite.** `TypeAnalysis::apply` does the following:
  - removes dropped entries from method lists;
  - makes a type without instances a bare constructor (no methods, record
    methods or factories) when `instance?` or `extend-type` reads it, and
    removes its form otherwise;
  - removes an `extend-type` left without methods when it targets a host type
    name or a type defined earlier at top level. The extension of a type not
    yet defined still fails;
  - does not define a map factory nothing reads, since `recordFromMap` keeps
    the generic core;
  - makes a `defprotocol` define only the protocol value and the dispatchers
    something reads;
  - removes a top-level definition that preparation removes when unread, once
    only dropped methods read it.

  `ApplicationStats` counts `uninstantiated_types`, `dropped_methods` and
  `devirtualized_calls`. Removed definitions add to `removed_definitions`.
- **Devirtualization.** Inference may type a call's receiver as an instance of
  one program type (`Known::Instance`) that implements the method at the
  call's arity. The call then reads the implementation from the receiver, as
  `protocolMethod`'s dispatcher does: `(r)["ns$P$m$arity$1"](r)`. When an
  argument cannot be read twice, it uses
  `((r0, a1) => r0["ns$P$m$arity$2"](r0, a1))(receiver, x)`, which keeps the
  evaluation order.
  - Conditions: the method is defined once and never assigned, takes no rest
    arguments, and its protocol is not `:extend-via-metadata`. Both the
    protocol and the type are top-level forms, the protocol first, so the
    dispatcher the original call reads exists wherever an instance does.
  - A later `extend-type` or `specify!` replaces the slot the call reads.
  - `rta::protocol_slot` is the Rust port of the runtime's `protocolSlot`.
  - It applies to `let` locals, constructor calls, receivers narrowed by
    `instance?` or `satisfies?`, and since light-cljs-229 the parameters of
    closed functions that every call passes an instance of the type.
- **Definitions in emitted code.** `ProgramFacts` now counts definitions only
  where the root reaches them. `defprotocol` leaves a `def` without a value
  for each of its Vars in the arena, so before this no protocol method was
  ever `Definition::Protocol`.
- **Type machinery in the runtime.**
  - `makeType`, `installMethods`, `reify` and `protocolMethod` no longer keep
    the generic core. Through the `typeHooks` unit they read the record
    methods, the variadic `applyTo` of callable types, metadata dispatch
    (`metadataMethod`) and the rest arguments of variadic protocol methods
    (`fnRest`).
  - Each hook is set by an install unit of the providing binding. The emitter
    keeps that binding as a `NameUse::Keep` root where the program needs it:
    `recordMethods` for an instantiated record, `metadataMethod` and `fnRest`
    for a protocol extendable through metadata or with a variadic method.
    `fnApplyTo`, which every variadic function keeps anyway, sets its hook
    wherever `ifnInvokeEntry` is kept.
  - A new hook follows the same pattern: kept by a `when:` install of the
    binding that provides it.
  - Each record method is a unit (`recordMethods.hash`, ...) that adds a
    builder to `recordMethodTable`. `recordMethods(Type)` installs every
    record protocol's marker and the table's methods.
- **Conditional effects.** The manifest header `effect: a when: b, c` (see
  `src/runtime_manifest.rs`) keeps a unit only where a target is kept and so
  is one of the `when:` bindings: runtime methods are RTA in the manifest.
  - A record method belongs to `recordMethods`, which only a program with
    records keeps. It lists the operations that dispatch its protocol method:
    `recordMethods.count effect: recordMethods when: count, seqProtocol`.
  - `RuntimeManifest::reached_by_uses` makes a conditional effect wait for
    its condition, and `RuntimeManifest::conditions` lists a unit's
    conditions.
  - The `runtime_manifest` suite checks every `when:` list. A unit
    installing protocol methods must name every unit whose body holds one of
    their keys other than as a method entry. A hook install must name every
    reader of its hook. A new dispatch site fails until it is listed.
- **`getBasis`.** At `Advanced` a type gets its static `getBasis` only when
  reachable code names it: a property read, a method call or index, a string,
  or raw `js*` text. It builds a vector of symbols, which kept the generic
  core for every type. See "Advanced-mode contract".
- **Tests.**
  - The `rapid_types` suite in `tests/compiler/` runs every program at every
    level. It checks the `Advanced` text for what goes (with sentinel
    strings) and for devirtualized calls. It covers:
    - every way to construct or reach an instance;
    - the least fixed point;
    - host extensions;
    - argument order;
    - `extend-type`, `specify!` and metadata extension;
    - munged slot names in a namespace with dashes;
    - the record methods each operation keeps;
    - `getBasis` and callable types.
  - The ratchet fixture `three-records` has three records implementing one
    protocol, of which one is constructed.
  - The generated programs' `uninstantiated-type` and
    `uncalled-protocol-method` garbage is `removed`.
- **Measured.** At `Advanced`, `record-protocol` drops from 148,802 to 11,637
  bytes, `pure-defs` from 145,151 to 11,471 and `three-records` from 150,433
  to 11,642. A program that keeps the generic core grows by 65 bytes
  (`typeHooks` and the `fnApplyTo` install), and the shared runtime by 1,260
  bytes. See `benchmarks/compiler.md`.
- **Left for later.**
  - The runtime's builtin types (light-cljs-227): a program that builds any
    collection, keyword or symbol keeps the 301-unit generic core (119 KB),
    because generic operations and builtin prototypes reference every
    collection type. Done; see "Phase 2 builtin types extension points".
  - A per-arity method set.
  - `IPrintWithWriter` and the other core protocol methods of program types
    whose operations the slice does not keep. This needs the slice before
    emission, a second emission like the reflection one.
  - The positional factory of a type that is only built with `new`.

#### Phase 2 builtin types extension points

Step 4 is implemented for the runtime's builtin types (light-cljs-227), in
the runtime and its manifest. A program keeps the generic operations it
calls, the builtin types it can construct, and for each of those types only
the branches of the kept operations that handle it.

- **Protocol hooks.** A generic operation (`seq`, `count`, `get`, `=`,
  `hash`, `conj`, ...) reaches protocol machinery only through
  `protocolHooks`: `builtin`, `publicMethod`, `satisfies`,
  `implementsProtocol` and the core dispatchers operations call themselves
  (`countProtocol`, `nthProtocol`, `firstProtocol`, `nextProtocol`,
  `keyProtocol`, `valProtocol`). The table is empty until
  `protocolHooks.install`, an effect of `installMethods`, fills it in. Every
  protocol implementation goes through `installMethods`: deftype, defrecord,
  reify, specify!, extend-type, extend-protocol and the runtime types built
  with it (Eduction, TaggedLiteral, `iteration`); `extendType` requires it.
  Where it is not kept no value has a protocol method or marker, so the
  empty table answers what the machinery would:
  - `protocolHooks.builtin?.(...)` and `protocolHooks.publicMethod?.(...)`
    give `undefined`, as `builtin` and `publicMethod` do;
  - a protocol test calls the native predicate for its protocol, what
    `builtinCoreImplementation` answers (which now calls them too):
    `countedValue` (with nil, the `ICounted` marker), `seqValue`,
    `nextValue`, `seqableValue`, `indexedValue` and `sequential`;
  - a dispatcher call is its fallback (`count`, `nth`, `first`, `next`,
    `entryKey`, `entryVal`), which the dispatcher calls whenever the value
    implements its protocol natively, as every value at these call sites
    does.
  The protocol machinery (`builtinCoreImplementation`, `coreProtocolNames`,
  `protocolSlot`, ...) is about 18 KB.
- **Host iterator seqs.** `seq` of a host iterable was an object built by
  `installMethods`, so every program keeping `seq` kept the protocol
  machinery. It is now a plain object that `seq`, `first`, `rest` and
  `builtinCoreImplementation` (ISeq, ISeqable) know from the `iteratorSeqs`
  store. It has no protocol slots, which only compiler-owned names could
  observe; `satisfies?`, printing, equality and hashing give what they gave.
- **Kind hooks.** A branch of a generic operation that only one kind of value
  takes is `kindHooks.<operation><Kind>`, installed by an effect whose
  targets build the kind and whose `when:` names the operation:
  `makeVector.seq effect: makeVector when: seq` fills in
  `kindHooks.seqVector`. A kind is a payload store (`vectors`, `maps`,
  `sets`, `cells`, `views`, `queues`, `lazyValues`, `transients`, `chunks`,
  `deferredVarMetadata`), built by every unit writing to it, or a narrower
  kind that only named units build (maps with a `tree` slot, by
  `treeMapConstructor`; maps of the hash-map kind, by their type, and sets
  of it, since light-cljs-231 by `putMember` and the public constructor).
  A branch for a kind that no kept unit builds is never taken. The hooks
  cover `seq`, `rest`, `count`, `nth`, `lookup`, structural equality,
  `hash`, `meta` (a Var's deferred `:test`), hash-map traversal, entry
  search and insertion; `KIND_HOOKS` in the `runtime_manifest` suite lists
  them with their kinds.
- **Kind tokens.** Factories of several types take the type from the caller,
  so a caller keeps only the types it builds: `view(Type, ...)` (a view
  records its type to rebuild itself further on), `makeMap(Type, ...)` and
  `makeSet(Type, ...)`. Code building a collection of an existing value's
  kind reads the type from `mapTypes` or `setType`, which the `.kind` effect
  of each map and sorted set type fills in. The empty vector is
  `makeVector([])`, so the vector type keeps no variadic `vector`.
- **Lazy entry hashes.** Map and set entries that compute their hash on
  demand call `entryHashing.hash`, which `hash.entries`, an effect of `hash`,
  sets. Every reader of an entry's hash keeps `hash` itself, so a map that is
  never hashed keeps no hashing.
- **Fields.** `publicFields` takes the `meta` reader and a `write` function
  for mutable fields from the type (`cellStateField`, `rangeChunkField`) and
  munges its field names itself, so a keyword's fields keep neither `meta`
  nor `mungeMember`.
- **Manifest.** Continuation lines continue the whole header, so long effect
  target and `when:` lists wrap, and the update wraps them. Each protocol
  declaration is its own unit and `protocolDefaults` is one effect per
  protocol (`ICounted.defaults`).
- **Tests.**
  - `runtime_manifest`: the `when:` check covers `kindHooks` and
    `protocolHooks` (a reader that is itself an effect stands for its own
    `when:` list), and `kind_hooks_are_installed_wherever_their_kind_can_be_built`
    requires every unit writing to a hook's store to be a target of its
    install. A new hook, or a new unit building a kind, fails until listed.
  - `builtin_types` in `tests/compiler/` runs each kind's branch of the
    generic operations, host iterator seqs, protocol extensions of builtin
    and host types and the host-facing methods of builtin prototypes at every
    level against `None`, and checks what small programs keep. It also runs
    the collection corpora of other suites and the 729 programs of
    `compiler-language.tsv` through `pr-str` at every level.
  - The ratchet fixtures `vector-only` and `keyword-only`.
- **Measured.** At `Advanced`, against master `75d9dd0`:
  - `(js/console.log :a)` drops from 119,502 to 9,963 bytes (301 to 41
    units), `(js/console.log [1 2 3])` from 119,659 to 29,293 (93 units) and
    `(js/console.log {})` from 119,528 to 39,501 (127 units).
  - `(conj [1] 2)` drops from 119,520 to 33,325 bytes, `(fn [& xs] xs)` from
    119,701 to 24,755 and `keyword-case` from 119,902 to 10,363. A program
    that assocs into a map keeps both map types and hashing, about 65 KB.
  - Fixtures that print keep the printer and drop about 28 KB (`println-hi`
    from 134,618 to 106,105); `seq-pipeline` drops from 137,887 to 81,638.
  - Programs without builtin values do not change.
  - The steady-state collection workloads run 2.3 to 11 times faster at
    `Advanced` in Node and QuickJS: without protocol implementations a
    generic operation no longer looks up protocol methods before taking its
    own branch.
  - The shared runtime grows by 12,907 bytes (3.6%): the hook tables, their
    installs, the native predicates and the moved branches' wrappers.
  Per-fixture sizes and timings are in `benchmarks/compiler.md`.
- **Left for later.** light-cljs-230 removes what was left; see the next
  section.

#### Phase 2 builtin types, second pass

light-cljs-230 finishes step 4 for the runtime's own types. A program keeps
the protocol machinery only with a protocol implementation, the printer's
map operations only where it passes print options, and the seqs behind a
collection's host-facing methods only where it builds another kind of seq.

- **Native protocol tests.** Each core protocol has a native test,
  `<name>Value` (`countedValue`, `lookupValue`, `editableValue`,
  `reduceValue`, `iterableValue`, `stackValue`, `printableValue`, ...): what
  `builtinCoreImplementation` answers for that protocol, including its
  exceptions for cycles, repeats, ranges, chunked conses and transients.
  `builtinCoreImplementation` is now the table `nativeImplementations` of
  these tests.
- **Core dispatchers.** `coreMethod(key, arities, fallback, native)` calls a
  protocol implementation through `protocolHooks.builtin`, else `fallback`
  where the protocol's test `native` holds (`everyValue` for IHash and IEquiv,
  `noValue` for the protocols no runtime value implements), else throws as
  before. A dispatcher keeps only its own test, not `builtin`,
  `builtinCoreImplementation`, `coreProtocolsByName` and the protocol
  objects.
- **Protocol tests.** Every runtime test of a core protocol reads
  `protocolHooks` and falls back to the native test, as 227's generic
  operations do: the `is*` predicates (`seq?`, `coll?`, `map?`, `counted?`,
  `iterable?`, `uuid?`, `inst?`, ...; `satisfies` adds a protocol's defaults,
  such as `ICounted` of nil, so these do too), `reduce` (IReduce, IIterable),
  `into` (IEditableCollection), the chunked seqs of `concat`, `drop` and
  `map`, `print-meta?`, `clj->js` and `js->clj`.
- **String writer.** `StringBufferWriter` implements `IWriter` natively
  (the `stringBufferWriters` store, `writerValue`), without
  `installMethods`.
- **Print options.** Options other than nil reach the printer only from its
  public entries `pr-str-with-opts`, `pr-seq-writer`, `pr-sequential-writer`,
  `print-meta?` and `-pr-writer` (a protocol implementation receives the
  printer's options and can only pass new ones through these). The printer
  calls their bodies (`printText`, `printSeq`, `printSequential`,
  `printsMeta`), which the entries wrap with their arity check, and reads
  options other than nil only through `printHooks.option` and
  `printHooks.optionsWriter` (`:alt-impl`), which the entries' `printOptions`
  effects install. A program that prints without options keeps no `assoc`,
  map lookup or keyword construction for them.
- **`str` and `atom`.** `str` prints a collection through
  `kindHooks.strCollection`, which the units building collections install,
  so `(str 1 2)` keeps no printer. At `Advanced` the emitter writes
  `(atom x)`, an atom without options, as `new rt$Atom(x, null, null, null)`
  (`Emitter::lowered_core_call`), so it keeps no option parsing (`seqToMap`,
  `into`).
- **Transients and `into`.** `persistent!`, `conj!` and transient lookup
  reach a map's or a set's branch through kind hooks (`persistMap`,
  `persistSet`, `conjTransientMap`, `transientEntryIndex`, ...) that the units
  building maps and sets install, so `(into [] ...)` keeps neither type.
  `-with-meta` of a function reaches `fnWithMeta` through
  `kindHooks.withMetaFn`, or `protocolHooks.fnWithMeta` with protocol
  implementations, so `into` keeps no function metadata.
- **Host-facing methods.** Without protocol implementations a vector answers
  iteration, `indexOf`, `lastIndexOf` and its `cnt` field from its items as
  `seq` walks them (`vectorStorage`, which includes the tail that the public
  constructor received, `itemIterator`, `vectorIndexOf`, `vectorCount`), and
  a map answers iteration, `keys`, `values`, `entries` and `cnt` from its
  entries (`ownMap`, `mapIterator`, `mapCount`). Two vectors compare their
  items (`kindHooks.equivVectors`, from `makeVector`) unless another
  sequential kind or a protocol implementation exists
  (`kindHooks.equivSequential`, from `cons.equiv`, whose targets include
  `installMethods`). So a program that builds only vectors and maps keeps no
  seq. With protocol implementations, for a sorted map its public
  constructor gave a tree, and for a receiver of another type, the methods
  call the generic operation through `hostHooks`, which effects of the
  generic operations install (`es6Iterator.host`, `sequentialIndex.host`,
  `count.host`, `mapProjection.host`, `es6EntriesIterator.host`, each with
  `installMethods` among its targets). The contract below lists what this
  leaves unsupported.
- **Map literals.** At `Advanced` a map literal of at most 8 entries that is
  not shaped (light-cljs-220) calls `arrayMapFromArray`, which builds only an
  array map, so `{:a 1}` keeps no hash map.
- **Tests.**
  - `builtin_types`: the host-facing methods of vectors (public constructor
    tails, subvecs, map entries, 40 items) and maps (array, hash, sorted,
    public constructors, a tree), with and without protocol extensions;
    printing with options, `str`, atoms with options, `into`, `reduce` and
    dispatchers; all at every level against `None`. What small programs keep:
    no seq for a vector or a map, no hash map for a map literal, no printer
    for `str` of scalars, no option parsing for an atom.
  - `runtime_manifest`: `printHooks` and `hostHooks` are hook tables that
    the `when:` check covers; `KIND_HOOKS` lists the new kind hooks.
  - Ratchet fixtures `map-only` and `atom-str`.
- **Measured.** At `Advanced`, against master `c45cd00`:
  - `(js/console.log [1 2 3])` drops from 29,293 to 13,900 bytes (target 25
    KB), `(js/console.log {})` from 39,501 to 22,700 (target 30 KB) and
    `(js/console.log {:a 1})` from 56,355 to 25,795.
  - `(fn [x] (str x 1))` drops from 104,255 to 1,151 bytes (`(str 1 2)` folds
    at compile time since light-cljs-214), `(atom 1)` from 112,428 to 3,993,
    `println-hi` from 106,105 to 43,926, `(into [] (range 3))` from 116,228
    to 63,397 and `seq-pipeline` from 81,638 to 69,798.
  - A program that builds another kind of seq as well as vectors or maps
    keeps both the native methods and the generic ones: `(conj [1] 2)` grows
    from 33,325 to 35,554 bytes, `(assoc {} :a 1)` from 65,654 to 67,369 and
    `#{1 2}` from 57,854 to 59,923. A program with protocol implementations
    keeps every native test as well: `closed-params` grows from 79,528 to
    82,559.
  - The shared runtime grows by 8,288 bytes (2.2%; 1,906 gzipped): the
    native tests, the printer's wrapped entries, the kind hooks and the
    vectors' and maps' native methods.
  Per-fixture sizes are in `benchmarks/compiler.md`.
- **Left for later** (light-cljs-231, done; see the next section). A set
  kept the generic seqs and both map types: its `hash-map` field built a
  map, and `setFromArray` could build a hash set.

#### Phase 2 builtin types, sets

light-cljs-231 gives sets what light-cljs-230 gave vectors and maps, so a
program that builds small sets keeps no seq, hash map or hashing.

- **Host-facing methods.** Without protocol implementations a set answers
  iteration, `keys`, `values` and `entries` from its members in the order
  `seq` walks them (`ownSet`, `setIterator` over `setTraversal`), and `has`
  with `lookup`. Two sets compare their counts with `setCount` instead of
  `count`, and their members with `lookup` instead of `contains`. With
  protocol implementations, and for a receiver of another type, the methods
  and the comparison call the generic operations through `hostHooks`
  (`hostHooks.has`, from `contains.host`; the iterator hooks now name
  `setPrototype` in their `when:` lists).
- **Hash-set kind.** A set has the hash-map kind (or another map's kind)
  only where `putMember` promoted it or the public constructor took the kind
  of the map it received; every other builder copies an existing set's kind
  or builds an array set. The hash hooks (`hashTraversal`, `hashEntryIndex`,
  `putHashEntry`) are installed by effects of those two instead of the set
  type. The set type's constructor binding is what builders instantiate, so
  `PersistentHashSetValue` now calls `typeHooks.hashSet`, which
  `publicHashSet.install` fills in with `publicHashSet`, the constructor's
  body. The emitter keeps `publicHashSet` (a `NameUse::Keep` root, as for
  `recordMethods`) wherever the program names `PersistentHashSet` other than
  by an empty set literal, which reads `PersistentHashSetValue.EMPTY`
  directly; `constantSetFromArray` calls it itself. The contract below lists
  what this leaves unsupported.
- **Public fields.** The `hash-map` field of a set its constructor did not
  build is an array map for an array set (`arrayMapFromArray`), else
  `kindHooks.hashSetMap` (`mapFromArray`, installed with the hash-set kind's
  builders). A set the public constructors built counts the value they
  received through `kindHooks.countPublicSet`, installed by
  `publicHashSet` and `treeSetConstructor`.
- **Set literals.** At `Advanced` a set literal of at most 8 constants calls
  `arraySetFromArray` (exported), which builds an array set without
  `putMember`, as `setFromArray` does for so few members.
- **Membership in value position.** See "Constant folding extension
  points": `(contains? #{:a :b} x)`, `(#{:a :b} x)` and `(get #{:a :b} x)`
  outside an `if` statement compare the value with each member, so they
  build no set.
- **Tests.**
  - `builtin_types`: the host-facing methods of sets (array, hash and
    sorted sets; sets the public constructors built from maps of every kind,
    from `nil` and from a vector; quoted literals; transients of them), with
    and without protocol extensions; what `#{1 2}` keeps (no seq, hash map,
    hashing or promotion), and that naming the type or a larger literal keeps
    hashing; membership in value position, all at every level against
    `None`.
  - `runtime_manifest`: `KIND_HOOKS` lists `hashSetMap` and `countPublicSet`
    and the hash hooks' new builders; 79 conditional effects.
  - `constant_lookups`: the `Advanced` text of membership in value position.
  - Ratchet fixture `set-only`.
- **Measured.** At `Advanced`, against master `58f8c05` (terse output), raw
  / gzip bytes:
  - `(js/console.log #{1 2})` drops from 27,667 / 8,512 to 11,924 / 3,910
    (188 to 104 units: those of `(js/console.log {:a 1})` but its keyword's,
    through the array map of the `hash-map` field, and 14 of the set's own).
    The new `set-only` fixture drops from 27,704 to 11,961.
  - A membership test in value position, an exported `(defn f [k]
    (contains? #{:fast :safe} k))`, drops from 28,666 / 8,796 to 4,412 /
    1,679, as small as the `if` form (4,444); with a number or string set
    from 27,755 and 27,769 to 241 and 256.
  - A program that keeps both the native methods and hashing grows: `#{1 2
    ... 9}` from 27,680 to 28,011 and `(conj #{1} 2)` from 34,457 to
    34,780; `constant-keys`, which builds sets with a dynamic member, from
    45,095 to 45,280. Programs that keep `contains` and equality, as every
    map program does, gain the 9-byte `contains.host`.
  - `(conj [1] 2)` (16,867) and `closed-params` (37,754 to 37,763) keep the
    vectors' and maps' native and generic paths as before: this change
    makes neither smaller.
  - The shared runtime grows by 2,388 bytes (0.6%; 555 gzipped).
  Per-fixture sizes are in `benchmarks/compiler.md`.
- **Left for later.** A set keeps the array map type, about 5 KB, for its
  public `hash-map` field; installing the field only where the program names
  it, as `getBasis`, would drop it.

#### Phase 2 builtin types, seq operations and the printer

light-cljs-238 slices the printer and the generic seq operations by the
kinds a program builds. The printer is polymorphic over those kinds, not
over all of them, and the generic operations call protocol implementations
only where the program has some.

- **Kind printers.** `printWriterImpl` prints nil, strings, numbers,
  booleans and functions itself and passes anything else to
  `kindHooks.print`, a chain of the printers of the kinds the program
  builds, each added by an effect of the kind's builders (`makeVector.print
  effect: makeVector when: printWriterImpl`): `ExceptionInfo`, transients,
  atoms and volatiles, delays, keywords, symbols and Vars, UUIDs, vectors,
  the other sequential kinds (`cons.print`), maps and sets. Each is
  `printKind(previous, test, print)`; the chain ends in `printHost`, which
  prints host values (errors, dates, regular expressions, arrays, objects,
  JavaScript symbols). The printer's metadata prefix is
  `kindHooks.printMetadata`, from the builders of the kinds `metaValue`
  tests and `installMethods`. `ExceptionInfo`'s `toString` is installed with
  its printer.
- **Walks without seqs.** Where no protocol implementation exists, which
  could see the seq of an array, `printSequential` walks an array by index
  and anything else through `printHooks.seq`, `first` and `next`, which the
  builders of sequential kinds, `pr-sequential-writer` and `installMethods`
  install. So a vector prints its items as `seq` walks them
  (`vectorStorage`), maps and sets their entries, `#js` arrays and objects
  their elements, all without `seq`. `pr-str` and the other variadic
  printers print their arguments by index (`printValues`), through
  `printText` where protocol implementations exist (`printHooks.seqText`).
- **Names in printed maps.** Lifting a namespace out of a map's keys
  (`*print-namespace-maps*`) is `kindHooks.namespaceOf` and
  `kindHooks.unqualified`, from the keyword and symbol builders. A key of a
  JavaScript object that reads as a keyword prints as `:key` without
  building one (`printJsKey`), unless options, protocol implementations or
  the program's keyword type could see the keyword
  (`printHooks.jsKeyword`).
- **Printer hooks.** `printHooks` starts empty: the fallbacks for collection
  `toString` and messages (`printHooks.defaults`) and the printer's
  replacements (`printWriterImpl.install`) are kept only with a unit that
  reads them, so printing a string keeps no collection `toString`.
- **Protocol wrappers.** `seq`, `first`, `rest`, `next`, `count`, `nth` and
  `reduce` no longer start by looking up a protocol implementation. The
  effect `seq.protocol effect: installMethods when: seq` replaces the
  operation with `behindProtocol(seq, ...)`, which calls the implementation
  the old prologue found and else the operation, under the operation's
  name; `nth.protocol` and `reduce.protocol` do the same for their
  arities. A program without protocol implementations keeps neither the
  wrappers nor the prologues.
- **Vectors in seq operations.** A program that builds no vector keeps none
  for the seq operations' sake: `iterator`'s branches for vectors and for
  array maps from the public constructor (`kindHooks.iteratorVector`,
  `kindHooks.iteratorArrayMap`), the `vec` field of a ChunkedSeq its
  constructor did not build (`kindHooks.vectorOfItems`), `drop` of a vector
  or a public ChunkedSeq (`kindHooks.dropVector`,
  `kindHooks.dropChunkedSeq`) and the chunk rest of a public ChunkedSeq
  (`kindHooks.chunkedVectorRest`) are kind hooks. `reduce` of `iterate` and
  `cycle` seqs is `kindHooks.reduceIterate` and `kindHooks.reduceCycle`.
- **Upstream fields.** Each runtime type's upstream fields and methods,
  which upstream are compiler-owned names that Closure renames and removes,
  are its `.fields` effect (`Cons.fields effect: Cons when: resetMeta,
  upstreamMembers`), apart from its host-facing `.members` (`indexOf`,
  `lastIndexOf`, iteration, `equiv`). The `when:` unit `upstreamMembers`
  is a marker that only an `Advanced` slice roots, where rapid type
  analysis finds a reachable expression naming one of `UPSTREAM_MEMBERS`
  (`TypeAnalysis::names_upstream_member`; `names_property` now matches
  `js*` text by identifier); `resetMeta` assigns `meta`. So `publicFields`
  and the stores it tests are kept only then. The hooks only fields read
  (`kindHooks.vectorOfItems`, `kindHooks.hashSetMap`, `hostHooks.count`,
  `protocolHooks`) wait for the same condition.
- **Reduced values.** `reduce`, `reduce-kv` and the `iterate` and `cycle`
  reductions take a `reduced` value apart with `reducedValue`, which calls
  `deref` only where a protocol implementation can give another value
  (`protocolHooks.deref`), so they keep no `deref` dispatch.
- **Messages.** The printer fallback for values in multimethod and
  hierarchy messages (`printHooks.messages`, with `printNumber`) is no
  longer kept with every collection's `toString` fallback.
- **Core cells.** The initial values of the cells `hostClock`, `hostRandom`,
  `execTapFn` and `rand` are units of their own (`coreCells.rand effect:
  coreCells when: randomSample`), kept with a runtime reader of the cell or
  where the program reads it: a root `rt$coreCells.<cell>` keeps the unit
  `coreCells.<cell>` (`RuntimeRoots::units`). So printing keeps neither the
  host clock nor the random source.
- **Smaller changes.** `enable-console-print!` sets the print functions
  without calling `set-print-fn!`, and the types whose prototype is a plain
  object (`Delay`, `Eduction`, `ChunkBuffer`, `arrayChunk`,
  `StringBufferWriter`) no longer call `builtinType`, which gave them the
  prototype a function already has.
- **Tests.**
  - `runtime_manifest`: `KIND_HOOKS` names a hook that several units
    install, each for its own kind, once per unit (`print/makeVector.print`);
    a unit wrapping an operation needs exactly that operation in its
    `when:` list, and a protocol key in a wrapper stands for the operation;
    `coreCells` is a hook table for the cells' initial values; a `.fields`
    unit waits for exactly `resetMeta` and `upstreamMembers`, which the
    export list does not reach; 145 conditional effects.
  - `builtin_types`: at `None` the prototype members of the runtime's types
    are host-facing or exactly `UPSTREAM_MEMBERS`; a program naming none
    keeps no `.fields` unit nor `publicFields`; programs naming a field as
    a property, method, string or in `js*` text, or calling `alter-meta!`,
    read the fields at every level as at `None`.
  - `size_ratchet`: the new fixture `mapv-count`; the printing and
    pipeline fixtures' ceilings drop. The printer's behavior over every kind,
    options, metadata, levels and lengths, namespace maps, `#js` values and
    protocol extensions is covered by the existing printing suites at every
    level against `None`.
- **Measured.** At `Advanced`, raw / gzip bytes, against master `49e64cc`:
  - `println-hi` drops from 21,456 / 6,737 to 4,771 / 2,021 (target 5,000),
    `vector-printed` from 24,932 / 7,794 to 9,703 / 3,740 (target 15,000),
    `map-printed` from 28,179 / 8,662 to 13,514 / 4,825 (target 20,000)
    and `seq-pipeline` from 33,742 / 10,138 to 24,450 / 7,610 (target
    25,000), of which the upstream fields were about 3.7 KB.
  - `(js/console.log (count (mapv inc [1 2 3])))` (the new `mapv-count`)
    drops from 33,935 to 28,725, a `defmulti` program (in
    `benchmarks/compiler.md`) from 57,807 to 52,739, `constant-literals`
    from 29,502 to 20,612 and `(conj [1] 2)` from 16,867 to 13,759.
  - A program with protocol implementations keeps the wrappers (about 200
    bytes) but no longer the fields: `closed-params` drops from 37,763 to
    35,923. No fixture grows.
  - The shared runtime grows by 6,994 bytes (1.8%): the kind printers and
    their chain, the walks without seqs, the protocol wrappers, the new kind
    hooks, the types' `.fields` units and their comments.
  Per-fixture sizes are in `benchmarks/compiler.md`.
- **Left for later.** The host-facing members stay with their type:
  `seq-pipeline` keeps `seqMethods`, iteration and, through `equiv`,
  structural equality (about 1.4 KB), which only a program whose seqs
  cannot reach the host could drop. Upstream members of other runtime
  values (a Var's `sym`, `val` and `isMacro`) are not sliced. `seq`,
  `first`, `rest`, `count` and `nth` still probe the stores of kinds the
  program does not build (a few hundred bytes).

#### Phase 2 builtin types, program types

light-cljs-246 separates the two things `installMethods` stood for. A
program type (a `deftype`, `defrecord` or `reify`) implements protocols for
its own values, which the generic operations dispatch to through their own
slots. Only `extend-type` and `specify!` (`extendType`, `extendInstance`) give
the runtime's values or host values implementations, which the runtime's own
paths for them must then defer to. Before, a program type with a vector or a
keyword kept about 30 KB of runtime that neither kept alone, through the
host-facing methods of the vector and the keyword.

- **Extended values.** The host-facing methods and upstream `cnt` fields of
  vectors, maps and sets take the generic operations (`hostHooks`) only
  where `protocolHooks.extend` is set, which `protocolHooks.extend effect:
  extendType, extendInstance` does, instead of wherever `protocolHooks.builtin`
  is. So `count.host`, `contains.host`, `sequentialIndex.host`,
  `es6Iterator.host`, `es6EntriesIterator.host` and `mapProjection.host`
  target `extendType` and `extendInstance` instead of `installMethods`. The
  contract below lists what this leaves unsupported.
- **Protocol hooks.** `protocolHooks.install` is one install unit per hook
  (`protocolHooks.builtin`, `.ownMethod`, `.satisfies`,
  `.implementsProtocol`, `.dispatchers`), each waiting for the readers of
  its hook, so a program keeps only those its operations read.
  - `builtin` is the value's own method (`directMethod`); the lookup of
    `extend-type` on host types, `object` and `default` (`externalMethod`:
    `coreDispatchers`, `nativeMethods` and `builtinCoreImplementation`) is
    `protocolHooks.externalMethod`, which `protocolHooks.external`, an effect
    of `extendType`, installs: `extendType` is the only unit that fills those
    tables.
  - `protocolHooks.publicMethod` is `ownMethod`, installed with
    `installMethods`, and `publicMethod`, installed later with `extendType`
    and `extendInstance`. Without them only values that declare a protocol
    (its marker or mask bit) have its methods, so `ownMethod` returns such a
    value's method, nothing for a value that does not declare the protocol
    (a host object holding a slot-named function is no lookup), and throws
    for one that declares it without the method, as `publicMethod` does,
    except where the runtime implements it natively (`IMeta` of a function,
    whose slot counts without a marker, or of a `reify`). A record has the
    method of every operation the program keeps, since each record method
    waits for the operations dispatching it.
  - A core protocol dispatcher read as a value, `(aset -count "object" f)`,
    can be extended by type through its table as `extend-type` does, so
    emission keeps `extendType` for it (`Emitter::escaped_dispatcher`), and
    with it `externalMethod`. Before, this worked only in a program with a
    protocol implementation.
  - Maps and sets compare with values of a program type through
    `protocolHooks.count` and `protocolHooks.has`, installed with
    `installMethods`, instead of `hostHooks`.
- **Effects in `when:` lists.** A `when:` list may name an effect unit
  (`protocolHooks.satisfies effect: installMethods when: ..., makeMap.equiv,
  ...`), kept only where that effect is. Before, a hook read in the body of
  an effect waited for the effect's own `when:` list (`structuralEqualPair`),
  so every program comparing values and having a program type kept
  `satisfies`, although only maps, sets and seqs read it there.
- **Records are maps, but not here.** A record is in the `maps` store.
  `makeMap.equiv` and `makeMap.lookup` no longer target `makeType`: a record
  reaches structural equality only after `equalPair` and `objectEquiv` found
  no `IEquiv` method, and `lookup` only after `get` found no `-lookup`, while
  `recordMethods.equiv` and `recordMethods.lookup` wait for exactly those
  operations. The `runtime_manifest` suite checks this
  (`kind_hooks_skip_records`, and `Kind::Maps` in `KIND_HOOKS`). A map's
  host-facing methods (`ownMap`) treat a record as a receiver of another
  type.
- **Protocol roots.** Structural equality of a vector with a value of a
  program type implementing `ISequential` needs the generic seq comparison
  (`cons.equiv`), which targeted `installMethods`. It now targets the
  protocol's unit `ISequential`, besides the runtime's sequential kinds,
  `Eduction`, `extendType` and `extendInstance`. Emission keeps a unit of
  `PROTOCOL_ROOTS` (`src/runtime_slice.rs`) where a `deftype`, `defrecord`,
  `reify` or extension implements the protocol, as `recordMethods` is kept
  for a record. The `runtime_manifest` suite checks that the list is exactly
  the core protocols conditional effects target and that every runtime unit
  installing one's marker is a target too.
- **Tests.**
  - `builtin_types`: a record with a vector and with a keyword keep none of
    the extension machinery, seqs, `satisfies` or the map hooks, and stay
    under 12 KB; a vector's host-facing methods over records and a `deftype`
    implementing `IEquiv`; keywords invoked on records by the program, the
    host and `map`; protocols declared without a method (and `IMeta` of a
    `reify`); `ISequential`, `IMap` and `ISet` implementations compared with
    vectors, maps and sets, through `=` and `.equiv`; a `specify!` of a
    vector; a host object holding a slot-named function, a function's
    `IMeta` slot and a patched dispatcher table; all at every level against
    `None`. A record borrowed into a map's `keys` throws at `Advanced` or
    answers as at `None`.
  - `runtime_manifest`: the split hooks' `when:` lists, effects named in
    `when:`, records skipping the map hooks and `PROTOCOL_ROOTS`.
  - The ratchet ceiling of `closed-params` drops from 36,000 to 13,000.
- **Measured.** At `Advanced`, raw / gzip bytes, against master `5e83c97`:
  - `closed-params` drops from 35,923 / 11,755 to 12,996 / 4,960 (64%;
    light-cljs-243 asked for 25%).
  - `(defrecord Pt [x y])` with `(nth [1 2 3] ...)` in a loop from 35,426 /
    11,507 to 10,242 / 4,030 (target 12,000), a `deftype` instead from 33,285
    / 10,888 to 9,745 / 3,847, and `(defrecord Pt [x y]) (js/console.log (:x
    (->Pt 1 0)) (.toString :a))` from 35,274 / 11,387 to 9,895 / 3,751.
  - `(deftype T [] ICounted (-count [_] 7)) (count (T.))` drops from 33,027 /
    10,728 to 27,180 / 8,726: `count` still tests `implementsProtocol`.
  - No fixture grows. Those with vectors, maps or sets lose up to 23 bytes
    (four of them gain 3 to 8 gzipped bytes).
  - The shared runtime grows by 2,694 bytes (0.7%; 848 gzipped): `ownMethod`,
    the split hook installs and their comments.
  Per-fixture sizes are in `benchmarks/compiler.md`.
- **Left for later.** `satisfies` and `implementsProtocol` still need the
  protocol objects and `builtinCoreImplementation` wherever a program type
  exists and an operation tests a protocol (`count`, `seq?`, ...); a test of
  a value's own marker, like `ownMethod`, would need the protocol's name at
  each call site (light-cljs-251). Protocol roots cover `ISequential` only,
  the one protocol whose implementations reach a branch of the runtime's
  kinds that `installMethods` alone kept. A record's core protocol slot that
  the program names (`"cljs$core$ILookup$_lookup$arity$3"`) was installed
  only where an operation dispatching it is kept, as before in a program
  with records alone, until light-cljs-250 (below).
- **Named record slots (light-cljs-250).** The runtime installs a record's
  core protocol method (`recordMethods.lookup`) only where an operation
  dispatching it (`get`) is kept, but a program can call the method through
  its slot. Rapid type analysis now also finds the slots of
  `RECORD_METHODS` (`src/runtime_slice.rs`: each method, the arities of its
  slots and a unit of its effect's `when:` list) that reachable code names
  as a string, property or method, or in `js*` text, as for a program
  protocol's slots; emission of a record roots each such unit with
  `NameUse::Keep`, so the method is installed. The unit is the one of the
  list that adds least, mostly one the method requires anyway (`get`,
  `count`, `hash`; `metaSupported` and `withMetaSupported` for `IMeta` and
  `IWithMeta`, `printWriterImpl`, `reduceKv`). The `runtime_manifest` suite
  checks the table against the effects (keys, arities, `when:` lists), and
  `builtin_types::named_generated_record_slots_remain_callable` calls every
  record method through a named slot at every level. No fixture changes
  size; `(defrecord R [x]) (def r (->R 1))` with `(.call (aget r
  "cljs$core$ILookup$_lookup$arity$3") r r "missing" "nf")` now returns
  `"nf"` at 9,796 / 3,698 bytes where it threw at 5,695 / 2,395, and the
  pellet's scenario with `(.equiv [1] [1])` grows from 10,099 / 3,935 to
  12,358 / 4,598.

#### Phase 2 call shape extension points

Step 5 is implemented in `src/call_shapes.rs` (light-cljs-212, and
light-cljs-228 for local and named functions), at `Advanced` only.
`prepare_application` runs it last, over the program that rapid type analysis
rewrote, and emission reads it. `None` and `Prepare` output does not change;
`specialize` still runs there and at `Advanced`, whose emission prefers the
shapes below.

- **Known functions.** `call_shapes::analyze` walks the program from its root
  with each expression's parent. A global's function is known when one `def`
  binds it to a `fn` (in one-form `do`s) and nothing else assigns the global:
  no second `def`, `defonce`, `set!`, `binding`, `with-redefs`, `js*` hole,
  `:dynamic` or `:test` metadata, or host path name. A `def` without a value
  (`declare`) assigns nothing and does not count. Macro functions stay
  generic. A named function with several methods (`(def f (fn self ...))`) is
  known only when its `def` is a direct top-level form, which runs once, and
  its name is read only as the callee of calls whose count selects a method;
  calls through the name then name the entries too. A method that reads the
  name as a value needs that evaluation's dispatcher, so it stays generic.
- **Escape.** A known function escapes when the program reads its global other
  than as a call's callee, uses the value of its `def` (the `def` is not a
  discarded form), exports it, or calls it with an argument count that selects
  no method at compile time: no method of that count and fewer arguments than
  the variadic method's fixed parameters, which the dispatcher fills with
  `undefined` or rejects. An expression with two parents counts as read.
- **Shapes** (`FunctionShape`):
  - `Single`: one fixed method, which is the function. Calls are plain,
    `g3(a, b)`, whether or not the value escapes.
  - `Entries`: each method is bound at the `def` to a top-level entry
    `g<global>_<method index>`, `(g3_0 = function () {...}, g3_1 = ...)`, and
    calls name the entry their count selects. The global is bound only when
    the function escapes: to the `arguments.length` dispatcher over the same
    entries, with its arity properties, as the last items of the same comma
    expression. The dispatcher of a direct top-level `def`, which runs
    once, reads the entries from their bindings. A `def` that may run again
    (`rerun`, since light-cljs-228) rebinds the entries, so its dispatcher
    holds the entries of its own evaluation,
    `g3 = (() => { const m0 = g3_0; ...; return f; })()`: a dispatcher saved
    from an earlier evaluation keeps calling its own methods, while direct
    calls read the current entries, which are always those of the current
    global, as calls of the global did. One that does not escape may be
    defined anywhere, since its entries are rebound together and every call
    reads them.
  - `Generic`: as below `Advanced`.

  The entry names need no new hygiene rule: `hygienic_js` renames `g` followed
  by digits and underscores, and the top-level probe of
  `optimization::raw_js_names_resolve_only_to_holes_and_host_globals` sees
  them.
- **Local functions** (`LocalFunction`, light-cljs-228). A `fn` with several
  methods or rest arguments bound by `let` or `letfn` has no dispatcher when:
  - its value goes only to that binding (through one-form `do`s, each with
    one parent);
  - its local and its name are read only as the callee of calls whose count
    selects a method (`method_for`, as for globals);
  - no `reify` captures them (a `reify` object holds every visible local as a
    field, so the analysis counts its captures as reads; this also keeps
    229's closed functions sound), and no method of a type, `reify` or
    `extend-type` captures them (its `IFn` adapter may copy its captures into
    an object).

  Calls through the local or the name name the entry of the method they
  select. Each method is placed where its captures permit (`Placement`):
  - `Hoisted`: it reads no local from outside itself, except as the callee of
    a call of another hoisted method (itself, a sibling, a `letfn` peer), and
    holds no `js*` (raw text cannot move into the constant table) and no
    expression with two parents. It is a function entry of the constant table
    under `ConstantKey::Method`, `const k4 = function (l6) {...};`, never
    lazy, declared at its first call and numbered before its body is written,
    so recursive and mutually recursive methods can call each other. It is
    written as at the top level: none of the emitter's enclosing scope state
    applies to it. The function value never escapes, so creating it once
    instead of at each evaluation is invisible.
  - `Scoped`: any other called method is a `const` of its own next to the
    binding, `const l5_1 = function (l7, l8) {...};`, created at each
    evaluation of the scope as the dispatcher's methods were, so closures
    made in a loop see their own iteration. The local itself is not bound
    (`letfn` skips its `let l5;`).
  - `Unused`: a method no call selects is not emitted.

  `LocalFunctions::place` decides this with one walk over the subtrees of
  local functions, which tracks how many methods enclose each local's binding
  (a local whose binding it does not see, such as a type field, is outside
  every method), and a backward propagation from scoped methods to the
  methods that call them from outside their binding. Calls cannot run before
  their entry is bound: they are in the binding's scope, and `letfn`
  initializers are `fn` expressions. `l<n>_<m>` names are renamed by
  `hygienic_js` (`l` followed by digits and underscores); the
  `local-multi-arity` probe of
  `optimization::raw_js_names_resolve_only_to_holes_and_host_globals` runs
  in a scoped method next to hoisted ones. `ApplicationStats::
  dispatchers_removed` counts local functions too, and `hoisted_methods`
  counts the hoisted methods; `ConstantPool::functions` counts the function
  entries the output declares (the `--size-report` line adds `F functions`).

  `loop` locals (which `recur` rebinds), functions whose local or name
  escapes, calls whose count selects no method and `fn` literals called in
  place keep the dispatcher.
- **Calls** (`DirectCall`). A variadic method receives what its dispatcher
  would pass: the fixed arguments, then `rt$fnRest([rest...], 0)` or `null`.
  A method whose body holds `js*` outside nested functions keeps
  `.call(null, ...)` on its entry, because a plain call passes `undefined` as
  `this`, which strict raw text can tell from `null`; `this-as` maps both to
  the global object. A local bound to a `fn` with one fixed method, or such a
  `fn` literal, is called plainly too when `Inference::callee` knows it and
  its type has no `nil` (since light-cljs-229: calling nil keeps
  `.call(null, ...)` and so the `TypeError` message of lower levels); a
  captured local that the emitter writes as a member read is called as
  `(0, captures3.l5)(a)`. The call reads its target when it runs, as before,
  so a call before the definition still throws a `TypeError` (see
  "Advanced-mode contract").
- **`do` in value position** is a comma expression of its forms that have an
  effect, `(g3 = (log("init"), 5))`, or its last form alone, so a `def`
  initializer is no longer a function call. It stays a function when one of
  its forms, outside nested functions, reads `arguments`, holds `js*` (which
  may read `this`), or is a `def` with `:test` metadata, whose test write the
  return position skips. A discarded `def` without a value is no statement.
- **Unknown callees** keep `.call(null, ...)`. `cargo bench --bench compiler --
  --callees` times the candidates per call in Node and QuickJS (see
  `benches/support/callees.js` and "Direct call shapes" in
  `benchmarks/compiler.md`). Upstream's `:fn-invoke-direct` shape,
  `f.cljs$core$IFn$_invoke$arity$1 ? f.cljs$core$IFn$_invoke$arity$1(a) :
  f.call(null, a)`, is four times faster in QuickJS for a callee with a
  dispatcher but 19% slower for one with a single method, the common case of
  `fn` literals. It adds about 60 bytes per call site until Phase 3 mangles
  the property, needs a callee it can read twice, and passes the function as
  the method's `this`, which `this-as` sees: the methods of
  `ifnInvokeEntry` objects need that receiver, so `.call(null, ...)` on the
  property is not an option either.
- **For parameter types.** `CallShapes::closed(global)` says when a known
  function does not escape, so its direct calls are all its calls, and
  `CallShapes::calls_of(function)` lists them with the method each runs.
  Since light-cljs-229 `closed` also requires that the `fn`'s name is read
  only as a callee and that only the `def` evaluates the `fn` expression.
  Inference joins parameter types over the same calls, found before it runs
  by `closed_functions` (see "Phase 2 parameter type extension points").
- **Statistics.** `ApplicationStats::direct_calls` counts the direct calls and
  `dispatchers_removed` the retained functions defined as entries without a
  dispatcher.
- **Tests.** The `call_shapes` suite in `tests/compiler/` runs every program at
  every level, in sloppy and strict code, and checks the `Advanced` text. It
  covers:
  - every method kind, with self and mutual recursion across `declare`;
  - `recur`, `arguments` and the parameters `recur` writes through it;
  - `this` in raw text and `this-as`;
  - `apply`, higher-order arguments, `var`, used `def` values, exports,
    invalid and underfilled arities;
  - redefinition, `set!`, `binding`, `with-redefs`, `defonce`, `js*` holes,
    and definitions inside functions;
  - calls before the definition, rest sequences and argument order;
  - `do` in value position, local functions and calls across namespaces;
  - since light-cljs-228, local functions of every method kind, `letfn`
    mutual recursion, named local functions, methods hoisted, scoped and left
    out, closures made by scoped methods in a loop, every way a local function
    escapes (higher-order arguments, `apply`, aliases, its name as a value,
    `reify`, `extend-type`, `loop`, unselected and underfilled counts, `fn`
    literals called in place), `js*`, `this-as` and `(js-arguments)` in local
    methods, named global functions, and escaping definitions that run twice.

  The ratchet fixtures `multi-arity` and `local-arities` pin the sizes, and
  `--levels` runs both as steady-state workloads.
- **Measured.** At `Advanced`, `multi-arity` drops from 3,579 to 2,040 bytes
  and `raw-js` from 856 to 271. Its steady-state loop runs 1.8 times faster in
  QuickJS and 21 times faster in Node, which now inlines it. A direct call of
  a method costs 29 ns in QuickJS where `.call(null, ...)` through a
  dispatcher cost 152. See "Direct call shapes" in `benchmarks/compiler.md`.
  With local entries (light-cljs-228) `local-arities` drops from 2,864 to
  1,282 bytes, and its steady-state loop runs 1.9 times faster in QuickJS and
  30 times faster in Node, as fast as the global functions of `multi-arity`;
  see "Local function entries" in `benchmarks/compiler.md`.
- **Left for later.**
  - A method that captures only locals whose value is the same at every
    evaluation (a constant, or a global read) stays scoped; it could be
    hoisted with the value substituted.
  - `fn` literals with several methods called in place, `((fn ([] 1) ([x]
    x)) 2)`, keep their dispatcher; they could call the method literal.
  - A named function whose name escapes, or whose `def` may run again, keeps
    the generic shape.
  - A `def` initializer that is a `let`, `try`, `case` or `loop` is still a
    function call; top-level definitions could emit it as statements.
  - Parameter types from the direct calls of closed functions
    (light-cljs-229). Done; see "Phase 2 parameter type extension points".

#### Phase 2 parameter type extension points

Parameter types are implemented (light-cljs-229) in `src/call_shapes.rs` and
`src/inference.rs`, at `Advanced` only. Before, every parameter was `any`, so
the lowerings of step 3 and the devirtualization of step 4 mostly applied to
`let` locals.

- **Closed functions.** `call_shapes::closed_functions(program, exports)`
  returns `ClosedFunctions`: the `fn` expressions whose every call the program
  makes itself, with those calls. A `fn` is closed when each place its value
  goes calls it:
  - it is the function of a known global that does not escape
    (`CallShapes::closed`, 212's escape analysis);
  - it is bound by `let` or `letfn` to a local that is read only as the
    callee of calls (not `loop`, whose `recur` rebinds it);
  - it is the callee of a call, `((fn [x] ...) 1)`.

  A named `fn` must also have its name read only as a callee, and the `fn`
  expression and the one-form `do`s around it must have one parent each,
  since an expression reached twice is evaluated twice. A call runs the
  method its argument count selects as the dispatcher does: the fixed method
  of that count, else the variadic one, even for fewer arguments than its
  fixed parameters (they are `undefined`). A count that selects no method
  throws before a method runs and binds nothing. A function with one fixed
  method runs it for every count.
- **Ordering.** `closed_functions` is the part of call shapes that needs no
  inference: known globals, escapes and the calls of globals and locals. So
  `prepare_application` runs it first, then inference with its result, then
  rapid type analysis, which reads the parameter types to devirtualize, and
  last the full `call_shapes::analyze` over the rewritten program, which
  reads inference for the calls of locals. Rapid type analysis only removes
  code, so the calls found before it include every call emission keeps: the
  joins stay sound, and a function that only those removals close keeps
  `any` parameters. No second inference round is needed, and the program
  rewrites of 211 keep their order. `Prepare` output does not change.
- **Joins.** In inference's fixed point, each call of a closed function binds
  the parameters of the method it runs to its arguments' values, after
  narrowing, as a `let` binds its locals: `nil` for missing arguments, and for
  the rest parameter `seq` when the call has arguments beyond the fixed
  parameters and `nil` otherwise, as `fnRest` builds it. A method's `recur`
  binds its parameters through the recur target, the rest parameter included,
  as for `loop`. The parameters start at `never`, so a closed function that
  nothing calls has a body typed `never`, which no emitted code runs.
  Arguments may read other parameters, also through recursion: every value
  only grows, so the joins converge with the rest. `Known` representations
  pass through (`Function`, `Instance`, `Vector`, `Array`): a parameter that
  every call passes the same `fn` literal is called as that `fn`'s method.
- **Assignable parameters** stay `any`, as locals do: a `js*` hole, a `set!`
  target, and the parameters of a method that reads `(js-arguments)`.
- **Convergence.** A parameter type flows backward, from a call to the
  definition it calls, which usually comes earlier. Rounds therefore
  alternate: odd rounds walk the top-level forms in order, even ones in
  reverse, and a chain of 100 top-level definitions converges in at most
  four rounds. Inside one form each link of a chain still takes a round. If
  the fixed point does not converge in `ROUND_LIMIT` rounds, inference
  starts over without parameter types (`ClosedFunctions::default()`), so a
  long `letfn` chain costs its parameter types, not the program's.
- **Calling nil.** A direct call of a local or parameter whose type includes
  `nil` keeps `.call(null, ...)`. Calling nil then throws the `TypeError`
  lower levels throw, `Cannot read properties of null (reading 'call')`,
  instead of `l4 is not a function`. This also holds for `let` locals, which
  212 called plainly.
- **Statistics.** `ApplicationStats::typed_parameters` counts the parameters
  of closed functions whose type is narrower than `any`.
- **Tests.** The `type_inference` suite in `tests/compiler/` checks:
  - the joined types: missing and rest arguments, multi-arity methods,
    recursion, `recur`, mutual recursion across `declare`, local, named,
    `letfn` and in-place functions, and representations;
  - that every other caller keeps `any`: `apply`, the function as a value
    (its global, its `def`, its name, a local alias), `loop` locals, `js*`
    holes, `(js-arguments)`, underfilled variadic and unselected arities, and
    `with-redefs`;
  - every lowering on parameters, at every level, with adversarial values
    (`-0`, `NaN`, emoji strings, built keywords, subvectors, map entries,
    records), and callers that turn them off;
  - devirtualization through a parameter, and a parameter that may be nil;
  - convergence along a chain of 100 definitions, and the fallback for a
    `letfn` chain past the round limit;
  - a seeded generator of closed functions (single, multi-arity, variadic and
    local) called with adversarial arguments from nine classes, some through
    `apply`, `mapv` and `fn` literals, whose output must equal `None`'s at
    every level (`LIGHT_CLJS_INFERENCE_SEEDS=<n>` runs more than 16 seeds);
  - that the generated namespace programs of `generated_optimization` type
    parameters.
- **Measured.** The ratchet fixture and `--levels` workload
  `closed-params` reads record fields, a vector, a string and a keyword
  through parameters. At `Advanced` it shrinks from 79,842 to 79,800 bytes,
  and its steady-state loop takes 30% less time in Node and 56% less in
  QuickJS. The other fixtures and workloads compile to the same bytes with
  and without parameter types. See "Parameter types" in
  `benchmarks/compiler.md`.
- **Left for later.**
  - Parameters of functions that escape stay `any`, except that
    light-cljs-243 joins the calls core operations make of the functions
    they are given, and light-cljs-253 the calls of protocol methods into
    their implementations (next two sections).
  - Return types do not depend on the call: a function called with a string
    and with a vector returns the join for every call (no context
    sensitivity).

#### Phase 2 callback parameter extension points

Callback parameter types are implemented (light-cljs-243) in
`src/core_operations.tsv`, `src/call_shapes.rs` and `src/inference.rs`, at
`Advanced` only. Before, a function passed to `map`, `reduce` or `swap!`
escaped, so its parameters were `any`.

- **Callback shapes.** In the table's `params` column a `fn` parameter may
  carry the argument lists of every call the operation makes of it, one
  parenthesized list per call: `map` is `[fn]:fn [fn(e1+) seqable & seqable]:seq|list`,
  `reduce` `[fn()(e1|r0,e1) seqable] [fn(a1|r0,e2) any seqable]` and
  `swap!` `[object fn(any,a2+) & any]`. Each argument is a descriptor:
  types, `aN` (argument `N`), `eN` (an element of it), `rN` (what function
  argument `N` returns) and `aN+`/`eN+` (from argument `N` on), joined with
  `|`. Thirty-two rows have shapes: `map`, `mapv`, `mapcat`, `filter`,
  `filterv`, `remove`, `keep`, `keep-indexed`, `map-indexed`, `take-while`,
  `drop-while`, `partition-by`, `every?`, `not-every?`, `not-any?`, `some`,
  `run!`, `group-by`, `sort`, `sort-by` (its key function), `reduce`,
  `reductions`, `reduce-kv`, `iterate`, `repeatedly`, `update`, `update-in`,
  `merge-with`, `update-vals`, `update-keys`, `swap!` and `swap-vals!`. A
  function argument without a shape escapes: transducer arities, the
  comparator of `sort-by`, `apply`, `comp`, `partial` and every other row.
- **Arity returns.** `[fn]:fn` narrows the result of one arity, so `(filter
  p xs)` is `seq|list` rather than `fn|seq|list` and stays within `reduce`'s
  domain. `Operation::returns_for(count)` gives it; the row's `returns` is
  the union, so results stay monotone.
- **Elements.** `Inferred::elements` is the type and representation of the
  elements of a value's `seq`, `list`, `vector` and `set` values, which are
  immutable: `never` without those classes, `any` when unknown.
  `Inferred::element()` is what a sequence over a value yields: those
  elements, a map's entries (vectors), a string's characters, nothing for
  `nil` and `any` for every other class. Collection literals set them, they
  pass through bindings, parameters and joins, and a core call within its
  domain sets them from the `returns` column's descriptor in angle brackets
  (`filter` `<e1>`, `map` `<r0>`, `range` `<number>`, `conj` `<e0|a1+>`,
  `vector`, `list`, `hash-set`, `vec`, `set`, `seq`, `rest`, `next`,
  `reverse`, `cons`, `concat`, `take`, `drop`, `take-while`, `drop-while`,
  `keep`, `keep-indexed`, `map-indexed`, `mapv`, `filterv`, `iterate`).
  `(map vector xs ys)` holds vectors, `(map inc xs)` numbers: `rN` calls
  the function argument with the shape's arguments.
- **Closure.** `closed_functions` treats a function argument with a shape as
  a visible use: a global (which still escapes for emission and keeps its
  value and dispatcher), a `let` local, or the `fn` itself. The calls go to
  `ClosedFunctions::callbacks(call)`.
- **Joins.** At each such call inference binds the function's parameters as
  each of the operation's calls would, selecting the method by count, when
  the table describes the call: within the arity's domain, the operation not
  replaced, core protocols intact (also for a lazy operation such as `map`,
  whose `calls` is `-`, since traversing its collection runs them), and no
  other argument that may be a
  program type while one implements `IReduce`, `IKVReduce` or `ISwap`, whose
  method receives the function itself (`ProgramFacts::receives_functions`).
  Only `swap!` and `swap-vals!` admit program types in their domain. Any
  other call binds every parameter to `any`. `rN` inside the arguments is the
  function's current result, so a `reduce` accumulator is the initial value
  joined with what the function returns.
- **Tests.** The `type_inference` suite checks the joined types (literals,
  `range`, strings, maps, `conj`, `map` results, records), the cases that
  stay `any` (transducers, comparators, `apply`, a JavaScript array, an
  `IReduce` or `ISwap` program type, a replaced `mapv`, `ISeqable` extended
  to vectors, which lazy `map` reaches only when traversed), the return
  classes `reduce`, `reductions` and `iterate` feed back into their
  function's parameters, lowerings inside callbacks on
  adversarial values against `None`, including an `ISwap` type that passes
  `""` where the shape says `false`, and every annotated operation with
  callbacks whose truthiness tests `Advanced` lowers, so a shape that puts a
  value where the runtime passes another prints differently. The
  `core_operations` suite checks each arity's return type on samples.
- **Measured.** No existing ratchet fixture changes at any level;
  `closed-params` stays 12,996 bytes, since its functions are closed already
  (light-cljs-229). It was 37,763 bytes when this work was planned; the
  runtime a record type and a vector or keyword kept together went with
  light-cljs-246. The new fixture `callbacks` shrinks from 42,345 to 42,261
  bytes: its callbacks read record fields, so no `:qty`, `:price` or
  `:label` keyword is built. A typed lowering is not smaller by itself where
  the runtime keeps the operation for other calls: `(.-length s)` is longer
  than a renamed `count` call. See "Callback parameter types" in
  `benchmarks/compiler.md`.
- **Left for later.**
  - Specialized clones for a dominant call site were not added: lowerings
    only shrink output when they remove a runtime root, so a clone, which
    copies a body, grows every fixture measured here.
  - The atom's value is `any` in `swap!`'s shape: inference does not track
    what atoms hold.
  - `nth`, `first` and `get` on a collection return `any`, not its element
    type, so destructuring a vector parameter does not type the locals.
  - Parameters of protocol and `deftype` methods stayed `any` until
    light-cljs-253 (next section).

#### Phase 2 protocol method parameter extension points

Protocol method parameter types are implemented (light-cljs-253) in
`src/call_shapes.rs` and `src/inference.rs`, at `Advanced` only. Before, the
methods of `deftype`, `defrecord`, `reify` and `extend-type` had `any`
parameters, so `(defprotocol P (m [this flag])) (deftype T [] P (m [_ flag]
(if flag 1 0))) (m (T.) true)` kept `truth()` although every call passes a
boolean.

- **Closed protocol methods.** `closed_functions` also finds the program
  protocol methods whose dispatcher runs only for calls the program makes
  (see "Protocol methods" in `src/call_shapes.rs`): the method is declared
  once, its global is read only as the callee of calls and not exported, its
  signature is not variadic, no other method's name munges to the same slots
  (`m-foo` and `m_foo`), and no property, method, JavaScript global path
  (`js/probe.cljs$user$P$m$arity$2`), string or `js*` text holds one of its
  slots (`ns$P$m$arity$`) anywhere in it, whatever characters surround it:
  such a slot could be read and called on any instance. Inference also
  requires that the program defines it once and never assigns it
  (`Definition::Protocol`, so `with-redefs` and `set!` count).
- **Implementations.** The methods without rest parameters that implement
  such a method in a `deftype`, `defrecord`, `reify`, `specify!` or
  `extend-type` of a program type (`ClosedFunctions::implementations`). An
  `extend-type` or `specify!` whose value is used returns its last method, so
  its methods are left out, unless `specify!` returns its target. An
  `extend-type` of anything but a program type is left out too: the
  dispatcher's own table reaches one of a host type name (`string`,
  `default`) by the receiver's host type and may run its only method for any
  arity, and a host constructor's prototype is inherited by values whose
  class inference does not track.
- **Dispatch.** Each call runs the arity the dispatcher selects: the
  method's one arity, padding missing arguments with `undefined` and
  dropping extra ones, else the call's argument count when it is one of the
  arities (otherwise the call throws). `ClosedFunctions::dispatch(call)`
  lists the implementations with that many parameters, the receiver included.
- **Joins.** In inference's fixed point each such call binds the parameters
  of the implementations its receiver may run to its arguments, `nil` for
  missing ones. A method's receiver parameter is `this`, which the
  dispatcher and devirtualized calls set to the first argument. A `deftype`
  or `defrecord` method, or an `extend-type` method of a program type, runs
  when the receiver may be an instance of that type: its type has the
  type's class (`object` or `record`) and it is not known to be an instance
  of another program type (`Known::Instance`). A `reify` method runs when the
  receiver may be an `object` not known to be any program type's instance,
  and a `specify!` method for every receiver, since its target may be any
  object. A receiver that may be a host object reaches every method, since
  a host object may inherit any object's slots (`#js {"__proto__" t}`, or
  `aset` of a computed `"__proto__"`), and so does every receiver of a
  program with protocols that names a prototype in a text as above
  (`prototype`, `__proto__`, `setPrototypeOf`: `ClosedFunctions::inherits`),
  which may move an instance of one type onto another's slots. Receivers
  only grow during the fixed point, so this stays monotone. A receiver typed
  as one program type therefore types only that type's methods, and
  `(m (if c (T.) (U.)) x)` joins `x` into both.
- **Tests.** `protocol_method_parameters_join_visible_calls` in the
  `type_inference` suite checks the failure scenario, the receiver, mixed
  callers of two types, a receiver of either type, missing arguments,
  several arities, `reify`, `extend-type`, `specify!` and calls inside
  methods, and that every other way to reach a method keeps `any`: `apply`,
  `mapv`, a `def` of the method, its slot named as a method, a string or in
  `js*`, `with-redefs`, a variadic signature, an `extend-type` whose value is
  used or that extends `string`, a core protocol method and `(js-arguments)`.
  `typed_lowerings_apply_inside_protocol_methods_and_keep_the_results` runs
  lowered truth tests and record field reads through the receiver at every
  level, and adversarial programs (a receiver of either type, `specify!` on
  an instance, a called `extend-type` value, calls through slot names,
  `apply` and `mapv` of the method) whose output must equal `None`'s.
  `protocol_method_parameters_keep_hidden_callers` (from the review of this
  change) runs host objects inheriting a type's and a `reify`'s slots, an
  instance moved onto another type's prototype, two methods whose names
  munge alike, a slot called through a JavaScript global path and slots
  named with an emoji and a combining mark, against `None`, and checks that
  exporting the method keeps its parameters `any`.
- **Measured.** No existing ratchet fixture changes at any level. The new
  fixture `protocol-params`, two types pricing records through a protocol,
  shrinks from 47,413 to 47,267 bytes at `Advanced` (14,863 to 14,820
  gzipped): the methods read the record's fields, so no `:qty`, `:price` or
  `:label` keyword is built. See "Protocol method parameter types" in
  `benchmarks/compiler.md`.
- **Left for later.**
  - Protocol method calls return `any`; the union of the bodies of the
    implementations a call may run would type them.
  - An implementation that no call reaches, but that rapid type analysis
    keeps because its method is called on another type, has `never`
    parameters: a keyword lookup on one keeps the generic operation, which
    still builds the keyword, although the body never runs.
  - A protocol method passed to a core operation (`(map m xs)`) escapes;
    callback shapes could join its calls as they do for `fn`s.

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

#### Phase 2 folding and inlining extension points

Step 8 is implemented (light-cljs-214) in `src/folding.rs`,
`src/inlining.rs` and `src/defines.rs`, at `Advanced` only, except that
build-time defines apply at every level. `prepare_application` runs it after
the first definition removal and before rapid type analysis, type inference
and call shapes, which therefore see the simplified program. `None` and
`Prepare` output does not change.

- **Rounds.** Each round folds, removes the top-level definitions nothing
  reads any more (the `Inert` removal, which now runs as `retain` once per
  round), computes call shapes without types (`call_shapes::analyze` takes
  an optional `Inference`) and inlines. A round in which nothing folds or
  inlines ends them, and so does `SIMPLIFY_ROUNDS` (4); the last inlining
  round is always followed by folding and removal. Folding removes dead
  branches and the reads in them, inlining the calls of single-use and small
  pure functions, and removal then drops the definitions that only those
  read.
- **Folding** (`folding::fold`) rewrites expressions in place, children
  first:
  - a read of a `^:const` or `goog-define` global is its value when one
    top-level `def` defines it with a literal (a `declare` does not count),
    nothing assigns or redefines it, and the read is in a later top-level
    form, which runs after the definition. Earlier forms, including the
    functions they define, keep the read, which may see the global unset.
    Upstream likewise substitutes a `^:const` Var's value at the references
    it analyzes after the definition;
  - a `let` binding of a literal is substituted and dropped, and so is an
    unread binding whose initializer has no effect (literals, local and
    global reads, `fn`, and vector, list, map and set literals of those whose
    keys and members are scalars). This folds `and`, `or`, `when-let` and
    `if-let` over constants, whose expansions bind their operands;
  - a call of a core operation on literals is its result when the table
    (`src/core_operations.tsv`) makes the call `pure` for those argument
    types and `ProgramFacts::trusted` holds (the program does not replace the
    operation, and a `dispatch` row needs core protocols intact). `evaluate`
    reproduces `+ - * /`, `inc`, `dec`, `abs`, `max`, `min`, `mod`, `quot`,
    `rem`, the comparisons, `==`, `=`, `not=`, `identical?` on scalars,
    `not`, `boolean`, the type and number predicates and `str` with the
    runtime's double arithmetic in the runtime's order; `str` prints numbers
    as ECMAScript's `Number::toString` does (`folding::js_number`).
    Leading number literals of `+ - * /` fold into one, since the runtime
    folds its operands from the left: `(+ 1 2 x)` is `(+ 3 x)`. A call outside
    the operation's domain (`(+ 1 "a")`), a keyword `str` and an `identical?`
    of keywords keep their call;
  - an `if` on a literal is the branch it selects, a `^boolean` hint on a
    literal is the literal, and a `do` drops the forms that have no effect
    before its last, and its last too where its value is discarded (a
    discarded `do`, `let` body or `if` branch). A `do` or `let` left with
    one form is that form, unless its contents read `arguments`, hold `js*`
    or a `:test` definition: in value position emission writes the form as
    a function call, which those see (the emitter's `sequence_keeps_scope`
    asks the same).

  Locals that a closure, `reify`, type method or `js*` hole names are never
  substituted, since the emitter names captured locals and raw text may
  assign its holes. The root, namespace entries and macro functions keep
  their expression identities; `case` tests, the targets of `set!` and
  `binding`, and the globals of `var`, `exists?` and `resolve` are not reads
  and stay.
- **Literals.** Map keys and set members never become constants
  (`folding::fenced_positions`): the emitter builds a map whose keys are all
  constants from a key shape and a set of scalars without the duplicate check
  that members computed at run time get, both of which rely on the distinct
  keys analysis checks in source literals, so `(let [a 1 b 1] #{a b})` must
  still throw. Every other item may fold, and so a literal the program builds
  at each evaluation, such as `[x 1]` once `x` is a constant, could become
  one the pool declares once and every evaluation shares, which no level
  below shows. `folding::built_literals` marks the literals that are built
  (collection literals and `with-meta` with an item that is not a constant),
  folding and inlining carry the mark where an expression takes a literal's
  place or copies it, and after the last round `folding::keep_built` wraps
  one item of each marked literal whose items are all constants in a one-form
  `do` (a map's first value, so that its keys keep their shape). Emission
  writes the `do` as the item and does not pool the literal.
- **Inlining** (`inlining::inline`) replaces a call that `CallShapes::call`
  resolves to a method of a known global function with that method's body
  when the global's sole definition is a direct top-level `def` and the call
  is in a later top-level form or inside the function itself (both run after
  the definition), the argument count matches the fixed parameters (a
  variadic method may receive more when it never reads its rest parameter),
  the method captures nothing and is no `recur` target, the call has one
  parent, and the global has no `^boolean` tag, whose calls the advanced-mode
  contract trusts. The body may hold literals, reads, `if`, `do`, `let`,
  `^boolean` hints, calls, collection literals, property reads, method calls,
  `new`, `instance?` and metadata; a nested `fn`, `loop`, `letfn`, `try`,
  `throw`, `case`, assertion, assignment, definition, `js*`, `js-arguments`
  or `this-as` keeps the function. Upstream's cost model is
  `FunctionInjector.java:910-1080` in the research clone, which this
  follows:
  - a *single-use* function, closed (`CallShapes::closed`) with this one call,
    is inlined whatever its size up to `SINGLE_USE_LIMIT` (4,096
    expressions): its definition goes, so the body moves rather than grows;
  - otherwise the body must be *pure* (only reads, branches, bindings,
    literal collections, keyword lookups and calls of core operations whose
    row is `pure` or `allocates` and calls no function argument, so it can
    run only the code its arguments supply), at most `INLINE_BUDGET` (10)
    expressions, a pooled constant collection counting one, and must lower
    the cost (`lowers_cost`): a call costs two expressions plus its arguments
    and a definition its body plus three plus its parameters. An escaping
    function keeps its definition, so its body may cost no more than a call;
    a closed one with `n` calls may cost `n` times more in total than the
    calls by up to its definition. The walk that measures a body stops as
    soon as it exceeds its limit (rule 4 of docs/dos-and-donts.md), and the
    copies of pure bodies spend `growth_budget` (half the program's
    expressions plus 64) over all rounds.

  Calls are visited children first in source order, so a function's own
  calls are inlined before the function is copied elsewhere.
  `ApplicationOptions::no_inlining` turns inlining off (folding still runs),
  as Closure's `inlineFunctions` option does, for debugging and profiling
  with every stack frame; the `type_inference` suite sets it to keep the
  functions whose parameter types (light-cljs-229) it checks. Since inlining
  runs before inference, a function inlined into a closed one gets the
  parameter types of the arguments it was called with there.
- **Arguments.** A parameter read in place of a literal, core value,
  immutable local or pooled constant collection argument copies the
  argument at each read; an unread parameter drops an argument that has no
  effect; a stable argument (no effect, no global or mutable field read) moves
  to its parameter's only read; and one argument with an effect moves to its
  parameter's only read when the body reaches that read before anything with
  an effect or a throw, unconditionally (`first_read`; Closure's direct
  inlining). Any other argument is bound to a fresh local by a `let` around
  the body, in order. A `let` in value position would be a function call, so
  a call that needs one, or whose body has a `let` where it produces its
  value, is inlined only in statement or return position. The body's own
  locals get fresh identities.
- **Defines** (`src/defines.rs`). `(goog-define name default)` analyzes as a
  `def` of a `^:const` Var (no redefinition, `set!` or `binding`) marked
  `GlobalBinding::define`, whose default must be a string, number or boolean
  literal, as upstream checks. `ApplicationOptions::defines` (`Define { name,
  value }`, the qualified name and a `DefineValue`) replaces the default
  literal before anything else, at every level, as upstream's
  `:closure-defines` do at `:none` too; a define that names no `goog-define`
  or has another kind is ignored and listed by
  `PreparedApplication::rejected_defines`. `--define NS/NAME=VALUE` (repeatable,
  after `--compile`, for text and `--entry` builds) reads the value as the
  default's kind with `Define::parse` and rejects what does not fit. At
  `Advanced` folding then substitutes the value, so `(when DEBUG ...)` folds
  away with everything only it reads.
- **Statistics.** `ApplicationStats::inlined_calls` and `folded_expressions`
  count the rewrites; `removed_definitions` includes what they leave unread.
- **Tests.** The `folding_inlining` suite in `tests/compiler/` runs every
  program at every level, sloppy and strict, and checks the `Advanced` text:
  folded edge cases (`NaN`, `-0`, infinities, ECMAScript number printing,
  `mod`/`quot`/`rem` signs, `=` on keywords and `nil`), calls that must stay
  (outside the domain, replaced operations, `=` with core protocols extended
  to numbers), dead branches and the definitions only they read, scopes that
  `arguments` and raw text see, `^:const` reads before and after the
  definition, defines at every level including rejected ones and
  `goog-define` errors, single-use and small pure functions, argument order
  and effects, bodies and calls that must stay functions, mutable fields,
  rest parameters and recursion. The CLI suite checks `--define` for text and
  namespace builds and its errors. The ratchet fixture `debug-logging` pins
  a `goog-define`-gated logging path that leaves nothing at `Advanced`: the
  suite checks that the output is byte for byte that of the program written
  without it.
- **Folding against pooled constants** is implemented (light-cljs-221): see
  "Constant folding extension points" below. The inliner still takes no
  body with a `case` (`inlining::measure` rejects it), so `(kind :a)` of
  `keyword-case` stays a call until the whitelist admits `case` with its
  `Copier` arm; folding `case` on a constant would then remove it. New folds
  must keep the two rules of "Literals" above: map keys and set members do
  not become constants, and built literals stay marked.
- **Left for later.**
  - Functions with nested `fn`s, `case`, `throw` or `try` in their bodies,
    local functions and calls through locals are not inlined; copying
    closures needs their captures remapped.
  - Tests that inference types but that are not literals, such as `(if
    (string? s) ...)` on a known string, do not fold yet.
  - `str` of constants among other arguments is not merged, and only one
    argument with an effect may move into a body.

**Measured.** At `Advanced`, `debug-logging` is 48 bytes (`none` is
378,779), the same as the program without its logging. `multi-arity` drops
from 2,040 to 781 bytes and `raw-js` from 271 to 36, `console-log-sum` from
192 to 186; see "Folding, inlining and defines" in `benchmarks/compiler.md`.

### Constant collection literals

Tracked as Pellets 218-221 after keyword interning (208), whose constant
table they extend (see "Phase 1 constant table extension points"). Tiers 1
and 2 are implemented (218, below), tier 3 (219), tier 4 (220) and tier 5
(221, "Constant folding extension points"). Before
them every literal was rebuilt per evaluation: `[1 2 3]` copies, freezes and
wraps a fresh array; `{:a 1}` allocates the map each time (its keyword is
interned since 208); `[]` allocated although the runtime has `EMPTY`
singletons.
Below `Advanced` non-empty literals still are. Before 219 maps, lists,
records and positional constructions cached their hash, but vectors and
sets did not, so a constant vector used as a key was rehashed on every
lookup. Sharing is safe because payload arrays are frozen, `withMeta`
returns a new or unchanged object, and transients copy.

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
   Sets whose first members are constants add the others to a pooled
   shape of those (a constant core plus `conj` would reorder members).
   Vector prefixes are out of scope while vectors are flat arrays.
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

Tiers 1 and 2 are implemented (light-cljs-218), tier 3 (light-cljs-219) and
tier 4 (light-cljs-220); tier 5 (light-cljs-221) has a section of its own,
"Constant folding extension points".

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
- **Hash caching (tier 3, light-cljs-219).** At every level the runtime's
  `hash` keeps the hash of a vector, set or queue in `constructorHashes`, as
  it already did for maps, lists, keywords and symbols (records keep theirs
  in `__hash`). The values are immutable: `withMeta` copies the hash,
  metadata does not take part, and updates and `persistent!` make new
  values. Sequence views are not cached, because `(seq #js [...])` reads a
  host array. A collection holding a value whose hash can change (a `Date`,
  a mutable `deftype`) keeps its first hash, as upstream's `__hash` does.
- **Precomputed hashes (tier 3).** `src/value_hash.rs` ports the runtime's
  hashing bit for bit (Murmur3 mixing, string, number, keyword and symbol
  hashes, ordered and unordered collections, map entries) and
  `ConstantHashes` applies it to the pool: parts precede their entries, so
  each composite's hash is computed from its parts' hashes as it is
  declared. Its initializer stores it, `rt$hashed(rt$vectorFromArray([1,
  k0]), -1713415597)`, where the runtime's `hashed` seeds the cache, so the
  first hash of a pooled constant is a lookup too. `withMeta` entries inherit
  their value's hash, and `#uuid` entries cache their own on first use. Map
  and set literals merge equal keys as their builders do (first key, last
  value); when the pool cannot tell whether two keys are `=` (NaN, keys that
  are maps or sets, unequal keywords with equal spellings) the entry and the
  entries containing it get no hash. Precomputation needs the program to
  extend core protocols only to its own top-level types
  (`ProgramFacts::core_protocols_intact`): `hash` dispatches `IHash` and walks
  collections with `seq`, so an extension to a host or runtime type could
  change what it computes.
- **Key shapes (tier 4, light-cljs-220).** At `Advanced`, when the table has
  `hashes` (core protocols intact), `Emitter::shaped` builds a `Map` literal
  whose keys are all constants and whose values are not (a map of constants
  is pooled whole) from a key shape, `ConstantKey::Shape { set: false, keys }`
  declared from the map site in `value_expr`: `const k3 =
  rt$mapShape([k0, k1, k2], [1174270348, 2099068185, -1757859776]);`, the
  keys' parts and their hashes from `ConstantHashes::part`. The site passes
  the values, `rt$mapFromShape(k3, [k4, l0, l1])`, and each evaluation only
  makes the frozen entries `{key, value, hash}` and the map: no key is looked
  up, compared or hashed. `ConstantHashes::distinct` must prove the keys
  pairwise unequal; keys it cannot tell apart (`{:a 1}` and `{:a 1.0}`, map or
  set keys with items, NaN, keywords spelled alike) keep `mapFromArray` and its
  merging, and reader-distinct numbers that are `=` (`1`, `1.0`) already make a
  positional `LiteralMap`, whose `constantMapFromArray` neither checks nor
  hashes keys and is left alone. Past 8 entries, where sites call
  `mapFromArrays`, the shape is `rt$hashMapShape`: it runs the builder's trie
  insertion (`hashNodeAssoc`, with `effects` off, which distinct keys make
  equivalent) once, under one edit token, over placeholder entries that hold
  their index, and `rt$hashMapFromShape` copies that trie with each
  evaluation's entries. Trie nodes are never mutated and the shape's token
  never reaches a transient, so the map has exactly the entries, root, nodes,
  capacities and collision order `mapFromArray` gives it, which later
  operations (transient removal reorders collision nodes) can observe. The
  only difference is that an array map's entries hold their hash from the
  start, where `putEntry` computes it on the first read. Values are evaluated
  once each in the operand order the reader gave the literal, as before. A
  shape is a lazy cell when one of its keys is.
- **Set shapes.** A `Set` literal whose first members in reader order are
  scalar, keyword or symbol constants that `distinct` proves unequal, and whose
  other members are not all such constants, reads `ConstantKey::Shape { set:
  true, .. }`, `rt$setShape([first members])`, which holds the entries
  `literalSetFromArray` makes for those members and its edit token. The site
  `rt$setFromShape(k<n>, [other members])` copies the entries and adds the
  other members as `literalSetFromArray` would have, under the same token, so
  each is checked against every member before it and a duplicate throws the
  same error naming the same member. The reader orders set members by hash,
  so a literal whose first member is not a constant keeps
  `literalSetFromArray`: adding the dynamic members to a set of all the
  constant ones would reorder an array set and could name the other member of
  a duplicate pair.
- **Folding (light-cljs-221).** The folder classifies the collections it
  folds against with `constant_nodes`; see "Constant folding extension
  points".
- **Function entries** (light-cljs-228). Methods of local functions that
  capture nothing are entries under `ConstantKey::Method { function, method }`
  (see "Local functions" in "Phase 2 call shape extension points"), never
  lazy. `Emitter::hoisted_method` numbers such an entry before writing its
  body, unlike `constant_entry`, so the body can call its own entry; the
  body references globals and later entries only when it runs, after the
  output has loaded. A program that reads no runtime has no runtime slice;
  its table, then only function entries, directly follows the prelude.
  `ConstantPool::functions` counts them.
- **Hash tests.** `tests/compiler/constant_hashes.rs` rewrites the output's
  `hashed` to compare each embedded hash with the runtime's own, in Node and
  in QuickJS, for edge cases (`-0`, NaN, large and non-safe integers, non-BMP
  strings and lone surrogates, namespaced keywords and symbols, nested empty
  collections, reader-distinct `1`/`1.0` keys, merged keys, metadata, a lazy
  cell), seeded random scalars, every constant of the language and execution
  corpora and the generated programs; it also checks that cached hashes do
  not go stale and that `=` values hash alike, pooled or built. The
  generator's literal reads compare a literal with a built one in both
  renderings and throw if they are `=` but hash apart. `value_hash`'s unit
  tests pin runtime values. Since 220 the check also rewrites `mapShape` and
  `hashMapShape` to compare each key hash a shape embeds.
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
- **Key shape tests.** `tests/compiler/key_shapes.rs` runs shaped maps of
  both kinds with adversarial keys (nil, keys that hash alike in full or in
  their low bits, keys spelled alike, collection keys with metadata, a trie
  node that becomes an array node) and values (nil, undefined, one local at
  several keys) through printing, order, hashes, lookups, updates and
  transients at every level in Node and QuickJS, and compares array maps with
  maps built at their site in the program; likewise sets with duplicate
  members. It checks what stays at its site, compares the runtime's private
  entries and trie nodes of 600 random shaped maps and 600 sets with the
  builders' (exposed by a hook), and pins the allocation fixture
  `event_map_allocations_per_evaluation_are_pinned`: per-evaluation calls of
  `keyword`, the builders, `putEntry`, `putMember`, `entryIndex` and
  `hashNodeAssoc` over 1,000 evaluations of an event map, a 10-entry hash map
  and a set. The generator draws shaped literals from a stream of its own
  (`shape_read`), and the size ratchet and `--levels` have an `event-maps`
  fixture.

#### Constant folding extension points

Tier 5 is implemented (light-cljs-221) in `src/folding.rs`, with the
membership switch and entry reads in `src/emitter.rs`, at `Advanced` only.
It extends the folder of "Phase 2 folding and inlining extension points" and
runs in its rounds.

- **Value domain.** `folding::Constant` has two more variants: `Symbol`, a
  quoted symbol without metadata, and `Collection(ExprId)`, the literal of a
  constant collection. A literal is one when the emitter's `constant_nodes`
  (now `pub(crate)`) classifies it, so the folder folds against exactly what
  the pool holds: never a set literal whose construction may throw
  (`CheckedSet`), nor one whose construction reads a core operation the
  program replaces. `Folder::collection` follows a local a `let` binds,
  through chains of such locals, and a `^:const` global read in a later
  top-level form (as 214 substitutes scalars) to the literal; a local that
  may change after its binding (a type field, a `js*` hole, a `set!`
  target) is never followed. `Constant::of` still gives only scalars and
  symbols, which a `let` substitutes; a collection is never substituted,
  so a literal that is built at each evaluation is never duplicated.
- **Operations.** `folding::evaluate` has rows for `count`, `empty?`,
  `contains?`, `symbol?`, the collection predicates (`coll?`, `list?`,
  `seq?`, `seqable?`, `sequential?`, `vector?`, `map?`, `set?`,
  `associative?`, `counted?`) and `=` on symbols. `folding::select` folds
  what returns an item: `get`, `get-in`, `nth`, `first`, `second`, `last`,
  `peek`, `rest`, `next`, `seq` and `not-empty`, giving a `Folded`: a scalar,
  an item of the constant, a list of its items (the tail of a list), or one
  of the call's arguments (a default, or the collection itself: `seq` of a
  list). `Folder::invoke` treats keyword and symbol calls as `get` and calls
  of maps and sets as their lookups. The runtime's definitions are
  reproduced with their edges: `get` on a vector truncates a number index in
  range and finds nothing else; `nth` on a list counts down to an exact
  integer; `contains?` on a vector needs an integer index; `get` in a string
  is left alone except for keyword and `nil` keys; a vector called with an
  index out of range or a key that is not a number throws, and ignores its
  arguments after the first. What throws stays: `nth` out of range without
  a default, an index that is not a number, `nth` on a map or set, a call
  of a list or with a wrong arity.
- **Keys.** `folding::same_key` decides `=` between a lookup key and a key
  or member as the runtime's lookups compare them (`entryIndex` and
  `equalPair`; a hash map hashes first, and keys that are `=` here hash
  alike): scalars by value, keywords and symbols by spelling, and a scalar
  never `=` to a collection while core protocols are intact. It is
  undecided, and nothing folds, for `NaN`, zeros of both signs (equal, but
  hashed apart), keywords or symbols spelled alike with another namespace
  split, and a collection lookup key. A merging map or set (`mapFromArray`,
  `setFromArray`) finds the one key `=` to the lookup key and needs every
  other key provably unequal; a positional one (`LiteralMap`, `LiteralSet`)
  finds the first. `count` of a merging one needs its keys `distinct`: at
  most one collection key and scalars whose canonical forms (zeros of both
  signs as one, keywords and symbols as spellings) differ, decided in linear
  time.
- **Identity and metadata.** A folded item is copied to the call's place
  only when the pool holds it as an entry (`Folder::copyable`: a scalar or
  symbol, or a constant of at most `COPY_LIMIT`, 256, nodes, none of them a
  literal marked built), so the pool declares the copy as the same entry by
  hash-consing: it is `identical?` to the item and has its metadata. An
  item of a literal that is built at each evaluation is not copied. `rest`
  and `next` of a list are a list literal of its tail (see the contract),
  and `seq` of a list and `not-empty` their argument.
- **Gates and effects.** Every fold against a collection needs core
  protocols intact (`ProgramFacts::core_protocols_intact`) and, for a core
  operation, its table row for these arguments other than `effect` (`nth`
  is `throws`, which `select` rules out for the values it folds) and the
  operation trusted (`ProgramFacts::trusted`: not replaced); keyword and
  collection calls need `get` not replaced. Only constants and local reads
  are dropped. A not-found default that is not a constant is evaluated where
  it was: it is the result, or runs before another one, `(do default
  value)`.
- **Destructuring.** The expansions (`src/compiler_binding_macros.rs`, and
  the extended one in `src/compiler_destructuring.rs`) bind the value to a
  local and read it with `nth` and `get`, and a map through `(if (seq? m)
  (seq-to-map-for-destructuring m) m)`. With a constant value `seq?` folds,
  the `if` is its branch, the alias it binds (and an `:as` local) is
  substituted, since a `let` substitutes a local bound to another local
  that `Folder::collection` or `Folder::keyed` resolves, and every `nth`
  and `get` folds. `Folder::sweep` then drops the bindings nothing reads,
  counting reads from the root (expressions earlier rounds replaced stay in
  the arena but no longer count), up to `SWEEPS` (4) passes per round.
- **Maps with constant keys.** `Folder::keyed` finds the map literal that a
  local is bound to, or that the receiver is, whose keys are constants
  `distinct` proves unequal, whatever its values. Its `count`, `empty?`,
  `contains?`, predicates and the lookups of keys it lacks fold; a key's
  value is the lookup's result when it is a pooled constant, or a read of a
  local that cannot change where no local from the receiver to the map is
  captured (`Folder::uncaptured`), so the read stays in the function that
  built the map; a literal receiver whose other values have no effect is
  its value expression (as Closure folds a property read of an object
  literal). The other lookups read their entry at emission
  (`folding::entry_lookups`, `PreparedApplication::entry_lookups`):
  `rt$maps.get(m)[i].value`, since the map's builder, `mapFromArray`,
  `mapFromArrays` or a key shape (light-cljs-220), makes one entry per
  distinct key, in order. This is the keyword lookup on a shaped map of
  tier 4 as an index into its entries.
- **Membership.** An `if` in statement or return position whose test is
  `(s x)`, `(contains? s x)` or `(get s x)` on a set literal of at most 8
  keywords is a `switch` on `rt$keywordFqn(x)`, one `case` per member before
  the branch (`Emitter::membership`). An array set compares the value with
  each member by `equalPair` without hashing it, and with core protocols
  intact and no program `IEquiv` (whose instance may be `=` to a keyword) a
  value is `=` to a keyword exactly when it is one of the same spelling; a
  sequential or map value checks the keyword's kind before walking itself,
  so nothing realizes. `Folder::member_set` replaces a local or `^:const`
  global naming such a set with a copy of the literal.
  Since light-cljs-231 (`folding::members`):
  - a set of strings and numbers, which the array set's `entryIndex`
    compares with `===` (and nothing else is `=` to one), is a `switch` on
    the value;
  - in value position the test is a conditional expression over the members
    (`Emitter::membership_value`): `contains?` gives
    `(caseValue => caseValue === "a" || caseValue === "b")(rt$keywordFqn(x))`
    and a call of the set or `get` gives the member or nil, `... ? k0 :
    caseValue === "b" ? k1 : null`. The value is read in place, without the
    arrow function, where it is read once (one member) or where reading it
    again is reading it once (a local compared with strings or numbers). As
    an `if` test in value position, or wherever only its truth matters, it
    is the boolean. An empty set is `(x, false)`. `Folder::member_set` runs
    on every such call.
- **Inliner.** A literal argument that is built at each evaluation is no
  longer read in place of its parameter at each read (`inlining::builds`):
  every read of the parameter is the one value the call passed.
- **Options.** `ApplicationOptions::no_folding` turns folding off at
  `Advanced` (inlining and definition removal still run), as Closure's
  `foldConstants` option off does.
- **Tests.** `tests/compiler/constant_lookups.rs` runs every program at
  every level in Node and QuickJS and checks the `Advanced` text: folds
  with missing keys, defaults with effects, `nil`, string, symbol, number,
  boolean and namespaced keys, positional and merging collections,
  truncated, negative, fractional and infinite indexes, metadata and empty
  collections; the calls that throw or stay undecided; destructuring;
  identity, metadata and `^:const` reads before their definition; gates
  (a core protocol extended to a runtime type, a replaced operation);
  membership switches, including the cases that keep their lookup; and
  entry reads. `folding`'s unit tests pin `same_key`, `lookup`, `count` and
  `nth`'s throws. The generator reads constant literals through these
  operations from a stream of its own (`Builder::lookup_read`), with
  constant, local and `identity` keys and indexes, missing and out-of-range
  ones and defaults, and property 5 compares that with the same reads of
  built literals at every level; it compares the pools with folding off,
  since folding reads constants away. The size ratchet and `--levels` have
  a `constant-lookups` fixture.
- **Left for later.**
  - A seq over a constant vector, map or set (`seq`, `rest` of a vector of
    two or more items, `[a & more]` destructuring) is not a literal; folding
    through it needs a seq value in the domain.
  - `=`, `hash` and `identical?` of constant collections, and lookups of
    collection keys, do not fold.
  - A membership test in value position kept its lookup (light-cljs-231,
    done). A set with members of both kinds (keywords and strings), or of
    other constants (`nil`, booleans, symbols), keeps it, as does `get` with
    a default.
  - A `case` on a constant does not fold: the inliner takes no `case`
    body.

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
- String table for repeated long strings when it pays after gzip. It does
  not (light-cljs-242); see "Phase 3 terse emission extension points".

#### Phase 3 terse emission extension points

Renaming, runtime stripping and terse syntax are implemented
(light-cljs-215) in `src/js_syntax.rs`, `build.rs` and `src/terse.rs`, at
`Advanced` only. `ApplicationOptions::pseudo_names` (`--pseudo-names`) turns
all of it off and gives the readable output of Phases 1 and 2: the
compiler's names, the runtime with its comments and the emitter's layout.
Tests that read the output's text use it (`optimization::readable` in
`tests/compiler/`); the programs they run, and every differential suite,
run the terse output.

- **Front end.** `src/js_syntax.rs` holds the manifest's lexer and scope
  analysis, moved out of `src/runtime_manifest.rs`. It depends on nothing
  else in the crate, so `build.rs` includes it as well. `resolve` gives
  each identifier token the declaration it declares or reads (or none, for
  a free name), and marks shorthand properties; `Resolution::local_names`
  names the declarations below a body's top level; `TerseWriter` joins
  tokens; `redundant_semicolons` drops a `;` before `}` unless a statement
  needs it as its body; `joined_declarations` joins adjacent `let` and
  `const` statements (`const a=1,b=2;`). The writer keeps a space only
  where tokens would merge (`return a`, `a- -b`, `a/ /re/`, `1 .x`) and a
  line break only between a token that may end a statement and one that
  may start the next, or after `return`, `throw`, `break`, `continue` and
  `yield`, where automatic semicolon insertion may depend on it. Removing
  any other line break never changes the parse.
- **Runtime units.** `build.rs` writes `OUT_DIR/terse_runtime.rs` from
  `src/js_runtime.js`: the prologue and every unit body, terse, in
  manifest order (`TERSE_UNITS`). Locals get short names per scope,
  outermost first, each scope starting again at `a` and skipping the names
  of outer declarations its code reads and the ECMAScript globals, so a
  function's parameters read `a, b, c`. A top-level binding, declared or
  read, is a placeholder, `\u{1}<index>\u{2}` into `TERSE_BINDINGS`, and
  each unit lists, per binding it names, the local names visible where it
  occurs. `terse::terse_slice` names a slice's bindings by frequency, each
  the first name of Closure's sequence (`DefaultNameGenerator`: `a`-`z`,
  `A`-`Z`, `$`, then two characters) that no other binding has and no
  local visible at one of its occurrences has, so no name captures
  another. (Closure's same-length batches in order of first occurrence,
  which the program's names use, measured no gzip gain for the slice.)
  Constructors keep their names (`TERSE_KEPT`: function and class
  declarations with an uppercase initial, such as `KeywordValue`,
  `PersistentVectorValue` and `Atom`, and local ones bound to a function or
  class, such as the `Type` that `makeType` returns), because the host
  shows them (`console.log` of a keyword prints `KeywordValue {}`) and the
  printer writes them (`#object[...]`); protocol markers are renamed.
  Adjacent units join their `const` statements. `the_whole_terse_runtime_renames_without_capture`
  and `partial_slices_rename_without_capture` (unit tests of
  `src/terse.rs`) resolve the readable and the terse slice again and check
  that every identifier names the binding it named before; the
  `src/js_syntax.rs` tests pin the writer's separators.
- **Program.** `terse::terse_program` replaces the hygiene pass at
  `Advanced`. `terse::compiler_name` is the one list of the names the
  emitter binds (`l7`, `g3_1`, `k0`, `c2`, `r5_0`, `next0`, `self4`,
  `captures4`, `installed1`, `caught`, `caseValue`, `truth`, `host`,
  `rt$add`, `core$inc`, ...), which the readable output's `hygienic_js`
  prefixes too. Each gets a short name by frequency, the way Closure's
  `RenameVars` names globals (`RenameVars.java:466-498`): the most
  frequent first and, among the names of one length, in order of first
  occurrence. The names avoid every other identifier of the output (host
  globals, helper parameters, property names) and the ECMAScript globals
  the runtime reads. Then `x["name"]` with an identifier key that is no
  compiler name is `x.name`; the parentheses go around a member chain
  (`(a.b)(c)`), a single arrow parameter, an assignment's right-hand side
  (`x = (y);`), a returned or thrown value and the inner pair of `((x))`,
  each where the rule that applies keeps the parse, applied innermost
  first; a `;` before `}` goes; adjacent `let` and `const` join; and
  tokens join as above. The runtime factory, already terse, is one token.
  A new emitter construct that binds a name must add it to
  `compiler_name`.
- **Raw `js*`.** A program whose output keeps raw text keeps its layout,
  and its compiler names are the hygiene prefix and a short name
  (`__light0_a`), so raw text still sees only its holes and host globals.
  `optimization::raw_js_names_resolve_only_to_holes_and_host_globals`
  checks it at every level.
- **Retention report.** At terse `Advanced` a unit's bytes are its bytes in
  the slice, names included.
- **Measured.** Over the 25 size-ratchet fixtures `Advanced` output goes
  from 749,499 to 352,350 bytes (190,387 to 112,866 gzipped): −53% raw,
  −41% gzipped. Stripping the runtime's comments and whitespace, with the
  program rewrite, gives 544,806 (138,188); renaming runtime locals
  another 101,876 (13,498 gzipped); naming the slice's top-level bindings
  another 90,580 (11,824). So local renaming inside runtime units is worth
  its build-time pass. Keeping constructor names costs about 2.6% raw and
  0.9% gzipped. `[1 2 3]` (`vector-only`) is 5,883 bytes (2,321 gzipped)
  instead of 13,940 (4,487). See "Terse emission" in
  `benchmarks/compiler.md`.
- **Not done.**
  - ABI property mangling: no property is mangled. The arity ABI
    (`cljs$core$IFn$_invoke$arity$N`, `$variadic`,
    `cljs$lang$maxFixedArity`, `cljs$lang$applyTo`), `cljs$lang$type`,
    `cljs$lang$ctorStr`, the protocol mask fields and protocol slots
    (`cljs$user$P$m$arity$1`) keep their names. The runtime builds most of
    them by concatenation (`"cljs$core$IFn$_invoke$arity$" + n`, slots from
    protocol keys in `installMethods`), so mangling them needs a
    compile-time table the runtime reads, and a program that names one
    through interop (`(.-cljs$lang$maxFixedArity f)`) must keep it, as for
    `getBasis`. They are at most 568 bytes of any fixture (`closed-params`),
    mostly in the runtime slice, and repeat, so gzip shrinks them well:
    light-cljs-232 (maybe later) tracks it with these numbers.
  - A string table: measured by light-cljs-242 and not built. Hoisting
    every string literal that occurs twice or more in the terse output into
    one `const` saves 1.2% raw over the 27 ratchet fixtures but makes the
    gzipped output 1.1% larger, and every fixture with one grows; kept
    only where it lowers the gzipped size, a table saves 4 bytes in all.
    The repeated strings are short kind tags, protocol keys and separators
    that deflate already codes as back-references inside longer runs, and
    the one long string (the protocol-name list) occurs once. See "String
    table" in `benchmarks/compiler.md`.
  - Whitespace and parenthesis rewriting of programs with raw `js*` text.
  - Joining `if`/`else` blocks and dropping block braces around single
    statements in the program (`else{{const ...}}`).

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
  it. A value folded from a constant (light-cljs-221) is the pool entry of
  an equal literal: an item of a constant collection is the entry the
  constant holds it as, as without folding, and the `rest` or `next` of a
  constant list is the entry of a list literal of its tail, which may be
  `identical?` to an equal literal elsewhere where the runtime's tail is
  not.
- The `__hash` field of a pooled collection holds its hash before its first
  `hash`, because the compiler computed it (light-cljs-219), as upstream's
  `_hash` of a keyword does, and assigning the `_hash` or `__hash` field of a
  keyword, symbol or collection through interop does not change the hash of
  a pooled constant that contains it. At every level a collection's `__hash`
  is nil until it is hashed and its hash afterwards.
- `set!` on a core operation forces indirection for that operation only.
- `js*` is opaque: raw text can name only its `~{}` holes and host globals,
  because the compiler renames every name it binds away from it: the
  hygiene pass gives them the `__light<n>_` prefix, which no raw text of the
  program contains, and terse output (light-cljs-215) keeps the prefix on
  the short names of a program with raw text.
  Raw text that reaches compiler state another way (building a renamed
  identifier for `eval`, patching host or runtime prototypes so that core
  operations run program code) is unsupported, and a definition only such
  text reads may be removed.
- `^boolean` tags are trusted, as upstream trusts them: a call of a function
  tagged `^boolean` is tested without `truth`, so one that returns a
  JavaScript-falsy value other than `false` (`0`, `""`) takes the other
  branch at `Advanced`. Since light-cljs-210.
- Protocol slots (`cljs$user$P$m$arity$1` and the `cljs$user$P$` markers) are
  compiler-owned names. A program type has the slots of the methods the
  program can call on it, and a type the program never instantiates has none.
  The program can call a method through it, or by naming its slot in a string,
  property access, method call or `js*` text. Host code that calls a slot the
  program does not name is unsupported, as is raw text that builds an
  instance of a type it does not receive. Since light-cljs-211 (rapid type
  analysis). Likewise a record has the slot of a core protocol method the
  runtime generates (`cljs$core$ILookup$_lookup$arity$3`) where the program
  keeps an operation dispatching the method or names the slot in one of those
  ways (light-cljs-250).
- A `deftype` or `defrecord` type has its static `getBasis` method only when
  the program names `getBasis`: in a property read or method call, as a
  string, or in `js*` text. A name computed at run time does not count. Like
  Closure's removal of properties nothing reads, this keeps `getBasis`'s
  vector of symbols from pulling the collection runtime into every program
  that defines a type. Since light-cljs-211.
- A call of a program function whose target is known at compile time reads
  the method's own binding, `g3_1(a)` (light-cljs-212). A call that runs before
  the function's definition throws a `TypeError` at every level, but its
  engine-generated message names that binding (`b is not a function`, or
  `g3_1 is not a function` with `pseudo_names`) where lower levels name the
  property they read (`Cannot read properties of undefined (reading
  'call')`).
- Messages the JavaScript engine generates quote the output's identifiers,
  which terse output shortens (light-cljs-215), so their text differs at
  `Advanced`: calling a core value that is not a function, such as `INIT`,
  gives `a.call is not a function` where lower levels give
  `rt.core.iteratorInit.call is not a function` (`rt$iteratorInit.call` with
  `pseudo_names`). Messages the runtime builds are the same at every level.
- A function's `name` is its name in the output, and terse output renames
  the program's functions and the runtime's (light-cljs-215), as Closure
  does: `(pr-str inc)` is `#object[aK]` where lower levels print
  `#object[inc]`, and `(.-name (fn foo []))` is `d` where they give `l0`.
  Runtime constructors keep their names (`KeywordValue`,
  `PersistentVectorValue`, `Atom`, the constructor `deftype` and
  `defrecord` build), so the host and the printer show instances as at
  every level. The text of a function (`(str f)`, `*print-fn-bodies*`) is
  its output source, without comments or whitespace.
- An inlined call (light-cljs-214) leaves no stack frame: the `stack` of an
  error thrown in an inlined body names its caller, and recursion that
  overflows the stack may do so at another depth. The function's `name` and
  its definition are gone when nothing else reads it. Inlining never moves
  a call before the definition it reads, so such a call still throws.
- A `goog-define`'s value comes from the build at every level, as upstream's
  `:closure-defines`; at `Advanced`, a `^:const` or `goog-define` Var read
  after its top-level definition is its value (light-cljs-214).
- Where the program has no `extend-type`, `extend-protocol` or `specify!`
  (before light-cljs-246: no protocol implementation), the host-facing
  methods of the runtime's vectors and maps (iteration, `indexOf`,
  `lastIndexOf`, `keys`, `values`, `entries` and the `cnt` field, which
  exists as below) answer from the value's own items (light-cljs-230), and
  those of sets (iteration, `keys`, `values`, `entries` and `has`) from its
  members (light-cljs-231). Applied to a receiver of another type (a record
  too), through `.call`, `.apply` or a copied method, they run the generic
  operation, which a program that builds no other collection does not keep;
  such a call is unsupported there and throws a `TypeError` naming
  `hostHooks`.
- The runtime's types have their upstream fields and methods (`first` and
  `rest` of a `Cons`, `cnt` of a vector, `meta`, `__hash`, `sval` of a
  `LazySeq`, ...: `UPSTREAM_MEMBERS` in `src/runtime_slice.rs`) only where
  reachable code names one of them as a property, a method or a string, or
  as an identifier in `js*` text, or calls `reset-meta!` or `alter-meta!`,
  which assign `meta` (light-cljs-238). Upstream they are compiler-owned
  names, which Closure renames and removes where nothing names them. Host
  code that reads one the program does not name gets `undefined`, which is
  unsupported, as for protocol slots. The host-facing methods that
  upstream's externs keep (`equiv`, `indexOf`, `lastIndexOf`, `toString`,
  iteration) stay with their type.
- The constructor of hash sets is the public `PersistentHashSet` only where
  the program names it (light-cljs-231). Calling it as a value reached
  otherwise, `(type s)` or `(.-constructor s)` of a set, where the program
  never names `PersistentHashSet` is unsupported and throws a `TypeError`
  (`typeHooks.hashSet is not a function` with `pseudo_names`).
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
  function, and since light-cljs-229 the parameters of functions only the
  program calls, joined over their calls, and since light-cljs-243 over the
  calls core operations make of them and the elements of sequential
  collections. It has no context sensitivity: a function's result is the
  same for every call, and parameters of functions that escape otherwise
  stay `any`. Measured so far, the lowerings types enable are for speed: they
  shrink output only where they remove a runtime root.
