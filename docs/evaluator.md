# Synchronous evaluator

For supported scripting and embedding use [EvalSession](evaluation.md). The
low-level `Evaluator` remains the compiler execution boundary; it does not itself
provide interactive compiler symbol state.

`evaluator::Evaluator` executes compiler-produced JavaScript with exactly
`rquickjs = "=0.14.0"`, default features disabled and only `std` enabled.
Cargo.lock pins the engine and build dependencies. Building requires the normal
Rust toolchain and a C compiler for QuickJS. No Node process is required for
embedded execution. Node remains an independent runtime in execution tests.

`emitter::emit_js` retains self-contained application output. `emit_js_unit`
returns a function accepting a canonical runtime instance; `Evaluator::evaluate`
uses this form. Each session loads the exact `src/js_runtime.js` factory once
(without its `//@unit` manifest headers, which `build.rs` removes).
Collections, functions and dispatch tables therefore share brands across units,
while separate sessions have separate state. No alternate macro core exists.
Canonical `gensym` has a session-local counter. `setMacroexpandServices` explicitly
installs dynamic `macroexpand`/`macroexpand1` hooks; missing services fail on use.
Canonical literal quoted source calls, such as `(macroexpand-1 '(let [x 1] x))`,
expand during analysis in both application and ordinary `.cljs` macro phases,
without runtime hooks. They expand only the outer form (once or repeatedly),
retain unknown/nonmacro lists, and follow the canonical non-list source behavior.
Malformed quoted calls produce located compiler errors. Lexical/source shadowing
is respected. Dynamic computed forms remain an explicit hosted service; these
source macros do not provide bare-runtime `eval` or loading capabilities.

`MacroHost::evaluator_options` contains reusable configuration;
`MacroHost::new_evaluator()` creates a fresh heap. CLI scripts, the REPL and public
`EvalSession` embedding use compiler-emitted JavaScript; there is no other
evaluator to fall back to. `Evaluator::active(ctx)` borrows the
already-entered context for nested compilation, without reacquiring its runtime
lock. `install_unit_service` installs a reentrant `Fn` receiving that context and
an owned request ID. It catches panics, preserves owned failure diagnostics, and
shares sticky cancellation/resource limits with the outer operation.

## Compile-time phase contract

Rust owns reading, source metadata, name resolution, analysis, semantic IR,
optimization and JavaScript emission. QuickJS only executes the emitted code; it
does not implement the language or core library. Macros and the ordinary helpers
they call run as compiler-emitted JavaScript using the same canonical
`src/js_runtime.js` operations as applications, so both phases share lowering and
JavaScript Number semantics.

- `defmacro` in a `.cljs`/`.cljc` file declares a compile-time binding. It
  produces no application function or initializer and evaluates to nil. Its Var
  identity and metadata are reflectable; using the macro as a runtime value,
  `apply`ing, dereferencing or assigning it is a located phase error.
- Ordinary `:require` (with `:as`, `:refer`, `:rename`) imports both macros and
  runtime functions from the same namespace, and macros can call that namespace's
  ordinary helpers. Source lookup prefers `.cljs` over `.cljc` and never loads
  `.clj`; legacy macro-import clauses get migration diagnostics. Both phases
  select the `:cljs` (then `:default`) branch of `.cljc` reader conditionals;
  `.cljs` files reject reader conditionals.
- Definitions become visible in source order, including inside top-level `do`
  and macro-generated declarations. Locals shadow unqualified macros. Macro
  declarations under runtime lexical or conditional code are rejected.
- An ordinary definition is initialized at compile time only when executing
  macro code demands its value. Quoting, Var identity/metadata lookup, analysis
  and untaken branches do not initialize it. Each deferred definition keeps its
  definition-site resolution snapshot, so delayed analysis never resolves names in
  a later caller's environment.
- Roots are pending, initializing, ready or failed. Functions can become ready
  before their bodies run (allowing recursion); reading an initializing value
  reports an initialization cycle with its demand path. A failed generation keeps
  its diagnostic and does not repeat effects; redefinition starts a new
  generation. Compile-time `defonce` keeps any existing generation.
- `binding`/`with-redefs` overlays take precedence over root forcing and restore
  the exact previous root state without forcing it.
- `defprotocol`, `deftype`, `defrecord`, `extend-type` and `extend-protocol` are
  declaration bundles: demand initializes the whole bundle and publishes it only
  after installation succeeds; failures roll back dispatch patches. Protocol
  calls and `satisfies?` force only the source-visible extensions relevant to that
  protocol.
- Compile-time and application state are separate: roots, mutations, lazy caches
  and dispatch tables are not shared, standalone top-level effects are never
  replayed to prepare macro state, and every clean compilation (even on a reused
  `MacroHost`) starts with fresh roots. Application initialization keeps
  source/dependency order, and compile-time use alone does not make a helper an
  application root.
- ECMAScript intrinsics are available during expansion; DOM, Node globals, npm,
  JVM classes and ambient filesystem/network/process access are not.

Examples (each runs today):

```clojure
(def factor (+ 1 1))
(defn twice [x] (* factor x))
(def connection (js/connect))
(defmacro doubled [x] (twice x))
[(doubled 21) (twice 21)]
;; Compilation demands twice and factor without calling connect; the application
;; calls connect once, in order, and returns [42 42].

(def hits (atom 0))
(defn next-id [] (swap! hits inc))
(defmacro id [] (next-id))
[(id) (id) @hits]
;; => [1 2 0] on every clean compilation: macro-time swaps do not touch the
;; application's atom.
```

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
;; => [42 42 42]
```

Core macros bootstrap from the first-party `src/compiler_core_macros.cljs` and
`src/compiler_sequence_macros.cljs`, which this compiler analyzes and emits
(using a bounded Rust seed for primitive and binding/destructuring forms) and
installs in QuickJS. Only the emitted strings are cached in memory; there is no
disk or bytecode cache.

## Ownership and capabilities

Only opaque `ValueId` values leave the engine boundary. IDs include a unique
session generation, fail explicitly in another session, and become invalid when
released or when their owner drops. The private JavaScript registry holds roots;
Rust stores no `Value`, `Function`, `Ctx` or `Persistent` outside an entry scope.
Drop clears registry roots using a captured intrinsic, then drops Context before
Runtime and runs collection. Cycles and callbacks stay within that ownership.

`evaluate_js` is an explicit JavaScript embedding capability, not an untrusted
code sandbox. ECMAScript Math, strings, RegExp, JSON and Object work. DOM, process,
Node module loading and network globals are absent unless explicitly provided;
untaken branches do not probe them. `install_callback` currently supports a small
owned-string bridge. Its callable captures only Rust state, plus a weak execution
control reference. The structured compiler-form bridge does not use this string API as a form encoding. No promises are pumped or awaited.

## Compiler form bridge

`form_bridge::FORM_BRIDGE_VERSION` and `rt.compilerForms.version` identify the
in-process descriptor ABI (currently 1). `Evaluator::import_form`/`import_data`
and `export_form`/`export_data` transport actual engine arrays with primitive
payloads. Strings and name components cross as UTF-16 code units; Number payloads
cross as doubles, preserving signed zero and nonfinite values. Neither ordinary
JSON nor printing/reading is used. Symbols and keywords retain separate namespace
and name components; list/vector/map/set kinds remain distinct. Metadata stays in
the canonical runtime's private metadata store and never affects equality/hash.

A session-private identity table retains borrowed compiler data, original ordered
children and locations. Conversion snapshots the canonical form once on import.
Unchanged returned values recover their original data/source nodes, including
reader lexical distinctions. Mutated Date or explicit `#js` values instead create
new forms. Constructed forms retain call location and an owned `ExpansionOrigin`
with macro, definition, helper chain and prior expansions. These compiler forms
remain valid after evaluator teardown; engine value IDs cannot cross sessions.
`analyzer::analyze_compiler_form` analyzes returned code directly without parsing.

`MacroHost::expand_javascript` accepts a compiler-emitted function's session ID,
actual call Form, compiler-owned environment Datum and ExpansionOrigin. It invokes
the hidden `&form`/`&env` ABI followed by call arguments. `compiler_environment`
builds bounded lexical IDs/names/kinds/arguments, shadow records, context,
namespace/import information and line/column using compiler data alone. Source
macro registration and the default core bootstrap both execute compiler-emitted
JavaScript.

Finite canonical lists, sequences, vectors, maps and sets are readable results.
Lazy sequences may realize under the same engine control and conversion bounds;
functions, Vars, atoms, type/protocol objects, promises, arbitrary objects,
JavaScript Symbols/BigInts and cycles are rejected with category and path.
Diagnostics include macro definition/call and helpers, and do not call arbitrary
getters or `toString`. Regex, valid instants and UUIDs retain compiler literals.
Host arrays/objects require explicit `rt.compilerForms.jsArray`/`jsObject`
descriptors; an arbitrary host object never becomes a Clojure map implicitly.

`install_form_services` routes nested `macroexpand-1` and `macroexpand` through
one compiler-supplied resolution service. It releases all Rust table borrows
before compiler reentry and uses the active `Ctx` for nested engine work. Callback
returns own only Rust forms; lifetime-generic `IntoJs` creates temporary JS edges.
An explicit `install_gc_callback` exercises GC during lazy output conversion and
between nested service callbacks, verifying retained descriptors and roots.
Normal collection operations remain JavaScript operations. Gensym shares one
session counter and avoids imported call/environment symbol names; exact spelling
is not an API promise. Syntax quote retains the reader's definition-site view.

Focused bridge execution checks are in
`tests/compiler/macros.rs` (`javascript_form_bridge`).

## Resource controls

Limits are configured before Context entry. Defaults are a 64 MiB allocation
ceiling, 1 MiB engine stack limit, one million cooperative polls, and a ten-second
session deadline. These are session-wide compilation limits, not reset per nested
call. Engine interrupts and Rust `ExecutionControl::poll` share fuel/deadline/
cancellation. Long Rust callbacks must poll cooperatively. A clone of the control
can cancel from another thread without accessing the engine.

QuickJS's bundled regex matching VM polls this handler during backtracking;
bounded pathological matching tests verify budget, deadline and cross-thread
cancellation. This is cooperative interruption, not a hard latency guarantee
for every builtin: pattern compilation and string copying can run until their
next poll or return. The host checks controls again after return. Emitted Node
applications use their embedding's resource controls.

Allocation accounting wraps the upstream `RustAllocator` with a small documented
unsafe boundary. It rejects over-budget allocation before allocation and marks
the terminal state even if JavaScript catches the resulting exception. This is
needed because the engine's ordinary memory limit alone is catchable. Alignment
and deallocation delegate to the same upstream allocator; accounting includes
rounded usable payload sizes (not allocator headers or Rust compiler allocations).
Callback panics are contained before crossing C and poison the session. Controls
are checked again on return, so catches cannot restore a cancelled/exhausted
operation. Errors are owned strings, and a new session on the same host starts
clean. Compiler callers must poll the same control during their Rust work.

## Verification and measurements

`cargo test --locked --test evaluation evaluator::` compares compiler-produced arithmetic,
collections, closures/apply, state, exceptions, lazy sequences, protocols, types,
records and interop against direct expected values in QuickJS and Node. It also
checks cross-session/released IDs, explicit capabilities, cancellation in caught
Rust callbacks, caught infinite work/allocation failure, panic containment,
cycles, fresh roots and repeatable live engine allocation counts.

`cargo bench --locked --bench evaluator -- --iterations 20` reports validated
fresh-session, reused-session and bounded-loop/reset/recovery costs as total
nanoseconds and iteration counts. Use `--iterations 1` for a smoke run. The
reused phase retains one application heap; the recovery phase validates a terminal
poll limit followed by reset and successful evaluation.

`cargo test --locked --release --test evaluation evaluator::startup_measurement -- --ignored --nocapture`
is a focused cold-session startup/heap measurement; it is deliberately not a
flaky timing gate. `node tests/support/js_runtime.test.js` exercises the same
canonical runtime source and bootstrap hooks directly in Node. A past measurement
run is recorded in [benchmarks/results](../benchmarks/results/evaluator-2026-10-10.md).

Upstream implementation provenance: [rquickjs 0.14.0 source](https://docs.rs/crate/rquickjs/0.14.0/source/)
and [rquickjs-core 0.14.0 source](https://docs.rs/crate/rquickjs-core/0.14.0/source/),
especially synchronous runtime/context, allocator and function callback tracing.

## Deferred compiler units

`compilation_session` catalogs ordinary top-level initializer forms with stable
Var identities and source generations. Each unit owns its namespace, aliases,
refers, exclusions, visible declaration/metadata snapshot, macro-generation map,
analysis options and source provenance. Analysis uses an independent private
frame. The catalog borrows only to select/mark/copy a request and to commit its
result; no `RefCell` guard survives compiler reentry or JavaScript execution.
Callbacks capture weak session/evaluator handles, so no Rust/JS ownership cycle
retains the heap.

Unit analysis is unstarted, analyzing, analyzed or failed. This is independent
of pending, initializing, ready or failed JS roots. Analysis reentry reports the
helper dependency path; ordinary recursive functions use already-installed code.
A scope guard unwinds the analysis stack even on failure. Frame ownership restores
namespace and macro environments automatically. Active engine handles reuse the
same context for calls, conversion and installation; they never configure the
runtime, collect garbage or call `Context::with`.

Successful semantic IR is reused by compile-time emission and final application
assembly. `program_fragment` remaps arena IDs while preserving binding identity
and original locations. A unit's snapshot keeps the program's declarations and
metadata, with only the literals (and, for executable metadata, the locals) that
metadata uses, and records where those entries live in the program it was taken
from. Assembly appends only what the unit itself added, so the assembled
program grows linearly with its definitions. Compile-time emission uses the ordinary emitter with
root access hooks and never invokes an application optimizer. Ready root reads
stay in JS. Unrequested initializers drain in source order; standalone runtime
expressions are not replayed during macro setup. The warning API drains each
source form early to preserve diagnostics before later errors.

The unit cache is scoped to a fresh compilation. No application-source artifact
survives host reuse, so source, options, imports and selected conditional edits
cannot reuse stale IR. The separately cached bootstrap source is immutable and
built from this binary's embedded sources. Required `.cljs`/`.cljc` namespaces
use the same compiler and selected `:cljs` branch for macros and ordinary helpers.
Legacy macro-import syntax reports a migration diagnostic; `.clj` companions
are never loaded by compiler source lookup. The session's JS registry now retains compiled
code and pending/initializing/ready/failed roots. Only missing code crosses the
Rust service boundary; cached reads and ordinary initializer cycles stay in JS.
Var construction and `resolve` inspect identity without demanding roots. Dynamic
and redefinition overlays take precedence; saving/restoring them leaves pending
and failed roots intact. Failure is sticky within a source generation, and
ordinary redefinition replaces that generation. Compile-time `defonce` preserves
any encountered generation, including a pending promise that returns undefined;
application lowering retains its ordinary undefined check.

Declaration bundles use the same unit catalog and generation analysis cache.
Their JS state stages named descriptor/factory/method products until complete
installation. Canonical dispatch journals owned property/table patches for
rollback and for restoring an earlier deferred-analysis extension frontier.
Relevant extension candidates are indexed by protocol and known target identity;
computed aliases remain protocol-specific deferred candidates. Ready products,
method calls and cached target values stay in JS. Reentrant analysis scopes
restore the dispatch frontier on every nonterminal exit. Runtime output receives
the same analyzed declaration IR with ordinary source-order effects and an
independent canonical runtime.

These paths are exercised by the `reentrant_deferred_units`,
`ordinary_helper_demand` and `declaration_bundle_demand` test modules in
`tests/compiler/macros.rs`.

## Explicit reader capabilities

`Evaluator::input_provider(name, callback)` creates an opaque input value for
compiled reader functions. The synchronous Rust callback receives the session's
`ExecutionControl` and returns `Result<Option<String>, EvalError>`: a nonempty
chunk, EOF (`None`), or an error. Parsing, tag invocation, cursor ownership,
compaction and located diagnostics remain canonical JavaScript operations.
Callbacks must retain only Rust data and poll control during potentially long
provider work. The provider never discovers stdin or filesystem access.

The ordinary namespace resolver supplies compiler-owned `cljs.reader` and the
strict `clojure.edn` compatibility facade; see
[compiler-owned libraries](compiler.md) for the reader profiles.
