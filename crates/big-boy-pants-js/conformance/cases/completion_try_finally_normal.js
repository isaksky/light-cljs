// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 517-525 (`script_completion_values_follow_try_statements`), the rows
//   try { 1; } catch (e) { 2; } finally { 3; if (true) { 4; } }
//   try { 1; } finally { 2; }
//   try { 1; } catch (e) { 2; }
// Rule: a normally completing `finally` block never changes the value.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that let the finally block's value through completes with 4.
// Completes with 1.
try { 1; } catch (e) { 2; } finally { 3; if (true) { 4; } }
