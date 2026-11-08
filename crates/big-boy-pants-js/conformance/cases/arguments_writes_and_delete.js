// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 482-532
// (arguments_object_holds_every_argument, bbp-97: rows 5, 6 and 20) and
// light-cljs-854's probe rows: an arguments object is an ordinary object
// (CreateUnmappedArgumentsObject, ECMAScript 10.4.4.6; QuickJS
// js_build_arguments, quickjs.c 16401).
// Element writes, element writes past the length, `length` writes, deletes
// of an element and of `length`, and new named and symbol properties, all
// in strict functions (unmapped objects). Left out until their pellets land:
// Object.defineProperty, Object.freeze and Object.keys (the objects area,
// light-cljs-766); writes that alias a parameter (the mapped object of a
// sloppy function, light-cljs-860). Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ function f() { arguments[0] = 5; return arguments[0]; } row(f(1) === 5); }
{ function f() { arguments.length = 1; return arguments.length; } row(f(1, 2) === 1); }
{ function f() { return delete arguments[0] && !(0 in arguments); } row(f(1) === true); }
{ function f(a) { a = 9; arguments[0] = 3; return a === 9 && arguments[0] === 3; } row(f(1)); }
// A write past the end adds an element; `length` is a plain data property.
{ function f() { arguments[3] = 7; return arguments.length === 1 && arguments[3] === 7 && arguments[2] === undefined && !(2 in arguments); } row(f(9)); }
{ function f() { arguments.length = 5; return arguments.length === 5 && arguments[1] === undefined && arguments[0] === 9; } row(f(9)); }
{ function f() { arguments.length = "x"; return arguments.length === "x" && arguments[0] === 9; } row(f(9)); }
{ function f() { arguments[1] = 2; arguments[0] = 1; return arguments[0] + arguments[1]; } row(f(4, 5) === 3); }
// A deleted element reads undefined; another write brings it back.
{ function f() { delete arguments[1]; const gone = arguments[1] === undefined && arguments.length === 3; arguments[1] = 8; return gone && arguments[1] === 8 && 1 in arguments; } row(f(1, 2, 3)); }
{ function f() { return delete arguments.length && !("length" in arguments) && arguments.length === undefined; } row(f(1)); }
{ function f() { return delete arguments[Symbol.iterator] && arguments[Symbol.iterator] === undefined; } row(f(1)); }
{ function f() { return delete arguments[5] && delete arguments.missing; } row(f(1)); }
// New own properties, by name and by symbol.
{ function f() { arguments.x = 1; arguments.y = 2; return arguments.x + arguments.y === 3 && Object.getOwnPropertyDescriptor(arguments, "x").enumerable; } row(f()); }
{ const s = Symbol.match; function f() { arguments[s] = 4; return arguments[s] === 4; } row(f()); }
{ function f() { arguments[Symbol.iterator] = 1; return arguments[Symbol.iterator] === 1; } row(f()); }
// Each call has its own object.
{ function f() { arguments[0] = 1; return arguments; } const a = f(5), b = f(6); row(a[0] === 1 && b[0] === 1 && a !== b); }
// Descriptors: Object.keys counted with for-in until it lands.
{
  function f() {
    const a = arguments;
    const d = Object.getOwnPropertyDescriptor(a, "length"); const i = Object.getOwnPropertyDescriptor(a, "0");
    let keys = 0; for (const k in a) keys++;
    return Object.getPrototypeOf(a) === Object.prototype && d.value === 2 && d.writable && !d.enumerable && d.configurable && i.value === 1 && i.writable && i.enumerable && i.configurable && keys === 2;
  }
  row(f(1, 2));
}
{
  function f() {
    const d = Object.getOwnPropertyDescriptor(arguments, Symbol.iterator);
    let keys = 0; for (const k in arguments) keys++;
    return d.value === Array.prototype.values && d.writable && !d.enumerable && d.configurable && keys === 1;
  }
  row(f(1));
}
{ function f() { arguments.length = 0; const d = Object.getOwnPropertyDescriptor(arguments, "length"); return d.value === 0 && d.writable && !d.enumerable; } row(f(1)); }
// An iterator reads the current length and elements.
{ function f() { arguments.length = 2; arguments[1] = 7; return [...arguments]; } const a = f(1, 2, 3); row(a.length === 2 && a[0] === 1 && a[1] === 7); }
{ function f() { arguments[0] = 4; return Array.prototype.join.call(arguments, "-"); } row(f(1, 2) === "4-2"); }
{ function g(x, y) { return x * y; } function f() { arguments[1] = 10; return g.apply(null, arguments); } row(f(3, 4) === 30); }
failed === 0 ? rows : -failed;
