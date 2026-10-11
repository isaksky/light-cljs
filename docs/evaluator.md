# Synchronous evaluator

`evaluator::Evaluator` executes compiler-produced JavaScript with exactly
`rquickjs = "=0.14.0"`, default features disabled and only `std` enabled.
Cargo.lock pins the engine and build dependencies. Building requires the normal
Rust toolchain and a C compiler for QuickJS. No Node process is required for
embedded execution. Node remains an independent runtime in execution tests.

`emitter::emit_js` retains self-contained application output. `emit_js_unit`
returns a function accepting a canonical runtime instance; `Evaluator::evaluate`
uses this form. Each session loads the exact `src/js_runtime.js` factory once.
Collections, functions and dispatch tables therefore share brands across units,
while separate sessions have separate state. No alternate macro core exists.
Canonical `gensym` has a session-local counter. `setMacroexpandServices` explicitly
installs `macroexpand`/`macroexpand1` hooks; missing services fail on use.

`MacroHost::evaluator_options` contains reusable configuration;
`MacroHost::new_evaluator()` creates a fresh heap. The transitional native macro
adapter still exists until light-cljs-115; source macros and public native CLI/
embedding consumers switch in their assigned migration steps. Creating this
session does not execute source macro bodies through the old adapter, and engine
failure never falls back to native evaluation. `Evaluator::active(ctx)` borrows the
already-entered context for nested compilation, without reacquiring its runtime
lock. `install_unit_service` installs a reentrant `Fn` receiving that context and
an owned request ID. It catches panics, preserves owned failure diagnostics, and
shares sticky cancellation/resource limits with the outer operation.

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
macro registration and default bootstrap switching remain separate migration
steps; the native adapter is explicitly temporary until light-cljs-115.

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

Focused bridge execution checks are hand-authored in
`tests/compiler_macros.rs::javascript_form_bridge`; their provenance and precise
scope are recorded in `tests/reference/compiler-macros.md`.

## Resource controls

Limits are configured before Context entry. Defaults are a 64 MiB allocation
ceiling, 1 MiB engine stack limit, one million cooperative polls, and a ten-second
session deadline. These are session-wide compilation limits, not reset per nested
call. Engine interrupts and Rust `ExecutionControl::poll` share fuel/deadline/
cancellation. Long Rust callbacks must poll cooperatively. A clone of the control
can cancel from another thread without accessing the engine.

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

`cargo test --locked --test evaluator` compares compiler-produced arithmetic,
collections, closures/apply, state, exceptions, lazy sequences, protocols, types,
records and interop against direct expected values in QuickJS and Node. It also
checks cross-session/released IDs, explicit capabilities, cancellation in caught
Rust callbacks, caught infinite work/allocation failure, panic containment,
cycles, fresh roots and repeatable live engine allocation counts.

`cargo test --locked --release --test evaluator startup_measurement -- --ignored --nocapture`
is a focused cold-session startup/heap measurement; it is deliberately not a
flaky timing gate. Existing CI runs the evaluator with all ordinary tests and
checks the all-features build. `node tests/support/js_runtime.test.js` exercises
the same canonical source and bootstrap hooks independently. See
[evaluator measurement record](../benchmarks/results/evaluator-2026-10-10.md).

Upstream implementation provenance: [rquickjs 0.14.0 source](https://docs.rs/crate/rquickjs/0.14.0/source/)
and [rquickjs-core 0.14.0 source](https://docs.rs/crate/rquickjs-core/0.14.0/source/),
especially synchronous runtime/context, allocator and function callback tracing.
No new dependencies or engine-selection experiment are prerequisites for the
remaining compiler integration.

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
and original locations. Compile-time emission uses the ordinary emitter with
root access hooks and never invokes an application optimizer. Ready root reads
stay in JS. Unrequested initializers drain in source order; standalone runtime
expressions are not replayed during macro setup. The warning API drains each
source form early to preserve diagnostics before later errors.

The unit cache is scoped to a fresh compilation. No application-source artifact
survives host reuse, so source, options, imports and selected conditional edits
cannot reuse stale IR. The separately cached bootstrap source is immutable and
built from this binary's embedded sources. Legacy explicit native macro imports
keep their existing eager adapter until light-cljs-115; they are not a fallback
for failed JavaScript compilation. General demand/overlay behavior and declaration
bundles remain separate migration work.

Execution evidence and the focused commands are recorded in
[deferred unit validation](../tests/reference/compiler-deferred-units.md).
