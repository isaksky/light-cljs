// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9230-9234 (`script_completion_value_follows_javascript`), the rows
//   5; for (let k in {a: 1, b: 2}) { if (k === 'b') continue; 6; }
//   5; for (let k in {a: 1}) {}
//   5; for (let k in {a: 1, b: 2}) 6;
// Rule: for-in resets to undefined; an `if` around the last iteration's `continue` makes it undefined.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the first iteration's 6 completes with 6.
// Completes with undefined.
5; for (let k in {a: 1, b: 2}) { if (k === 'b') continue; 6; }
