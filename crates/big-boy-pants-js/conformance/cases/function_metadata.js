// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 743-784, 809-830:
// function_length_and_name (line 743, bbp-101) and
// function_metadata_property_operations (809, bbp-101); js_source.rs
// function_own_properties (15720, bbp-125); the `bind` rows are back
// (light-cljs-852). Rows that need Object.getOwnPropertyNames, Object.keys,
// Object.defineProperty, Object.freeze, Object.preventExtensions or
// Object.create (the objects area, light-cljs-766) are left out, and so are
// the Pants-only rows (publication, executable round trips, the
// Function.prototype methods the Pants engine lacks). Not repeated: the
// names, lengths and descriptors function_name_length and
// function_properties_prototype already pin. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// `length` and `name`.
row((function (a, b) {}).length === 2);
row(((a, b = 1, c) => {}).length === 1);
row((() => {}).length === 0);
{ function f() {} row(f.name === "f"); }
{ const g = () => 1; row(g.name === "g"); }
{ let h; h = function () {}; row(h.name === "h"); }
row((function () {}).name === "");
{ var v = function () {}; row(v.name === "v"); }
{ const f = function named() {}; row(f.name === "named"); }
{ const f = (0, () => {}); row(f.name === ""); }
{ const o = {m: function () {}, 'x': () => 1, 3: () => 0}; row(o.m.name === "m" && o.x.name === "x" && o[3].name === "3"); }
{ const o = {'\ud800': () => 0}; row(o['\ud800'].name === '\ud800'); }
{ function f(a = () => {}, b = function () {}) { return a.name === "a" && b.name === "b"; } row(f()); }
{ function f(a, b = 1, c) {} row(f.bind(null).length === 1 && f.bind(null, 1).length === 0 && f.bind().name === "bound f"); }
{ const f = (a, b = 1) => a; const d = Object.getOwnPropertyDescriptor(f, "length"); row(f.length === 1 && f.name === "f" && !d.writable && !d.enumerable && d.configurable && f(7) === 7); }
{ function entry(a, b = 1, c) { return entry.length === 1 && entry.name === "entry"; } row(entry()); }
// Their descriptors and operations.
{
  function f(a, b) {}
  const d = Object.getOwnPropertyDescriptor(f, "length");
  const n = Object.getOwnPropertyDescriptor(f, "name");
  row(d.value === 2 && !d.writable && !d.enumerable && d.configurable && n.value === "f" && !n.writable && !n.enumerable && n.configurable);
}
{ function f() {} f.x = 1; row(f.length === 0 && f.name === "f" && "length" in f && "name" in f); }
{
  function f() {}
  row(delete f.length && delete f.name && f.length === 0 && f.name === "" &&
      Object.getOwnPropertyDescriptor(f, "length") === undefined && Object.getOwnPropertyDescriptor(f, "name") === undefined);
}
{ function f(a, b) {} delete f.length; delete f.name; const g = f.bind(null, 1); row(g.length === 0 && g.name === "bound "); }
// Assigning `length` or `name` is a TypeError in strict code.
{ const f = () => 0; row(name(function () { f.length = 7; }) === "TypeError" && f.length === 0); }
{ const f = () => 0; row(name(function () { f.name = 7; }) === "TypeError" && f.name === "f"); }
// `prototype` and ordinary own properties.
{ let f = function f() {}; row(f.prototype.constructor === f && f.prototype === f.prototype); }
{ function h() {} row(h.prototype.constructor === h); }
{ function g() { return g.prototype; } row(g() === g.prototype); }
{ function mk() { return function () {}; } let a = mk(), b = mk(); row(a.prototype !== b.prototype && a.prototype.constructor === a); }
{ let f = function () {}; row(f.prototype.toString() === "[object Object]"); }
{ let f = function () {}; f.prototype.m = function () { return this === f.prototype; }; row(f.prototype.m()); }
{ let f = function () {}; row(f.y === undefined); }
{ let f = function () {}; f.m = function () { return this === f; }; row(f.m()); }
{ let f = function () {}; row(typeof f.valueOf === "function" && f.valueOf === Object.prototype.valueOf); }
row((() => 1).prototype === undefined);
{ let a = () => 1; a.prototype = 3; a.x = 4; row(a.prototype + a.x === 7); }
{ let f = function f() {}; f.x = 1; row(f.x === 1); }
{ let f = function () {}; f.x = 1; f.x = f.x + 1; row(f.x === 2); }
{ let f = function () {}; f.prototype = 5; row(f.prototype === 5); }
{ let f = function () {}; let p = f.prototype; f.prototype = 5; row(f.prototype === 5 && p.constructor === f); }
{
  let f = function () {}; f.prototype.n = 0; let i = 0;
  while (i < 50) { f.prototype.n = f.prototype.n + i; i = i + 1; }
  row(f.prototype.n + (f.prototype.constructor === f ? 1000 : 0) === 2225);
}
// `call`, `apply` and `constructor` are inherited from Function.prototype.
{ let f = function () {}; row(f.call === Function.prototype.call && f.apply === Function.prototype.apply && f.constructor === Function); }
{ let f = () => 1; row(f.call === Function.prototype.call); }
{ let f = function () {}; row(f.length === 0); }
{ let f = function f() {}; row(f.name === "f"); }
// Attributes.
{ let f = function f() {}; row(Object.getOwnPropertyDescriptor(f, "prototype").enumerable === false); }
{ let f = function () {}; let d = Object.getOwnPropertyDescriptor(f, "prototype"); row(d.writable && !d.enumerable && !d.configurable && d.value === f.prototype); }
{ let f = function () {}; let d = Object.getOwnPropertyDescriptor(f.prototype, "constructor"); row(d.writable && !d.enumerable && d.configurable && d.value === f); }
{ let f = function () {}; f.x = 1; let d = Object.getOwnPropertyDescriptor(f, "x"); row(d.writable && d.enumerable && d.configurable); }
row(Object.getOwnPropertyDescriptor(() => 1, "prototype") === undefined);
row(Object.getOwnPropertyDescriptor(function () {}, "x") === undefined);
{ let f = function f() {}; row(Object.getPrototypeOf(f) === Function.prototype); }
failed === 0 ? rows : -failed;
