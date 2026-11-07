// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 528-528 (`script_completion_values_follow_try_statements`), the rows
//   7; try {} finally { 3; }
// Rule: an empty `try` block completes with undefined, which the finally block leaves alone.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the 7 or took the 3 completes with that.
// Completes with undefined.
7; try {} finally { 3; }
