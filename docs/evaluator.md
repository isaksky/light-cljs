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
failure never falls back to native evaluation. The later reentry boundary belongs
to light-cljs-109; callbacks here do not reenter `Context::with`.

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
control reference. The forthcoming structured compiler-form bridge does not use
this string API as a form encoding. No promises are pumped or awaited.

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
