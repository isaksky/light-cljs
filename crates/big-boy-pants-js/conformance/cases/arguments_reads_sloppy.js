// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 534-560,
// arguments_bindings_shadow_and_capture_in_sloppy_code (bbp-97): parameters,
// `let`, `var` and function declarations named `arguments` shadow the
// arguments object and are captured by arrows; plus js_source.rs
// rest_parameters_have_unmapped_arguments (line 58) `function f(...arguments)`
// and use_strict_directives (11439, bbp-95) `typeof arguments` at script
// level. Rows run in their own function rather than a block, so no sloppy
// block-level function declaration (Annex B, light-cljs-859) is involved;
// the script-level `function arguments` row runs one function deep. Left
// out: `function f(a=()=>arguments) { var arguments=7; return a; }
// f()().length===0`, false on QuickJS (true on the Pants engine and Goose).
// The rest-parameter row that writes `arguments[1]` is last (light-cljs-854;
// a rest parameter makes the object unmapped). Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row(typeof arguments === "undefined");
row((function () { function f(arguments) { return () => arguments; } return f(7)() === 7; })());
row((function () { function f() { let arguments = 7; return () => arguments; } return f(1)() === 7; })());
row((function () { function f() { function arguments() { return 7; } return arguments(); } return f(1) === 7; })());
row((function () { function f() { var arguments; return arguments.length === 2 && arguments[1] === 3; } return f(1, 3); })());
row((function () { function f() { var arguments = 7; return arguments; } return f(1) === 7; })());
row((function () { function f() { const g = () => arguments; arguments = 7; return g() === 7; } return f(1); })());
row((function () { function f() { const g = () => arguments; arguments = 2; arguments++; return g() === 3; } return f(1); })());
{
  let arguments = 7;
  const f = function () { return () => arguments; };
  row(f(1)()[0] === 1);
}
row((function () { function arguments() { return arguments.length; } return arguments(1, 2) === 2; })());
row((function () { function f(a = arguments[0]) { var arguments; return arguments.length === 1 && a === 4; } return f(4); })());
row((function () { function f(a = () => arguments) { let arguments = 7; return a; } return f()(4).length === 0; })());
row((function () { function f(a = () => arguments) { function arguments() {} return a; } return f()().length === 0; })());
row((function () { function f() { { let arguments = 7; if (arguments !== 7) return false; } return arguments[0] === 1; } return f(1); })());
row((function () { function f(...arguments) { return arguments[0] === 7; } return f(7); })());
row((function () { function f(a, ...r) { a = 8; arguments[1] = 9; return arguments[0] === 1 && r[0] === 2 && arguments.length === 3; } return f(1, 2, 3); })());
failed === 0 ? rows : -failed;
