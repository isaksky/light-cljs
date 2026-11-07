// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9158-9166 (`script_completion_value_follows_javascript`), the rows
//   let i = 0; 5; while (i < 3) { let j = (i = i + 1); }
//   1; while (false) {}
// Rule: a loop resets the value to undefined on entry; a body that never produces one leaves it undefined.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the value before the loop completes with 5.
// Completes with undefined.
let i = 0; 5; while (i < 3) { let j = (i = i + 1); }
