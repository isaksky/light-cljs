// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9293-9296 (`script_completion_value_follows_javascript`), the rows
//   5; L: while (true) { 6; while (true) { break L; } }
//   5; L: while (true) { 6; { break L; } }
// Rule: `break L` out of an inner loop carries that loop's undefined, not the 6.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the 6 completes with 6.
// Completes with undefined.
5; L: while (true) { 6; while (true) { break L; } }
