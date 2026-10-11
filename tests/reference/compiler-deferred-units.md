# Reentrant deferred compiler units

Implemented and validated 2026-10-10 against starting commit `5a1ff32`.
This is execution evidence for the ordinary initializer/compiler reentry seam,
not a claim that general demand overlays or declaration bundles are complete.

The six `reentrant_deferred_units` tests in `tests/compiler_macros.rs` execute
compiler-emitted applications in Node, with native evaluation disabled during
compilation. They cover:

- Macro → raw helper → visible macro → another raw helper reentry, exact
  initializer/expansion effect traces, repeated calls and reuse of expanded IR
  by application emission.
- Definition-site visible bindings, forbidden undeclared forward names, legal
  declares, macro redefinition snapshots and active ordinary generations.
- Ordinary function recursion versus an analyzing-unit dependency cycle with
  the helper path in its diagnostic.
- Shadowed namespace aliases, Var metadata, source/import/conditional edits and
  changed assertion options across clean compilations on one configured host.
- Failed analysis, nested expansion, malformed emitted JS installation,
  initializer execution, unsupported result conversion and recovery on reuse.
- Terminal nested execution limits and a subsequent clean successful compile.

Direct executing checks use a 20-second receive timeout. The focused suite was
also launched through Python `subprocess.run(..., timeout=120)`; nested evaluator
callbacks reuse the supplied `Ctx` rather than acquiring `Context::with` again.

## Commands and outcomes

- `cargo test --test compiler_macros --test compiler_warnings --test analyzer --test evaluator --test compiler_bootstrap --test compiler_boundary_completion --test compiler_namespace_completion --quiet`: **157 passed, 1 ignored** (the explicitly opt-in evaluator startup measurement).
- `cargo test --lib active_tests --quiet`: **4 passed**, including recursive
  same-context service calls, structured bridge operations, sticky cancellation,
  service failure and foreign-context rejection.
- `cargo test --lib program_fragment --quiet`: **3 passed**, including emitted
  application execution, metadata remapping and compact resolution snapshots.
- `cargo test --test compiler_language --quiet`: **1 passed**.
- `cargo test --test execution compiled_definitions_preserve_namespace_macro_and_callback_scopes --quiet`: **1 passed**.
- Final targeted rerun: `cargo test --test compiler_macros reentrant_deferred_units --quiet`
  (**6 passed**) and `cargo test --test evaluator unit_runtime_parameter_preserves_raw_javascript_hygiene --quiet`
  (**1 passed**).
- `cargo test --test compiler_warnings --quiet`: final **11 passed**, including
  a warning-mode forward placeholder resolving to the later definition's one
  stable global identity at runtime.
- `cargo check --all-features --quiet`: passed.
- `git diff --check`: passed.

`cargo test --test execution tiny_programs_execute_with_expected_values_and_effects --quiet`
failed at the existing `state-volatile-order` fixture: actual `[8 8 10]` with one
target evaluation versus retained `[17 17 10]` with two. An untouched `git archive
5a1ff32` built in a temporary directory reproduced `[8 8 10 1]` for the reduced
volatile/target-count program. This fixture has no deferred definitions. Follow-up
`light-cljs-126` records the source, traces and required bootstrap/reference audit.

The explicitly pinned JVM oracle command
`cargo test --test execution compiled_definitions_match_pinned_values_effects_and_stdout --quiet`
selected one ignored test and executed no oracle comparison. The opt-in startup
measurement, full JVM/reference suites and platform-specific suites were not run.
