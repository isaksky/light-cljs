// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9154-9155 (`script_completion_value_follows_javascript`), the rows
//   1; if (true) {}
//   1; if (false) 2;
// Rule: `if` replaces an empty branch completion with undefined (UpdateEmpty).
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that let the empty block keep the value before it completes with 1.
// Completes with undefined.
1; if (true) {}
