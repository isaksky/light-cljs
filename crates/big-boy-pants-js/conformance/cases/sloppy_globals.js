// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19744-19876:
// sloppy_implicit_globals_and_delete (line 19744, bbp-105) and
// global_object_descriptors (19830): implicit globals made by assignment,
// `delete` of an identifier (implicit globals, declared bindings, built-in
// globals, parameters and locals, the function-expression name), compound
// assignment and for-in to an undeclared name, ReferenceErrors after delete
// and in a strict function, and the descriptors of the global object's own
// value properties. Each Pants script used a fresh global object; here each
// row uses its own names, and the row that replaces `globalThis` runs last.
// Not repeated: the rows sloppy_implicit_global_delete and
// global_var_function_properties already pin (an implicit global from a
// function, its descriptor, delete of a var, function, let and unknown
// name, global var and function descriptors). Left out: `delete __rt`
// (Pants-only early error); rows with Object.defineProperty,
// Object.setPrototypeOf, Object.preventExtensions or Object.keys (the
// objects area, light-cljs-766); the descriptors of isNaN, isFinite,
// parseInt and parseFloat, which core_global_number_functions pins
// (light-cljs-900). Math's is here (light-cljs-901). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Implicit globals at script level, and delete.
u1 = 3;
row(delete u1);
u2 = 3; delete u2; u2 = 5;
row(u2 === 5);
u3 = 3; delete u3;
row(name(function () { return u3; }) === "ReferenceError");
// Declared bindings and built-in values are not deleted.
const constW = 1;
row(delete constW === false && constW === 1);
row(delete eval);
{ let eval = 1; row(delete eval === false && eval === 1); }
row((function () { return delete arguments; })() === false);
var tdzOk = delete tdzW === false; let tdzW = 1;
row(tdzOk && tdzW === 1);
row((function (p) { var v = 1; let w = 2; return delete p === false && delete v === false && delete w === false; })(3));
let closedW = 1;
row((function () { return delete closedW; })() === false && closedW === 1);
{ let f = function self() { return delete self; }; row(f() === false); }
row(delete undefined === false && delete NaN === false && delete Infinity === false);
row(delete (/* comment */ Infinity) === false && delete \u0075ndefined === false);
NaN;
row(delete (/* NaN */ 1) && delete (NaN, 1));
// Compound assignment and update of an implicit global, for-in to an
// undeclared name.
u4 = 1; u4 += 2; var oldU4 = u4++;
row(oldU4 === 3 && globalThis.u4 === 4 && delete u4);
for (k5 in {a: 1}) {}
row(k5 === "a" && delete k5);
// ReferenceErrors: a strict function, and compound or update assignment to
// an unresolvable name.
row(name(function () { "use strict"; u6 = 1; }) === "ReferenceError" && typeof u6 === "undefined");
row(name(function () { missing7 += 1; }) === "ReferenceError");
row(name(function () { missing8++; }) === "ReferenceError");
// The global object's own value properties.
function describes(key, value) {
  var d = Object.getOwnPropertyDescriptor(globalThis, key);
  return d !== undefined && d.value === value && d.writable && !d.enumerable && d.configurable;
}
row(describes("globalThis", this));
row(describes("Object", Object) && Object.name === "Object");
row(describes("Number", Number) && Number.name === "Number");
row(describes("String", String) && String.name === "String");
row(describes("Array", Array) && Array.name === "Array");
row(describes("Function", Function) && Function.name === "Function");
row(describes("Reflect", Reflect) && typeof Reflect === "object" && typeof Reflect.apply === "function");
row(describes("Math", Math) && typeof Math === "object" && typeof Math.floor === "function");
function fixed(key) {
  var d = Object.getOwnPropertyDescriptor(globalThis, key);
  return d !== undefined && !d.writable && !d.enumerable && !d.configurable;
}
row(fixed("undefined") && Object.getOwnPropertyDescriptor(globalThis, "undefined").value === undefined);
row(fixed("NaN") && Object.is(Object.getOwnPropertyDescriptor(globalThis, "NaN").value, NaN));
row(fixed("Infinity") && Object.getOwnPropertyDescriptor(globalThis, "Infinity").value === Infinity);
// Replacing the `globalThis` binding does not change the global object.
var savedGlobal = globalThis;
globalThis = null;
u9 = 1;
row(savedGlobal.u9 === 1 && delete u9 && typeof u9 === "undefined");
failed === 0 ? rows : -failed;
