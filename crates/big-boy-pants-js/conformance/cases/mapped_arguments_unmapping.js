// bbp-authored conformance case (sloppy script).
// Derives from: ~/src/quickjs/quickjs.c lines 9480-9500 (delete_property's
// fast-array case; convert_fast_array_to_array, lines 9353-9395, keeps each
// index's JS_PROP_VARREF; revision 7d1c13c), ECMAScript 10.4.4.5 (the
// arguments exotic object's [[Delete]]) and light-cljs-860's rows: deleting a mapped
// index unmaps it, the parameter keeping its value, and a property added
// again at that index is an ordinary one; deleting one index leaves the
// others mapped, and a `length` write unmaps nothing. Left out until the
// objects area (light-cljs-766) brings Object.defineProperty to Goose: a
// value redefinition that writes through and keeps the link, and a
// `writable: false` or accessor redefinition that unmaps (light-cljs-860
// lists those rows). QuickJS is the oracle; the Pants engine's arguments
// object is always unmapped (rust/docs/grammar.md). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row((function () { function f(a) { delete arguments[0]; a = 5; var r = arguments[0]; arguments[0] = 6; return r + "," + a; } return f(1) === "undefined,5"; })());
row((function () { function f(a) { delete arguments[0]; arguments[0] = 6; a = 7; return arguments[0] + "," + a; } return f(1) === "6,7"; })());
row((function () { function f(a) { arguments[0] = 3; delete arguments[0]; return a; } return f(1) === 3; })());
row((function () { function f(a) { a = 4; return delete arguments[0] && !("0" in arguments) && a === 4; } return f(1); })());
row((function () { function f(a, b) { delete arguments[1]; a = "x"; b = "y"; return arguments[0] + "," + arguments[1]; } return f(1, 2) === "x,undefined"; })());
row((function () { function f(a, b) { delete arguments[0]; arguments[1] = "z"; return a + "," + b; } return f(1, 2) === "1,z"; })());
row((function () { function f(a) { arguments.length = 0; a = 2; return arguments[0]; } return f(1) === 2; })());
row((function () { function f(a) { var g = () => a; delete arguments[0]; arguments[0] = 8; return g(); } return f(1) === 1; })());
row((function () { function f(a) { delete arguments[0]; delete arguments[0]; a = 3; return arguments.length + "," + arguments[0]; } return f(1) === "1,undefined"; })());
row((function () { function f(a) { eval("delete arguments[0]"); a = 9; return arguments[0]; } return f(1) === undefined; })());
failed === 0 ? rows : -failed;
