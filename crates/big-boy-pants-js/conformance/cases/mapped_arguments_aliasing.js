// bbp-authored conformance case (sloppy script).
// Derives from: ~/src/quickjs/quickjs.c lines 16462-16520
// (js_build_mapped_arguments, revision 7d1c13c) and light-cljs-860's rows: in a sloppy
// function with a simple parameter list the arguments object is mapped
// (ECMAScript 10.4.4.7 CreateMappedArgumentsObject): while index i is mapped,
// `arguments[i]` and parameter i are one binding, for i below both the
// argument count and the parameter count. Rows: assignment both ways, extra
// arguments and missing ones, a duplicate parameter name (its last
// occurrence is mapped), closures over a parameter, compound assignment,
// the object passed to and applied by other functions, a function
// declaration and a `var` sharing a parameter's name, generators, direct
// eval, for-in, methods, functions compiled at run time (Function, indirect
// and direct eval), and the unmapped objects of strict functions and
// of default, rest and destructured parameter lists. QuickJS is the
// oracle; the Pants engine's arguments object is always unmapped
// (rust/docs/grammar.md). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row((function () { function f(a) { a = 2; return arguments[0]; } return f(1) === 2; })());
row((function () { function f(a) { arguments[0] = 2; return a; } return f(1) === 2; })());
row((function () { function f(a, b) { b = 2; var r = arguments[1]; arguments[1] = 3; return r + "," + b; } return f(1) === "undefined,2"; })());
row((function () { function f(a) { arguments[1] = 5; return a + "," + arguments[1] + "," + arguments.length; } return f(1, 2) === "1,5,2"; })());
row((function () { function f(a) { a = 3; return arguments[1] + "," + arguments[0]; } return f(1, 2) === "2,3"; })());
row((function () { function f(a, a) { a = 9; return arguments[0] + "," + arguments[1]; } return f(1, 2) === "1,9"; })());
row((function () { function f(a, a) { arguments[0] = 7; return a; } return f(1, 2) === 2; })());
row((function () { function f(a) { var g = function () { return a; }; arguments[0] = 5; return g(); } return f(1) === 5; })());
row((function () { function f(a) { var g = (v) => { a = v; }; g(4); return arguments[0]; } return f(1) === 4; })());
row((function () { function f(a) { return (() => { arguments[0] = 6; return a; })(); } return f(1) === 6; })());
row((function () { function f(a) { arguments[0] += 5; a *= 2; return arguments[0]; } return f(1) === 12; })());
row((function () { function f(a) { a++; arguments[0]++; return a; } return f(1) === 3; })());
row((function () { function set(args) { args[0] = "x"; } function f(a) { set(arguments); return a; } return f(1) === "x"; })());
row((function () { function g() { return arguments[0] + arguments[1]; } function f(a, b) { a = 10; return g.apply(null, arguments); } return f(1, 2) === 12; })());
row((function () { function f(a) { function a() {} return typeof arguments[0]; } return f(1) === "function"; })());
row((function () { function f(a) { var a = 4; return arguments[0]; } return f(1) === 4; })());
row((function () { function* g(a) { arguments[0] = 2; yield a; a = 3; yield arguments[0]; } var it = g(1); return it.next().value + "," + it.next().value === "2,3"; })());
row((function () { function f(a) { eval("a = 2"); return arguments[0]; } return f(1) === 2; })());
row((function () { function f(a) { eval("arguments[0] = 3"); return a; } return f(1) === 3; })());
row((function () { function f(a) { a = 5; var s = ""; for (var k in arguments) s += k + "=" + arguments[k] + ";"; return s; } return f(1, 2) === "0=5;1=2;"; })());
row((function () { var o = { m(a) { arguments[0] = 3; return a; } }; return o.m(1) === 3; })());
row((function () { return Function("a", "a = 2; return arguments[0];")(1) === 2; })());
row((function () { var f = (0, eval)("(function (a) { a = 2; return arguments[0]; })"); return f(1) === 2; })());
row((function () { var f = eval("(function (a) { arguments[0] = 3; return a; })"); return f(1) === 3; })());
row((function () { function f(a) { a = 2; return arguments.callee === f && arguments.length === 1; } return f(1); })());
row((function () { function f(a) { a = 2; return Array.prototype.join.call(arguments); } return f(1, 7) === "2,7"; })());
// Unmapped: strict functions and non-simple parameter lists.
row((function () { function f(a) { "use strict"; a = 2; return arguments[0]; } return f(1) === 1; })());
row((function () { function f(a, b = 0) { a = 2; return arguments[0]; } return f(1) === 1; })());
row((function () { function f(a, ...r) { a = 2; return arguments[0]; } return f(1) === 1; })());
row((function () { function f({ x }, a) { a = 2; return arguments[1]; } return f({}, 1) === 1; })());
row((function () { function f(a) { "use strict"; arguments[0] = 2; return a; } return f(1) === 1; })());
failed === 0 ? rows : -failed;
