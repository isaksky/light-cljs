// bbp-authored conformance case (sloppy script).
// Derives from: rust/tests/js_source.rs lines 16224-16267 and 16371-16381
// with_statement_resolution (the Symbol.unscopables rows) and
// with_statement_array_unscopables, and light-cljs-855. HasBinding of an
// object environment (ECMAScript 9.1.1.2.1) reads @@unscopables only after
// HasProperty finds the name, and only an object value's truthy entry hides
// the name; a getter on either object runs each time. Array.prototype's
// @@unscopables (23.1.3.41) is a null-prototype object of sixteen `true`
// names, its property neither writable nor enumerable. `Object.keys` is
// not implemented yet, so a for-in counts its names. A computed-key getter
// gets its name from the key (SetComputedName). Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row((function () { var x = 1, o = { x: 2, [Symbol.unscopables]: { x: true } }; with (o) { x = 3; } return x === 3 && o.x === 2; })());
row((function () { var x = 1, o = { x: 2, [Symbol.unscopables]: { x: false } }; with (o) { x++; } return x === 1 && o.x === 3; })());
row((function () { var x = 1, o = { x: 2, [Symbol.unscopables]: { x: 0, y: "yes" } }; with (o) { return x === 2; } })());
// A value that is not an object hides nothing.
row((function () { var o = { x: 2, [Symbol.unscopables]: true }; with (o) { return x === 2; } })());
// HasProperty first, then the @@unscopables getter, then the entry's getter.
row((function () {
  var x = 1, n = 0, log = "";
  var o = { x: 2, get [Symbol.unscopables]() { n++; log += "u"; return { get x() { n++; log += "x"; return true; } }; } };
  with (o) { if (x !== 1) return false; }
  return n === 2 && log === "ux";
})());
row((function () { var n = 0, o = { get [Symbol.unscopables]() { n++; return {}; } }; with (o) { typeof absent; } return n === 0; })());
row((function () { var o = { get [Symbol.unscopables]() { throw 7; }, x: 2 }, caught; try { with (o) { x; } } catch (e) { caught = e; } return caught === 7; })());
// An @@unscopables written by the right-hand side does not move the store.
row((function () { var x = 1, o = { x: 2 }; with (o) { x = (o[Symbol.unscopables] = { x: true }, 4); } return x === 1 && o.x === 4; })());
// Inherited @@unscopables counts.
row((function () { var x = 1, o = { __proto__: { [Symbol.unscopables]: { x: true } }, x: 2 }; with (o) { return x === 1; } })());
row((function () {
  var o = { get [Symbol.unscopables]() { return {}; } };
  var d = Object.getOwnPropertyDescriptor(o, Symbol.unscopables);
  return d.get.name === "get [Symbol.unscopables]";
})());
row((function () { var o = { [Symbol.iterator]() {}, ["a" + 1]() {}, [2]() {} }; return o[Symbol.iterator].name === "[Symbol.iterator]" && o.a1.name === "a1" && o[2].name === "2"; })());
// Array.prototype[@@unscopables].
row((function () { var values = 42; with ([]) { return values === 42 && typeof keys === "undefined"; } })());
row((function () { var values = 42, a = []; a.values = 3; with (a) { return values === 42; } })());
row((function () { with ([]) { return typeof values === "undefined" && typeof push === "function" && length === 0; } })());
row((function () {
  var d = Object.getOwnPropertyDescriptor(Array.prototype, Symbol.unscopables);
  var u = d.value, n = 0, all = true;
  for (var k in u) { n++; if (u[k] !== true) all = false; }
  return !d.writable && !d.enumerable && d.configurable && Object.getPrototypeOf(u) === null && n === 16 && all;
})());
row((function () {
  var u = Array.prototype[Symbol.unscopables];
  var names = ["at", "copyWithin", "entries", "fill", "find", "findIndex", "findLast", "findLastIndex", "flat", "flatMap", "includes", "keys", "toReversed", "toSorted", "toSpliced", "values"];
  for (var i = 0; i < names.length; i++) {
    var d = Object.getOwnPropertyDescriptor(u, names[i]);
    if (!d || d.value !== true || !d.writable || !d.enumerable || !d.configurable) return false;
  }
  return !("push" in u) && !("toString" in u);
})());
failed === 0 ? rows : -failed;
