// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9320-9320 (`debugger_statement_completes_empty`), the rows
//   5; debugger;
// Rule: `debugger` with no debugger attached completes empty (ECMAScript 14.16), keeping the value before it.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that gave it undefined completes with undefined; one that rejected it fails with a SyntaxError.
// Completes with 5.
5; debugger;
