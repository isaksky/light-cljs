// bbp-authored conformance case.
// Derives from: ~/src/quickjs/quickjs.c lines 16401-16420 (js_build_arguments,
// revision 7d1c13c) and light-cljs-854's probe rows: for-in over an arguments object
// (a gap light-cljs-763 left to this area) and its class, "Arguments", for
// Object.prototype.toString (ECMAScript 20.1.3.6 step 7, its
// [[ParameterMap]]; QuickJS JS_CLASS_ARGUMENTS), in strict functions. for-in
// visits the elements in index order, then named properties in creation
// order; `length` and @@iterator are not enumerable (`callee`, which the
// Pants engine does not define, is arguments_callee's). Left out until
// their pellets land: Object.keys and Object.getOwnPropertyNames (the
// objects area, light-cljs-766). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ function f() { var s = ""; for (var k in arguments) s += k; return s; } row(f(1, 2, 3) === "012"); }
{ function f() { return Object.prototype.toString.call(arguments); } row(f() === "[object Arguments]"); }
{ function f() { var s = ""; for (var k in arguments) s += k; return s; } row(f() === ""); }
{ function f() { var s = ""; for (var k in arguments) s += k + "=" + arguments[k] + ";"; return s; } row(f("a", "b") === "0=a;1=b;"); }
{ function f() { delete arguments[1]; arguments.x = 1; arguments[5] = 2; var s = ""; for (var k in arguments) s += k; return s; } row(f(1, 2, 3) === "025x"); }
{ function f() { arguments.length = 1; var s = ""; for (var k in arguments) s += k; return s; } row(f(1, 2) === "01"); }
// The class shows in ToString and ToPrimitive, not in Array.isArray.
{ function f() { return String(arguments) + "" + arguments; } row(f(1) === "[object Arguments][object Arguments]"); }
{ function f() { return Array.isArray(arguments) || !(arguments instanceof Object) || typeof arguments !== "object"; } row(f() === false); }
{ function f() { arguments[Symbol.toStringTag] = "Mine"; return Object.prototype.toString.call(arguments); } row(f() === "[object Mine]"); }
{ function f() { return Object.prototype.toString.call(arguments); } function g() { return f.apply(null, arguments); } row(g(1) === "[object Arguments]"); }
// `in` and own descriptors see the own properties, enumerable or not.
{ function f() { return "length" in arguments && Symbol.iterator in arguments && Object.getOwnPropertyDescriptor(arguments, "length") !== undefined && Object.getOwnPropertyDescriptor(arguments, 0) !== undefined && Object.getOwnPropertyDescriptor(arguments, 1) === undefined; } row(f(1)); }
{ function f() { return Object.getPrototypeOf(arguments) === Object.prototype && arguments.toString() === "[object Arguments]"; } row(f()); }
// As instanceof's right operand it is an object without [[Call]]: a TypeError.
{ function f() { return arguments; } const a = f(1); let t = "none"; try { ({}) instanceof a; } catch (e) { t = e.name; } row(t === "TypeError"); }
failed === 0 ? rows : -failed;
