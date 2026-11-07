// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 2699-2732:
// the call, apply, Reflect.apply and `bind` rows of
// sloppy_nullish_receivers_use_the_task_global_object (bbp-102,
// light-cljs-851, light-cljs-852): a sloppy callee sees the global object
// for an undefined or null receiver, and a dispatcher forwarding `this` and
// `arguments`. Left out: the eval and Function-constructor rows (later
// children of light-cljs-764) and collection between slices (a
// Pants runtime test). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
{ function f() { return this; } row(f() === globalThis && f.call(undefined) === globalThis && f.call(null) === globalThis); }
{ function f() { return this; } row(f.apply(undefined, []) === globalThis && Reflect.apply(f, null, []) === globalThis && f.apply(null) === globalThis); }
{ function f() { return this; } const o = {}; row(f.call(o) === o && f.apply(o, []) === o && Reflect.apply(f, o, []) === o && f.bind(o)() === o); }
{ function f() { return this; } row(f.bind(undefined)() === globalThis && f.bind(null)() === globalThis); }
{ function f() { 'use strict'; return this; } row(f.bind(undefined)() === undefined && f.bind(null)() === null && f.bind(7)() === 7); }
{ function f() { 'use strict'; return this; } row(f.apply(undefined, []) === undefined && f.apply(null, []) === null && Reflect.apply(f, 7, []) === 7); }
{ function f() { return () => this; } const arrow = f(); row(arrow.apply({}, []) === globalThis && f.apply(null)() === globalThis); }
{ function entry() { return this; } function dispatch() { return entry.apply(this, arguments); } row(dispatch() === globalThis && dispatch.call(null) === globalThis); }
{
  function entry(x, rest) { return this.marker + x + rest; }
  function dispatch() { return entry.call(this, arguments[0], arguments.length - 1); }
  globalThis.marker = 10;
  row(dispatch(20, 30, 40) === 32 && dispatch.call({marker: 11}, 20, 30) === 32);
}
failed === 0 ? rows : -failed;
