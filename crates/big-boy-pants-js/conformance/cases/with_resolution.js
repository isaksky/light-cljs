// bbp-authored conformance case (sloppy script).
// Derives from: rust/tests/js_source.rs lines 16224-16267
// with_statement_resolution (bbp-110), the rows without eval, the Function
// constructor, Object.create or symbol creation, and light-cljs-855. A
// `with` object is an object environment record (ECMAScript 9.1.1.2): a name
// resolves through it when HasProperty finds it, reads and writes go
// through [[Get]] and [[Set]] (getters, setters, inherited properties), a
// block declaration in the body shadows it, and a binding deleted between
// resolution and use is recreated by a sloppy store. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row((function () { var o = { x: 1 }; with (o) { x = 2; } return o.x === 2; })());
// A `var` in the body is hoisted to the function, its initializer stores
// through the object.
row((function () { var o = { x: 1 }; with (o) { var x = 2; } return o.x === 2 && x === undefined; })());
row((function () { var x = 1, o = { x: 2 }; with (o) { let x = 3; if (x !== 3) return false; } return x === 1 && o.x === 2; })());
row((function () { var o = { x: 2 }; with (o) { return typeof absent === "undefined" && typeof x === "number"; } })());
// Compound assignment and update through an inherited property create an
// own one.
row((function () { var p = { x: 2 }, o = { __proto__: p }; with (o) { x += 3; } return o.x === 5 && p.x === 2; })());
row((function () { var o = { x: 1 }; with (o) { x++; ++x; } return o.x === 3; })());
row((function () { var n = 0, o = { get x() { n++; return 3; } }; with (o) { return x === 3 && n === 1; } })());
row((function () { var n = 0, o = { set x(v) { n = v; } }; with (o) { x = 7; } return n === 7; })());
row((function () { var o = { get x() { throw 7; } }, caught; try { with (o) { x; } } catch (e) { caught = e; } return caught === 7; })());
// Nested objects and labelled breaks out of them.
row((function () { var x = 1; with ({ x: 2 }) { out: { with ({ x: 3 }) { break out; } } if (x !== 2) return false; } return x === 1; })());
row((function () { var x = 1, o1 = { x: 2, y: 2 }; with (o1) { with ({ x: 3 }) { return x === 3 && y === 2; } } })());
// The binding is chosen before the right-hand side runs.
row((function () { var o = { x: 1 }; with (o) { x = (delete o.x, 4); } return o.x === 4; })());
row((function () { var x = 1, o = {}; with (o) { x = (o.x = 3, 4); } return x === 4 && o.x === 3; })());
row((function () { var o = { x: 1 }; with (o) { delete x; } return !("x" in o); })());
// ToObject of the expression: null and undefined throw, a primitive is boxed.
row(name(function () { with (null) {} }) === "TypeError");
row(name(function () { with (undefined) {} }) === "TypeError");
row((function () { with ("abc") { return length === 3 && valueOf === String.prototype.valueOf; } })());
row((function () { with (5) { return toString === Number.prototype.toString; } })());
row((function () { with ({}) x: { break x; } return true; })());
// An absent name falls through to the enclosing scope and the global.
row((function () { var y = 5; with ({ x: 1 }) { return y === 5 && Array === globalThis.Array; } })());
row((function () { with ({ x: 1 }) { return name(function () { return absentName; }) === "ReferenceError"; } })());
// Script-level `var` inside `with` stores through the object too.
var o0 = { g: 1 };
with (o0) { var g = 2; }
row(o0.g === 2 && g === undefined);
failed === 0 ? rows : -failed;
