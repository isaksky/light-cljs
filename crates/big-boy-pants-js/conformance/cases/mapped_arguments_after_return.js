// bbp-authored conformance case (sloppy script).
// Derives from: ~/src/quickjs/quickjs.c lines 16437-16458 (the mapped
// arguments object marks and frees its parameters' variable references,
// which outlive the call; revision 7d1c13c), ECMAScript 10.4.4.7 (the
// map's getters and setters close over the parameters' bindings) and
// light-cljs-860's rows: a mapped arguments object that
// outlives its call still aliases the parameter a closure sees, both ways;
// each call's object aliases its own parameters; a deleted index stays
// unmapped; a suspended generator's object aliases its parameters across
// a yield. QuickJS is the oracle; the Pants engine's arguments object is
// always unmapped (rust/docs/grammar.md). Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function pair(a) { return [() => a, arguments, (v) => { a = v; }]; }
row((function () { var p = pair(1); p[1][0] = 9; return p[0]() === 9; })());
row((function () { var p = pair(1); p[2](4); return p[1][0] === 4; })());
row((function () { var p = pair(1), q = pair(2); p[1][0] = 7; return p[0]() + "," + q[0]() === "7,2"; })());
row((function () { var p = pair(1); delete p[1][0]; p[1][0] = 3; return p[0]() === 1; })());
row((function () { var p = pair(1, 2); p[1][1] = 5; p[2](6); return p[1][0] + "," + p[1][1] + "," + p[0]() === "6,5,6"; })());
row((function () { function f(a) { a = 1; return arguments; } var x = f(0); x[0] = 5; return x[0] === 5 && x.length === 1; })());
row((function () { function* g(a) { yield arguments; yield a; } var it = g(1); var args = it.next().value; args[0] = 8; return it.next().value === 8; })());
row((function () { var ps = []; for (var i = 0; i < 50; i++) ps.push(pair(i)); for (var j = 0; j < 50; j++) ps[j][1][0] += 100; var s = 0; for (var k = 0; k < 50; k++) s += ps[k][0](); return s === 6225; })());
failed === 0 ? rows : -failed;
