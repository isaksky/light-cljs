// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9159-9171 (`script_completion_value_follows_javascript`), the rows
//   let i = 0; while (i < 2) { i = i + 1; if (i > 5) { 9; } }
//   let i = 0; while (i < 3) { i = i + 1; }
// Rule: an `if` that runs no branch completes with undefined and replaces the body's earlier value.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the assignment's value completes with 2.
// Completes with undefined.
let i = 0; while (i < 2) { i = i + 1; if (i > 5) { 9; } }
