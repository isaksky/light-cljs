// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9254-9269 (`script_completion_value_follows_javascript`), the rows
//   let i; 5; for (i = 0; i < 2; i = i + 1) {}
//   5; for (let i = 0; i < 0; ) {}
//   5; for (var i = 0; i < 0; ) {}
// Rule: a `for` head's init, test and update never set the value.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that let the update set it completes with 2, one that skipped the reset with 5.
// Completes with undefined.
let i; 5; for (i = 0; i < 2; i = i + 1) {}
