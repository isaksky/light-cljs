// bbp-authored conformance case (sloppy script: areas.edn :mode :sloppy).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9266-9266 (`script_completion_value_follows_javascript`), the rows
//   5; for (var k = 2 in {x:1, y:2}) 7;
// Rule: an Annex B `var` initializer in a for-in head has no completion value; the body's 7 is kept.
// Only the script's own completion is observable without eval, so this
// slice is the one row an engine that ignored the rule would get wrong:
// an engine that let the initializer set it, or never ran the body, completes with 2 or 5.
// Completes with 7.
5; for (var k = 2 in {x:1, y:2}) 7;
