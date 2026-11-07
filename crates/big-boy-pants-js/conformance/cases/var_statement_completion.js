// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 11944-11945,
// var_declarations_hoist_to_the_function (bbp-94): a `var` statement has no
// completion value, so the script completes with the earlier statement's 1.
1; var w = 2;
