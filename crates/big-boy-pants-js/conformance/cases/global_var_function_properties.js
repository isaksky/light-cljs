// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19818-19824,
// global_object_bindings and global_object_descriptors (light-cljs-824):
// script `var` and function declarations are properties of globalThis,
// writable, enumerable and not configurable, and reads and writes of the
// name go to that property. Completes with
// "a:numberWE- g:functionWE- u:undefinedWE- a21 own own own absent".
var a = 1;
var u;
function g() { a = a + 1; return a; }
function flag(b, yes, no) { return b ? yes : no; }
function describe(name) {
  var d = Object.getOwnPropertyDescriptor(globalThis, name);
  return name + ":" + typeof d.value + flag(d.writable, "W", "-") + flag(d.enumerable, "E", "-") + flag(d.configurable, "C", "-");
}
g();
globalThis.a = globalThis.a * 10;
g();
describe("a") + " " + describe("g") + " " + describe("u") + " " + flag(a === 21, "a21", "a?") + " " +
  flag(globalThis.g === g, "own", "?") + " " + flag("u" in globalThis, "own", "?") + " " +
  flag(Object.getOwnPropertyDescriptor(globalThis, "a") !== undefined, "own", "?") + " " + flag("missing" in globalThis, "?", "absent");
