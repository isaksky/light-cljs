// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1942-1988,
// sloppy_if_clause_and_labelled_function_declarations
// (light-cljs-859). ECMAScript Annex B.3.4 and B.3.2: in sloppy code a plain
// function declaration may be an `if` clause (a block holding only it, so
// Annex B.3.3 also binds it with `var`) or labelled in statement-list
// position (a declaration of that list). Strict code, loop and `with`
// bodies, labelled `if` clauses and generator declarations keep the early
// error. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// `run` evaluates a row as eval code of a direct eval in a fresh function.
function run(source) { return eval(source); }
function syntaxError(source) {
  try { run(source); return false; } catch (e) { return e instanceof SyntaxError; }
}
// Script code. (The declaration is hoisted, as in function code; QuickJS
// hoists a labelled one in a script only when it is evaluated.)
outer859: inner859: function labelTop859() { return 1; }
row(labelTop859() === 1);
if (true) function ifTop859() { return 9; } else function elseTop859() {}
row(ifTop859() + typeof elseTop859 === "9undefined");
// Function and eval code.
row(run("if (true) function z() { return 9; } else function w() {} z() + typeof w") === "9undefined");
row(run("if (false) function z() {} typeof z") === "undefined");
row(run("function g() { var r = typeof h; if (true) function h() {} return r + typeof h; } g()") === "undefinedfunction");
row(run("l: function f() { return 1; } f()") === 1);
row(run("function g() { a: b: function f() { return 2; } return f(); } g()") === 2);
row(run("function g() { return f(); l: function f() { return 2; } } g()") === 2);
row(run("{ l: function f() { return 1; } } typeof f") === "function");
// Early errors that remain.
row(syntaxError("'use strict'; if (true) function z() {}"));
row(syntaxError("'use strict'; l: function f() {}"));
row(syntaxError("function g() { 'use strict'; if (true) function z() {} }"));
row(syntaxError("while (false) function f() {}"));
row(syntaxError("with ({}) function f() {}"));
row(syntaxError("if (true) l: function f() {}"));
row(syntaxError("l: function* g() {}"));
row(syntaxError("if (true) function* g() {}"));
failed === 0 ? rows : -failed;
