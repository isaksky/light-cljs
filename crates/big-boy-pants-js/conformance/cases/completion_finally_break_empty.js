// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9209-9213 (`script_completion_value_follows_javascript`), the rows
//   while (true) { try { 6; break; } finally { break; } }
//   let i = 0; while (i < 2) { i = i + 1; try { 6; } finally { continue; } }
// Rule: a `break` out of a `finally` block that produced nothing replaces the try block's value.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the try block's 6 completes with 6.
// Completes with undefined.
while (true) { try { 6; break; } finally { break; } }
