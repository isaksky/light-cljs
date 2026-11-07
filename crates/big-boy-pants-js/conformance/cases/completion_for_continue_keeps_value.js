// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9253-9272 (`script_completion_value_follows_javascript`), the rows
//   for (let i = 0; i < 3; i = i + 1) { 9; continue; }
//   5; for (;;) { 6; break; }
//   for (let i = 0; i < 3; i = i + 1) { i * 2; }
// Rule: `continue` carries the body's value into the next iteration.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that dropped it completes with undefined.
// Completes with 9.
for (let i = 0; i < 3; i = i + 1) { 9; continue; }
