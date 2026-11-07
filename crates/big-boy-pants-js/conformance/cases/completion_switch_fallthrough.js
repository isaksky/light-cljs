// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9300-9305 (`script_completion_value_follows_javascript`), the rows
//   5; switch (2) { case 1: 6; default: 7; case 2: 8; }
//   switch (1) { case 1: 7; case 2: 8; }
//   5; switch (3) { case 1: 6; }
// Rule: `switch` completes with the value of the clauses that ran, from the matching one on.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the value before the `switch` completes with 5, one that chose `default` and stopped at it with 7.
// Completes with 8.
5; switch (2) { case 1: 6; default: 7; case 2: 8; }
