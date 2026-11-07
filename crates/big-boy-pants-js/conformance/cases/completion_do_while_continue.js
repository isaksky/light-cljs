// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9219-9240 (`script_completion_value_follows_javascript`), the rows
//   let i = 0; do { i = i + 1; if (i < 3) continue; 7; } while (i < 3)
//   5; do { 6; } while (false)
//   5; do { } while (false)
//   let i = 0; do i = i + 1; while (i < 3) 4
// Rule: `do`-`while` keeps the value the body produced last; the test's value is never the completion.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that took the test's value completes with false, one that reset per iteration with undefined.
// Completes with 7.
let i = 0; do { i = i + 1; if (i < 3) continue; 7; } while (i < 3)
