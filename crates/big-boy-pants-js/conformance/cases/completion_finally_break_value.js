// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9201-9205 (`script_completion_value_follows_javascript`), the rows
//   while (true) { try { throw 1; } finally { 8; break; } }
//   5; while (true) { try { 6; break; } finally { 7; } }
// Rule: a jump out of a `finally` block carries the value that block produced.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that ignored it completes with undefined.
// Completes with 8.
while (true) { try { throw 1; } finally { 8; break; } }
