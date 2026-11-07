// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14809-14834,
// object_constructor (bbp-280), its object and nullish rows, and
// rust/src/vm/builtin.rs object_constructor (ECMAScript Object(value) when
// new.target is Object itself): `new Object` and `new Object()` are distinct
// ordinary objects inheriting Object.prototype, undefined and null give a new
// one, an object argument is returned as it is and is not converted, and
// extra arguments are evaluated in order. Primitive arguments (ToObject's
// wrappers) are left to the wrapper pellets. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ const a = new Object, b = new Object(); row(a !== b && Object.getPrototypeOf(a) === Object.prototype && Object.getPrototypeOf(b) === Object.prototype); }
{ const a = new Object(); a.x = 42; row(a.x === 42 && a.constructor === Object && Object.getOwnPropertyDescriptor(a, "x").value === 42); }
row(typeof new Object() === "object");
row(new Object(undefined) !== new Object(null) && Object.getPrototypeOf(new Object(null)) === Object.prototype);
row(new Object(undefined) !== new Object(undefined));
{ const o = {x: 1}; row(new Object(o) === o && Object(o) === o); }
{ const a = [], f = function () {}; row(new Object(a) === a && new Object(f) === f && new Object(Object) === Object); }
{ const o = {}; row(new Object(new Object(o)) === o); }
{ let n = 0; const o = {valueOf() { n++; throw 1; }, toString() { n++; throw 2; }}; row(new Object(o) === o && n === 0); }
{ let n = 0; const o = {}; function arg(x) { n = n * 10 + x; return o; } row(new Object(arg(1), arg(2)) === o && n === 12); }
// The new objects are ordinary objects with no own keys.
{ const o = new Object(); let keys = 0; for (const k in o) keys++; row(keys === 0 && Object.getOwnPropertyDescriptor(o, "constructor") === undefined && Object.prototype.toString.call(o) === "[object Object]"); }
// Object called without `new` agrees.
row(Object.getPrototypeOf(Object()) === Object.prototype && Object() !== Object());
failed === 0 ? rows : -failed;
