// bbp-authored conformance case (light-cljs-883), run as a sloppy script.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 7102-17240:
// stored_proto_accessor (line 7102, bbp-261), its failing rows run in sloppy
// code, and prototype_reflection (line 16949, bbp-121), its sloppy
// `o.__proto__ = o` row and the sloppy no-op rows on Object.prototype. The
// `__proto__` setter's refusals throw in sloppy code too: they come from
// the setter, not from a refused [[Set]]. Errors are compared by name. Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
var o = {};
row(name(function () { o.__proto__ = o; }) === "TypeError" && Object.getPrototypeOf(o) === Object.prototype);
var n = Object.preventExtensions({});
row(name(function () { n.__proto__ = {}; }) === "TypeError" && Object.getPrototypeOf(n) === Object.prototype);
row(name(function () { n.__proto__ = Object.prototype; }) === "none");
row(name(function () { Object.prototype.__proto__ = {}; }) === "TypeError" && Object.getPrototypeOf(Object.prototype) === null);
Object.prototype.__proto__ = null; Object.prototype.__proto__ = 1; [].constructor.prototype.__proto__ = Object.prototype;
row(Object.getPrototypeOf(Object.prototype) === null);
var p = {x: 1}; var m = {}; m.__proto__ = p; m.__proto__ = 2;
row(Object.getPrototypeOf(m) === p && m.x === 1);
var b = true; b.__proto__ = {}; (1).__proto__ = null;
row(b === true);
var d = Object.getOwnPropertyDescriptor(Object.prototype, '__proto__');
row(name(function () { d.set.call(undefined, {}); }) === "TypeError" && name(function () { d.get.call(null); }) === "TypeError");
row(name(function () { Object.setPrototypeOf(n, {}); }) === "TypeError" && Reflect.setPrototypeOf(n, {}) === false);
failed === 0 ? rows : -failed;
