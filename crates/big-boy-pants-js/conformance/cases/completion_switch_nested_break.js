// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9301-9309 (`script_completion_value_follows_javascript`), the rows
//   5; switch (1) { case 1: 6; switch (2) { case 2: break; } }
//   5; switch (1) { case 1: }
// Rule: an inner `switch` whose clause produced nothing completes with undefined.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the 6 completes with 6.
// Completes with undefined.
5; switch (1) { case 1: 6; switch (2) { case 2: break; } }
