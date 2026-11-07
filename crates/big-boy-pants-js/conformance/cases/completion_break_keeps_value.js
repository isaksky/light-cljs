// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9176-9190 (`script_completion_value_follows_javascript`), the rows
//   5; while (true) { 6; { break; } }
//   while (true) { 1; try { 2; break; } catch (e) {} }
//   let i = 0; while (i < 3) { i = i + 1; continue; 7; }
// Rule: `break` carries the value its statement list produced so far.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that dropped it completes with undefined or 5.
// Completes with 6.
5; while (true) { 6; { break; } }
