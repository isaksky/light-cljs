// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 518-520 (`script_completion_values_follow_try_statements`), the rows
//   1; try { 2; throw 0; } catch (e) {}
//   try { throw 0; } catch (e) { 5; }
// Rule: a `try` that throws completes with its catch body's value, undefined for an empty one.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that kept the try block's 2 or the 1 before completes with that.
// Completes with undefined.
1; try { 2; throw 0; } catch (e) {}
