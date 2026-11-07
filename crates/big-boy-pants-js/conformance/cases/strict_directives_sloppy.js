// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 11439-11538,
// use_strict_directives (bbp-95): a sloppy script with one strict function
// and an arrow nested in it; a "use strict" function rejects an assignment to
// an undeclared name while the script does not; sloppy bindings named `eval`
// and `arguments`; the global `eval` as a value. Rows that declare functions
// run in their own function rather than a block, so no sloppy block-level
// function declaration (Annex B, light-cljs-859) is involved; the
// script-level `function eval` row runs one function deep, so the script
// keeps the global eval. Left out: the Pants executable's strictness flags
// and byte-format round trip (Pants API); the early errors of strict code
// (`function f(eval) {}`, `let arguments`); the directive completion values
// (use_strict_completion_*); `typeof arguments` at script level
// (arguments_reads_sloppy, as this script declares a global `arguments`).
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function sloppy(eval) { var arguments = 2; return 2; }
function strict(a) { "use strict"; return () => a; }
row(strict(1)() + sloppy(0) === 3);
// The directive applies to the function and its nested arrow; the script
// stays sloppy.
function strictAssign() { "use strict"; return () => { strictUndeclared = 1; }; }
row(name(strictAssign()) === "ReferenceError" && typeof strictUndeclared === "undefined");
sloppyUndeclared = 1;
row(sloppyUndeclared === 1);
function sloppyEntry(eval) { return 1; }
row(sloppyEntry() === 1);
// Sloppy bindings named `eval` and `arguments` are ordinary bindings.
{ let eval = 1; eval = 2; row(eval === 2); }
function argumentsParameter(arguments) { arguments = 3; return arguments; }
row(argumentsParameter(1) === 3);
function evalParameter(eval) { eval += 1; eval++; return eval; }
row(evalParameter(1) === 3);
row((function () { function eval() { return 4; } return eval(); })() === 4);
var arguments = 2;
row(arguments === 2);
function letArguments() { let arguments = 5; return () => arguments; }
row(letArguments()() === 5);
{ let arguments = 6; row((() => arguments)() === 6); }
function functionArguments() { function arguments() { return 7; } return arguments(); }
row(functionArguments() === 7);
function blockLetArguments() { { let arguments = 8; return arguments; } }
row(blockLetArguments() === 8);
{ let r; try { throw 9; } catch (eval) { r = eval; } row(r === 9); }
// The global eval is a function value.
evalCopy = eval;
row(typeof evalCopy === "function");
failed === 0 ? rows : -failed;
