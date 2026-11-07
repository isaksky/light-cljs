// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9286-9289 (`script_completion_value_follows_javascript`), the rows
//   5; L: { 6; break L; }
//   5; L: { break L; }
//   5; L: 7;
// Rule: `break L` out of a labelled block keeps the value produced so far.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that dropped it completes with 5 or undefined.
// Completes with 6.
5; L: { 6; break L; }
