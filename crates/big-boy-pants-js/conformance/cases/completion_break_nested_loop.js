// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9178-9194 (`script_completion_value_follows_javascript`), the rows
//   3; while (true) { 4; while (true) { break; } break; }
//   let i = 0; while (true) { i = i + 1; if (i > 2) break; }
// Rule: the inner loop's own undefined replaces the 4 before the outer `break` carries it.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that treated the inner loop as empty completes with 4.
// Completes with undefined.
3; while (true) { 4; while (true) { break; } break; }
