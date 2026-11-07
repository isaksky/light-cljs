// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19624-19626,
// global_object_bindings ("let b = 1; globalThis.b === undefined";
// light-cljs-824): a script's `let` and `const` bindings live in the global
// lexical environment, not on globalThis; functions declared beside them
// read them, and a let assigned later is seen. Completes with
// "undefined absent absent 7 number".
let b = 1;
const c = 2;
function read() { return b + c; }
function flag(x, yes, no) { return x ? yes : no; }
b = 5;
typeof globalThis.b + " " + flag("c" in globalThis, "?", "absent") + " " +
  flag(Object.getOwnPropertyDescriptor(globalThis, "b") !== undefined, "?", "absent") + " " + flag(read() === 7, "7", "?") + " " + typeof b;
