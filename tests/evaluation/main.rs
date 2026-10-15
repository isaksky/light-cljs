//! Evaluator, `EvalSession`, `--eval`/`--repl` and SCI-reference integration tests,
//! linked into a single test binary. Add a suite as `tests/evaluation/<name>.rs` and
//! register it below.

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

mod eval_cli;
mod eval_examples;
mod eval_nsstar;
mod eval_sessions;
mod evaluator;
mod evaluator_conformance;
mod evaluator_migration;
mod sci_reference;
