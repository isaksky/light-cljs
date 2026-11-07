// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9176-9190
// (`script_completion_value_follows_javascript`), with a generator in place
// of an array (array iterators are light-cljs-844).
// Rule: a for-of loop whose iterator is exhausted at once completes with
// undefined (ForIn/OfBodyEvaluation starts V at undefined), replacing the
// value the statements before it produced: an engine that treated the loop
// as an empty completion would complete with 7.
// Completes with undefined.
7; function* g() {} for (const x of g()) x;
