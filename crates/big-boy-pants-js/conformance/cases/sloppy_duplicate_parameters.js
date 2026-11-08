// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 1989-2023,
// sloppy_duplicate_parameter_names (light-cljs-859), its rows on
// which QuickJS agrees. A sloppy function's simple parameter list may repeat
// a name (ECMAScript 15.2.1 early errors), and the last occurrence is the
// binding; strict code, arrows, methods and non-simple lists keep the early
// error. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function syntaxError(source) {
  try { (0, eval)(source); return false; } catch (e) { return e instanceof SyntaxError; }
}
function pair(a, a) { return a; }
row(pair(1, 2) === 2);
row(pair.length === 2);
function count(a, a) { return arguments[0] + "," + arguments.length; }
row(count(1, 2) === "1,2");
function missing(a, b, a) { return a + b; }
row(missing(1, 2) !== missing(1, 2));
function three(a, b, a) { return [a, b, arguments.length].join(); }
row(three(1, 2, 3) === "3,2,3");
function captured(a, a) { var c = function () { return a; }; return c(); }
row(captured(1, 2) === 2);
function* generator(a, a) { yield a; }
row(generator(1, 2).next().value === 2);
row(Function("a", "a", "return a")(1, 2) === 2);
// Early errors that remain.
row(syntaxError("function f(a, a) { 'use strict'; }"));
row(syntaxError("'use strict'; function f(a, a) {}"));
row(syntaxError("function f(a, a = 1) {}"));
row(syntaxError("function f(a, [a]) {}"));
row(syntaxError("function f({a}, a) {}"));
row(syntaxError("function f(a, ...a) {}"));
row(syntaxError("((a, a) => a)"));
row(syntaxError("({ m(a, a) {} })"));
failed === 0 ? rows : -failed;
