// bbp-authored conformance case (sloppy script).
// Derives from: ~/src/quickjs/quickjs.c lines 9480-9500 (delete_property's
// fast-array case; convert_fast_array_to_array, lines 9353-9395, keeps each
// index's JS_PROP_VARREF; revision 7d1c13c), ECMAScript 10.4.4.5 (the
// arguments exotic object's [[Delete]]) and light-cljs-860's rows: deleting a mapped
// index unmaps it, the parameter keeping its value, and a property added
// again at that index is an ordinary one; deleting one index leaves the
// others mapped, and a `length` write unmaps nothing. Light-cljs-868 added
// light-cljs-860's Object.defineProperty rows (ECMAScript 10.4.4.2, the
// arguments exotic object's [[DefineOwnProperty]]): a value redefinition
// that writes through and keeps the link, a `writable: false` or accessor
// redefinition that unmaps, and an `enumerable: false` redefinition that
// keeps it. QuickJS is the oracle; the Pants engine's arguments
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
// Object.defineProperty on a mapped index (ECMAScript 10.4.4.2): a value
// redefinition writes through and keeps the link; a non-writable one unmaps
// after writing its value through; an accessor one unmaps, the parameter
// keeping its old value; an `enumerable: false` one keeps the link while
// the property leaves the element run, and a later delete still unmaps.
row((function () { function f(a) { Object.defineProperty(arguments, '0', { value: 5 }); var r = a; a = 7; return r + ',' + arguments[0]; } return f(1) === "5,7"; })());
row((function () { function f(a) { Object.defineProperty(arguments, '0', { writable: false }); a = 7; return arguments[0] + ',' + a; } return f(1) === "1,7"; })());
row((function () { function f(a) { Object.defineProperty(arguments, '0', { value: 3, writable: false }); var r = a; a = 7; return r + ',' + arguments[0] + ',' + a; } return f(1) === "3,3,7"; })());
row((function () { function f(a) { a = 2; Object.defineProperty(arguments, '0', { get: function () { return 'g'; } }); var r = a; a = 7; return r + ',' + arguments[0] + ',' + a; } return f(1) === "2,g,7"; })());
row((function () { function f(a) { Object.defineProperty(arguments, '0', { enumerable: false }); a = 7; var r = arguments[0]; arguments[0] = 8; return r + ',' + a + ',' + Object.keys(arguments).length; } return f(1) === "7,8,0"; })());
row((function () { function f(a, b) { Object.defineProperty(arguments, '1', { enumerable: false }); delete arguments[1]; b = 9; return arguments[1] + ',' + a + ',' + b; } return f(1, 2) === "undefined,1,9"; })());
failed === 0 ? rows : -failed;
