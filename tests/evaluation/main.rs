//! Evaluator, `EvalSession`, `--eval`/`--repl` and SCI-reference integration tests,
//! linked into a single test binary. Add a suite as `tests/evaluation/<name>.rs` and
//! register it below.

// Some imports, helpers and inputs are used only by the V8 checks.
#![cfg_attr(
    not(light_v8),
    allow(unused_imports, unused_variables, unused_mut, unused_macros, dead_code)
)]

// `row_tests!` declares one test per fixture row, row range or share of the rows.
#[macro_use]
#[path = "../support/row_tests.rs"]
mod row_tests;

// The SCI wire protocol is shared by several suites; load it once.
#[path = "../support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

// Temporary directories removed when their test ends, however it ends.
#[path = "../support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

// The Node driver of dev builds (tests/support/dev_execution.mjs).
#[path = "../support/dev_execution.rs"]
#[allow(dead_code)]
mod dev_execution;

mod clojure_fallback;
#[cfg(light_v8)]
mod dev_session;
mod eval_cli;
mod eval_examples;
mod eval_nsstar;
#[cfg(light_v8)]
mod eval_sessions;
#[cfg(light_v8)]
mod evaluator;
mod evaluator_conformance;
#[cfg(light_v8)]
mod evaluator_migration;
mod sci_reference;
