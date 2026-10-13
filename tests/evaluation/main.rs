//! Evaluator, `EvalSession`, `--eval`/`--repl` and SCI-reference integration tests,
//! linked into a single test binary. Add a suite as `tests/evaluation/<name>.rs` and
//! register it below.

// The SCI wire protocol is shared by several suites; load it once.
#[path = "../support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

mod eval_cli;
mod eval_examples;
mod eval_nsstar;
mod eval_sessions;
mod evaluator;
mod evaluator_conformance;
mod evaluator_migration;
mod sci_reference;
