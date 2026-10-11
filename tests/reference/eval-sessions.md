# Compiler-backed embedding execution

`tests/eval_sessions.rs` exercises `EvalSession` directly. Every result passes
through the Rust compiler and canonical JavaScript runtime in embedded QuickJS.
There is no native evaluation or alternate implementation in this suite.

The ten unchanged embedding evaluation examples are copied from
`tests/fixtures/sci-outcomes.tsv` (`sci-embedding-eval-*` and
`sci-embedding-context-redefinition`) with direct numeric expected results.
The eleventh fixture deliberately changes native ratio division/catch to
JavaScript Number infinity and a CLJS `:default` catch. This is an explicit
numeric/host adaptation; it is not reported as unchanged SCI parity.

Other cases are local acceptance fixtures adapted from the behavior covered by
`interpreter_embedding`, `interpreter_reuse`, `interpreter_limits` and
`interpreter_reclamation`: persistent definitions and saved functions, typed
callbacks and recursive invocation, arity/error provenance, dynamic restoration,
input/output/arguments, source namespaces and aliases, analysis/form APIs,
compiler policy, reader handlers, cancellation/reset, dropped/foreign IDs,
cycles and callback capture reclamation. Exact native collector counters,
shared-owner forks, class descriptors and exact integer/ratio semantics are
explicitly retired in [the migration map](../../docs/evaluation.md).

The suite also executes transduction and bundled string/set/walk/reader operations
with direct expected values, independent session protocol tables, compile-time
versus application atoms (`[1 2 0]`), repeated clean compilations on one `MacroHost`,
macro errors followed by successful compilation, runtime compiler `eval`, and
idle interactive handles. The compiler unit tests cover namespace snapshots,
macro failure recovery, default-reader precedence, and bounded retained arenas.

Verification commands and outcomes for light-cljs-112 are recorded below after
execution. Existing opt-in oracle/performance tests are not evidence of newly
executed checks merely because their fixtures are present.

## Executed checks

- `cargo test --locked --all-features --test eval_sessions --test evaluator --test interpreter_embedding --test interpreter_reuse --test interpreter_limits --test interpreter_reclamation --test boundaries --test compiler_deferred`: **135 passed**, one existing manual startup measurement ignored. The native suites verify the temporary adapter; the 20 session tests verify the replacement path.
- `cargo test --locked --all-features --test eval_sessions`: **20 passed** after explicitly disabling native evaluation on every session compiler host.
- `cargo test --locked --all-features --test eval_sessions --test evaluator`: **29 passed**, one existing measurement ignored, after the final handle-validation and error-data changes.
- `cargo test --locked --test eval_sessions --test evaluator --test analyzer`: **64 passed**, one existing measurement ignored. Analyzer expectations now include the already delivered transducer core IDs.
- `cargo test --locked --test eval_sessions --test evaluator --test compiler_data_libraries --test compiler_transducer_contracts --test compiler_transducers`: passed; the library suite had **8**, contract suite **6**, and transducer fixture suite **2** passing tests. The external pinned-oracle run and startup measurement remained explicitly ignored.
- `cargo test -q eval_compiler::tests --lib`: **4 passed**; `cargo test -q compiler_default_reader_tests --lib`: **1 passed**.
- `cargo check --locked --all-features`: passed.
- `node tests/support/js_runtime.test.js`: passed all canonical runtime assertion groups.
- `cargo run --locked --example embedding`: passed, reporting `Hello, Rust`, a located host type error, retained calls `2`, and evaluated form `3`.
- `git diff --check`: passed.

`cargo test --locked --all-features --no-fail-fast` was also attempted. It
completed the compiler suites through the evaluator tests, exposing two obsolete
macro-source fixtures; both were migrated and their focused reruns passed. The
run then aborted in
`execution::deeply_nested_accepted_source_compiles_and_executes` with a Rust stack
overflow. An isolated `git archive` of starting commit
`df98d8f5409ca002d17430cdc1990d588bebbcd1` reproduced the identical failure with
`cargo test --all-features --test execution deeply_nested_accepted_source_compiles_and_executes -- --exact --nocapture`
and default stack settings. This pre-existing failure is already tracked by
**light-cljs-129**. The macOS crashed process did not exit normally, so that
broad invocation was stopped; later full-suite targets were not executed by it.
The separately listed focused suites did execute. No full-suite success,
external oracle rerun, platform-specific run or new benchmark result is claimed.
