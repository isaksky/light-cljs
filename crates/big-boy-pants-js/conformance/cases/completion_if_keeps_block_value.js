// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9151-9157 (`script_completion_value_follows_javascript`), the rows
//   if (true) { 3; let y; }
//   1; let x = 2;
//   1; {}
//   1; ;
//   if (false) 1; else 2;
// Rule: a declaration completes empty, so the block keeps the 3.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that let the declaration produce a value completes with undefined.
// Completes with 3.
if (true) { 3; let y; }
