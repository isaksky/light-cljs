# JavaScript evaluator migration

## Decision and implementation status

The architecture adopted on October 10, 2026 uses **rquickjs to execute JavaScript
emitted by light-cljs**, including macro bodies and their ordinary helpers.
Rust remains the compiler. Production core/source macros, ordinary helper demand,
embedding, scripts and the REPL now execute compiler-emitted JavaScript in QuickJS.
Immutable compiler reading, structured forms, declaration bundles, ordinary
`.cljs`/`.cljc` imports and effect-preserving application preparation are implemented.
The [execution gate](../tests/reference/evaluator-migration/README.md) tests this
contract. The prior gate ran with every native execution entrypoint disabled;
its [historical evidence and remaining gaps](../tests/reference/evaluator-migration/verification.md#final-resumed-verification)
remain recorded separately. The native interpreter, temporary adapters, native-only
tests and benchmark harnesses have now been physically removed under light-cljs-115.
Retained historical measurements and oracle observations do not provide a fallback
or claim whole-language completion. The [post-removal verification](../tests/reference/evaluator-migration/verification.md#physical-removal-verification--october-11-2026)
passed the complete execution gate and default/all-feature builds.

The [compiler reference](compiler.md) and [evaluation API](evaluation.md) describe
the supported production paths. The [native interpreter reference](interpreter.md)
is historical compatibility evidence. Its exact-number, companion-file, heap and
collector contracts are not the current evaluation contract.

Pellets is authoritative for implementation scope, prerequisites, acceptance and
status. The `native-cljs-macros` name is retained for continuity; its context now
specifies JavaScript execution. `compiler-language-completion` owns the broader
language inventory. Read their current records with:

```sh
pl --json group show 212
pl --json group show 186
pl --json list --group native-cljs-macros
pl --json list --group compiler-language-completion
```

This page records architecture and completion boundaries, not a second task queue.

## Compiler and evaluator responsibilities

Rust owns reading, source metadata and provenance, name resolution, analysis,
compiler-owned forms, semantic IR, optimization and JavaScript emission.
rquickjs executes the emitted code. Both phases use the same canonical
`src/js_runtime.js` operations and lowering semantics. QuickJS provides ECMAScript
execution; it does not implement the ClojureScript language or core library.

Compile-time emission may add deferred-root access and take a conservative,
fast compilation path. It must not become a second implementation of the language.
Whole-program reachability and specialization belong in Rust before final
application emission; invoking a macro must not rerun the application optimizer.
The Rust application preparation pass now separates compile-time demand from
ordered effect roots and conservative reachability; it is not a complete general
optimizer. See [the preparation boundary](compiler.md#application-preparation).

The migration does not introduce SCI or another evaluator in JavaScript, an
upstream self-hosted compiler, a permanent native fallback, or a Rust numeric
profile for macros. JVM/GraalVM execution, `.clj` macro companions and unchanged
compatibility with existing applications are not goals. Engine selection is
settled; implementation tests and ordinary performance regression checks belong
in the implementation work, not in another feasibility phase.

## Macro and phase contract

- `defmacro` creates a compile-time binding in `.cljs` or `.cljc`. It produces
  no application function or macro Var; its declaration result is nil.
  Runtime value, `apply` and Var uses of a macro receive located phase errors.
- Ordinary `:require` imports macros and runtime functions from the same
  namespace. Macros can call ordinary unmarked helpers also used by applications.
  Compiler source lookup prefers `.cljs` to `.cljc` and never loads `.clj`
  macros or helpers. Legacy macro-import clauses get migration diagnostics.
- Both phases select `:cljs` and the usual `:default` branch in `.cljc`.
  Inactive `:clj` branches cannot add dependencies. There is no new platform tag;
  the current rejection of reader conditionals in `.cljs` is retained.
- Ordinary definitions initialize at compile time only when execution demands
  their values. Quotation, identity/metadata lookup, analysis and an untaken
  branch do not initialize roots. Closures, higher-order calls, `apply`, functions
  stored in collections and supported Var consumption all use the same rule.
- Compile-time and application roots, mutations, lazy caches and dispatch tables
  are separate. Standalone top-level runtime expressions are never replayed to
  prepare macro state. Application initialization retains source/dependency order.
- Shared numeric computation uses JavaScript Number throughout, including
  division, rounding, predicates, equality, hashing and map/set keys. The earlier
  Rust macro-number profile proposal is superseded by `light-cljs-100`, whose
  [phase audit](../tests/reference/compiler-numeric.md#compile-time-and-application-number-semantics)
  verifies emitted macro bodies and QuickJS/Node applications against explicit
  expected outcomes and both pinned upstream references.
- Ordinary ECMAScript operations may execute during expansion. Embedding QuickJS
  does not supply a DOM, Node/Bun globals, npm resolution, JVM classes or ambient
  filesystem/network/process access. Host callbacks, streams and source providers
  remain explicit capabilities.

### Source order and deferred definitions

Definitions become visible in source order, including top-level `do` and
macro-generated declarations. Locals shadow unqualified macros; qualified names
retain their binding identity. One namespace mapping distinguishes macro and
ordinary bindings and diagnoses conflicting kinds. Macro declarations under
runtime lexical or conditional execution are rejected.

Each deferred definition retains its owned form, source, binding identity,
generation and definition-site resolution snapshot: aliases, refers, exclusions,
visible declarations and macro generations. Delaying analysis cannot resolve a
name in a later caller's environment or make an undeclared future name legal.
`declare` and an initializer-free `def` reserve ordinary identities without
making a future initializer available. They do not declare future macros.
The default compiler's unresolved-name errors and explicit warning API remain
distinct.

Analysis/expansion is reused once per valid definition generation and resolution
snapshot, then lowered for the required phase. First demand can initiate analysis;
remaining application units are processed deterministically in source order.
The same source macro occurrence must not execute twice merely because both
phases need its analyzed helper. Saved functions retain code and lexical captures;
ordinary global reads observe the active same-kind generation.

Root initialization tracks **pending, initializing, ready and failed** states.
A function can become ready before its body executes, allowing self/mutual
recursion through available definitions. Reading an initializing root instead
reports an initialization cycle with its demand path. Consuming a declaration
whose initializer has not been encountered reports an unavailable definition.
Compilation cycles through macro expansion are tracked separately from these
value cycles.

A failed generation retains its diagnostic and does not repeat initializer
effects on another read. Ordinary redefinition installs a new pending generation.
Compile-time `defonce` preserves an existing pending, ready or failed generation,
including nil/false; declaration-only slots permit initialization. A pending
`defonce` remains preserved even if its eventual value is undefined. This is the
chosen demand policy; application `defonce` retains its current undefined-root
semantics.

Dynamic bindings and `with-redefs` take precedence over root forcing. Initializer
expressions use the existing parallel-binding semantics; restoration runs in
reverse order and restores the exact previous pending/ready/failed state without
forcing it. Scope failures restore installed overlays. Ordinary nested definitions
and mutations execute only in the phase actually executing their containing code.
Already-performed external host effects cannot be rolled back.

### Protocols, types, records and extensions

`defprotocol`, `deftype`, `defrecord`, `extend-type` and `extend-protocol` have
explicit declaration bundles. Products include descriptors, method bindings,
constructors/factories and extension registrations. Demand of a product initializes
its bundle and publishes it only after installation succeeds. Callable/type shells
may support method linking and recursion, but do not permit arbitrary reads of
unfinished values.

Protocol invocation, `satisfies?` and applicable core operations force relevant
source-visible receiver and extension bundles before dispatch. Extension lookup
is indexed by protocol and target identity. A computed target alias is demanded
only when that protocol query needs to establish applicability. Unrelated or
future extensions do not initialize. Nil/native/default selectors and marker
protocols participate in the same contract.

Failure rolls back unpublished slots and dispatch patches, while retaining failed
state. New generations invalidate affected phase-local dispatch caches. `reify`
and `specify!` inside executed helpers retain ordinary expression semantics;
standalone runtime effects are not replayed. Runtime declaration effects remain
ordered application roots where required. `light-cljs-110` owns this model;
later multimethod declarations follow it as well.

## Bootstrap and form conversion

Bootstrap uses a bounded Rust primitive compiler seed and pure form transforms.
First-party macro sources are compiled by light-cljs to JavaScript; primitive
`fn*`/`let*`/`loop*`/`letfn*`, control forms and canonical core calls avoid a circular
dependency on the evaluator being removed. `defmacro` registration itself is a
compiler declaration. Bootstrap cycles are resolved in that bounded source,
never through native evaluation.

Core bootstrap is implemented in `compiler_bootstrap.rs`. The dependency-ordered
`compiler_core_macros.cljs` and `compiler_sequence_macros.cljs` units are analyzed
and emitted by this compiler, then installed in QuickJS. The sequence source
retains its EPL notice and upstream pin. All control/iteration/interop/selector
algorithms execute compiled JavaScript; binding/function/destructuring and
namespace reflection use the pure Rust seed. The old embedded `.clj` resource,
inline native source strings and runtime-defmacro shim are deleted.

Only immutable emitted unit strings are cached in memory by the running compiler
binary. There is no disk artifact or engine bytecode cache. Each clean compilation
installs the units in a fresh runtime with independent functions, gensyms and
roots. The regeneration test bypasses that cache and executes both freshly emitted
units, including collection during bootstrap. The former
`disable_native_evaluation()` test seam was removed with the native implementation.

A standalone source `defmacro` is now a compile-time declaration returning nil;
its body can call core operations and ECMAScript intrinsics. Same-file `.cljs`
and `.cljc` declarations, generated/top-level `do` interleaving, source-ordered
macro generations, binding-kind conflicts, declaration metadata and runtime
phase errors are implemented and exercised by `same_file_javascript_macros`
in `tests/compiler_macros.rs`. Source macro analysis carries definition-site
namespace/resolution snapshots and preserves host assertions and expansion
budgets. Deferred ordinary initializer units now have a reentrant compilation
service, frozen definition snapshots and once-per-generation semantic IR shared
between both emissions. Nested macro/helper compilation, cycles, alias and macro
generation snapshots, failure recovery and shared limits are exercised by
`reentrant_deferred_units` in the same suite. Ordinary value demand, lazy Var
identity/resolve, root generations/failures/defonce and exact overlay restoration
are now exercised by `ordinary_helper_demand`; [validation evidence](../tests/reference/compiler-helper-demand.md)
records commands and phase-isolation witnesses. Declaration bundles and ordinary source imports are now implemented and exercised
by their demand/import suites. Legacy embedding APIs and native adapters have been
removed; production sources cannot load `.clj` companions.

Same-file macro validation on 2026-10-10 passed
`cargo test --test compiler_bootstrap --test compiler_macros --test compiler_boundary_completion`
(88 tests, including the eight direct same-file execution/diagnostic tests) and
`python3 -m unittest discover -s tests/reference/compiler-language -p test_report.py`
(39 tests). `git diff --check` passed. The focused
[macro boundary verification](../tests/reference/compiler-language/boundary-support.md#same-file-macro-verification-2026-10-10)
passed local compilation/execution and both pinned upstream references while
retaining the deliberate runtime ABI difference. The full project suite and
complete compiler-language reference report were not run for this scoped change.

Numeric values crossing the compiled macro boundary use JavaScript Number:
`case` tests `1` and `1.0` are consequently duplicate constants. Compiler-owned special forms, reader
transforms, namespace snapshots and declaration lowering remain in Rust.
Any precompiled bootstrap JS must be reproducibly generated by this compiler
with valid source/compiler/runtime/options assumptions. Reference-only `.clj`
oracle files may remain, but are never production macro inputs.

The bridge carries structured compiler-owned forms, not JSON or printed/reparsed
source. It preserves symbols versus keywords, lists versus vectors, maps/sets,
exact UTF-16 names and strings, Number bits, metadata, ordered source children,
supported reader-literal payloads and expansion provenance. JS values use canonical
runtime representations; origin side tables must not alter equality or hashing.

Expansion results are finite readable literals and canonical collection/sequence
trees with representable metadata. Lazy result sequences may realize under shared
bounds. Functions, Vars, atoms, arbitrary host/type/record objects, promises,
cyclic values and other unsupported results get located category/path diagnostics;
valid intermediate macro values need not be valid returned forms. `#js` literal
data requires explicit descriptors rather than guessing from arbitrary JS objects.

`&form` retains the actual call shape/location. `&env` stays bounded to the existing
lexical binding, expression-context, namespace/import and location information;
there is no upstream analyzer compatibility API. Syntax quote preserves
definition-site resolution, unquote/splicing and hygienic gensyms. Nested errors
retain macro definition, caller and demanded-helper provenance.

## Sessions, ownership and reentrancy

The implementation baseline is synchronous rquickjs 0.14.0 with
`default-features = false` and `features = ["std"]`. The matching crate source was
inspected during planning. No async, parallel, module-loader or serialization
feature is needed initially. rquickjs-specific types stay behind the evaluator
and macro-host boundary; analyzer/IR data remains compiler-owned.

One clean compilation owns one fresh Runtime/Context for all its expansions.
`MacroHost` reuse preserves configuration, not source-defined mutable roots.
An explicit interactive evaluation session owns persistent application state in
a separate heap. Compiled functions are retained in a private JS registry and
referenced by session/generation IDs. Foreign or expired handles are rejected.
Any `Persistent` roots must be cleared before Context/Runtime teardown. Rust-backed
JS objects retaining JS edges must trace them correctly; callbacks must not hide
untraced JS references or create Rust/JS ownership cycles.

A macro can demand a helper whose compilation needs another macro expansion.
The required reentrant path uses separate owned analysis frames and immutable
resolution snapshots. Shared catalog borrows are short and end before analysis
reentry or a JS call. A callback receives the active `Ctx` and uses it for nested
execution: it must not call `Context::with` again or use runtime-locking setters.
Reentrant callbacks use `Fn`, not a mutably borrowed `MutFn` closure. No raw
reentrant `&mut` pointer or lifetime extension is part of this design.

Scope guards restore compilation frames, namespaces, environments, demand stacks
and dynamic bindings on failure. Nested work shares cancellation and expansion,
node, depth, conversion and resource bounds; it cannot reset them on reentry.
Host cancellation/limits remain terminal even if JS catches an engine exception.
Failed compilation discards the session, and a later clean compilation on the same
configured host must succeed. Interactive language errors preserve completed
runtime effects; cancellation or unrecoverable engine failure requires session reset.

Unchanged definitions compile once per applicable session/generation/mode.
Ready root reads and normal collection operations stay in JS. There is no runtime
recreation per macro call, daemon, disk cache or broad incremental-build prerequisite.
Only immutable artifacts with valid dependency/compiler/engine/options assumptions
may be reused across clean compilations.

## Required behavior examples

These are implementation acceptance requirements, **not currently passing examples**.

```clojure
(def factor (+ 1 1))
(defn twice [x] (* factor x))
(def connection (js/connect))
(defmacro doubled [x] (twice x))
[(doubled 21) (twice 21)]
```

Compilation demands `twice` and `factor` without calling `connect`. With the test
host supplied, application execution calls `connect` once in initialization order
and returns `[42 42]`.

```clojure
(def hits (atom 0))
(defn next-id [] (swap! hits inc))
(defmacro id [] (next-id))
[(id) (id) @hits]
```

Application execution returns `[1 2 0]`. Every clean compilation repeats that
result, including reuse of the surrounding host. A separate fixture with a
standalone top-level `(swap! hits inc)` must show mutation only at application time.

```clojure
;; tools.cljc
(ns tools)
#?(:cljs (def factor 2)
   :clj (def factor jvm-only-placeholder))
(defn twice [x] (* factor x))
(defmacro doubled [x] (twice x))

;; app.cljs
(ns app (:require [tools :as t :refer [doubled]]))
[(doubled 21) (t/doubled 21) (t/twice 21)]
```

Both phases select `:cljs`; application execution returns `[42 42 42]`.
An ordinary require still preserves necessary dependency initialization even
when the caller uses only a macro. Compile-time use alone does not make a pure
helper a runtime root, but eliminating it must not erase application effects.

## Interpreter extraction and consumer migration

| Current responsibility | Endpoint |
| --- | --- |
| Forms, names, ordered children, metadata, source spans/origins, syntax quote and namespace views | Immutable forms/data/provenance are extracted in `compiler_data`; analyzer/IR use them without native roots. `compiler_reader` and immutable `NamespaceView` now provide semantic reading, syntax construction and resolution snapshots. Pure compiler seed transforms provide destructuring; core/namespace metadata and assertion formatting use owned data. `source_provider` supplies engine-neutral request/input/error types. `native_form_adapter` and native compatibility storage have been removed; compiled macro conversion uses the structured form bridge and reader callbacks use source functions. |
| Macro algorithms, ordinary helpers, core/runtime collections, Vars/state and dispatch | Compiler-emitted JS using the canonical runtime and phase-local roots. |
| Native evaluation stack, exact numeric tower, native heap/GC/reuse and eager namespace evaluation | Removed; there is no fallback. |
| Public eval/source/form/invoke/macroexpand and embedding options/callbacks/readers/IO | Engine-neutral compiler-plus-QuickJS APIs, with explicit ownership and capabilities. |
| `--eval`, scripts and `--repl` | Retain product functions through the new evaluator, including incremental input, history and error recovery; migrate source files and documented limit controls. |
| Tests, examples, reference adapters, CI and benchmarks | Port supported semantics, retain labeled historical evidence, replace lifetime/limit tests and delete obsolete native-only machinery. |

The native arbitrary-precision proposal (`light-cljs-48`) and native-only built-in
extension descriptors (`light-cljs-66`) are superseded, not implemented. Number
semantics and public CLJS extensions remain intended. Shared-owner native fork,
GC-internal and exact native resource-unit contracts are explicitly retired or
mapped to replacement APIs; independent evaluator sessions do not implicitly
share mutable values.

## Language coverage and completion boundaries

Evaluator migration and language completion are different milestones. Existing
compiler execution tests cover substantial functions, control, state, protocols,
types, records, metadata, exceptions and sequence behavior. Native support, parsed
syntax or a mapped core name alone does not establish canonical JS support.
The broader inventory records parsing, analysis/lowering, native implementation,
canonical JS implementation, macro eligibility and meaningful execution separately.

The current [selected inventory](../tests/reference/compiler-language/README.md)
contains 3,237 assertion sites, of which 2,535 are selected source-language sites.
Its historical exclusions include 63 transient, 105 transducer, 104 sorted and
31 multimethod/hierarchy sites. Those features are intended language obligations,
not permanent dialect exclusions. Eager transduction now has a separate
[executing audit](../tests/reference/compiler-transducers.md). A selected-scope report with zero missing cannot
declare the language complete. `light-cljs-116` expands that executable ledger;
the existing report has not been expanded by this documentation update.

Remaining feature ownership is recorded in Pellets: transients (`117`),
sequence/eduction (`59`/`60`), multimethods (`47`), sorted collections (`119`),
equality/hashing/metadata (`120`), sequence/iterator gaps (`121`), regex (`49`),
core utilities (`122`), supported string/set/walk/EDN libraries (`123`),
printing/reflection/host hooks (`124`) and reader coverage (`9`). All IDs have the
`light-cljs-` prefix. Canonical eager transducers replace the supported native
transduction APIs (`118`); supported library facilities must also be replaced
before removing their production consumers.

Private upstream analyzer/storage layouts, build systems, dependency acquisition,
browser/Node facilities and unchanged application ports remain outside the agreed
scope. The previous milestone also did not promise every standalone standard
library or newer async/proxy library. Missing ordinary core language features
cannot be excluded merely because implementation is inconvenient.

Evaluator migration is complete only when:

1. Cold bootstrap and user macros execute this compiler's JavaScript in rquickjs,
   without native evaluation, a second compiler/evaluator or `.clj` macro loading.
2. Same-file/generated declarations, ordinary imports, reader selection, demand
   initialization, declaration bundles, reentrancy and phase diagnostics pass
   meaningful execution tests, including the examples above and higher-order,
   recursion/cycle, failure/redefinition/defonce and dynamic-restoration cases.
3. Structured forms, metadata/provenance, `&form`/bounded `&env`, syntax quote and
   Number edge cases work, including division, `2^53`, numeric keys, observable
   signed zero and NaN/Infinity.
4. Canonical lowering/runtime operations are shared, Rust owns final optimization,
   compile-only dependencies are not automatic application roots, and required
   runtime initialization effects remain ordered.
5. All retained production evaluation, scripting, REPL and embedding consumers
   are migrated with their supported CLJS behavior or explicit native-only retirement.
6. Ownership, tracing, limits/cancellation and failure cleanup pass, including a
   failed or cancelled compilation followed by success in a reused host with fresh roots.
7. Required default/all-feature builds and migrated tests pass with the native
   interpreter and every temporary adapter physically removed; documentation,
   examples, commands, CI and coverage accounting match the new implementation.

`light-cljs-114` owns the migration execution gate and `light-cljs-115` the removal
milestone. `light-cljs-125` owns the separate full intended-language completion
gate. Unimplemented, blocked, skipped, timed-out and unexecuted cases remain
visible; parsing a checkout is never execution coverage. Use pinned upstream
semantic checks where compatibility is intended and dedicated local assertions
for this dialect's macro, import and phase rules.

### Historical core bootstrap verification (light-cljs-108)

The following results describe the pre-removal revision, including the now-removed
native-disabled test seam and old test names. They are retained as historical
evidence, not commands or current test counts. Literal QuickJS results were checked:

- `cargo test --lib compiler_bootstrap`: 1 regeneration/GC test passed.
- `cargo test --test compiler_bootstrap --test compiler_macros --test compiler_destructuring_completion`: 76 tests passed.
- `cargo test --test compiler_boundary_completion --test compiler_sequences --test evaluator --test provenance`: 24 tests passed; the opt-in evaluator startup measurement was not run.
- `cargo test --test compiler_interop`: 3 tests passed.
- `cargo test --test boundaries`: 2 tests passed, including API/CLI diagnostic locations.

The macro tests include compile-time ECMAScript calls, all core families,
expansion bounds, malformed inputs and a failure followed by a successful clean
compilation using the same host. JVM reference rebuilds, cross-platform runs and
the later source/helper demand acceptance programs were not run for this step.
`cargo check --all-features` and `git diff --check` passed. `cargo clippy --lib`
completed with nine existing style warnings in the extracted binding/reader,
legacy adapter and macro-host code; `-D warnings` remains unsuccessful for those
pre-existing warnings.
