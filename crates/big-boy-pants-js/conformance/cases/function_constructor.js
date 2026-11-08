// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 75-176,
// function_constructor (bbp-111) and the semantics rows of
// function_constructor_waits_collects_and_migrates; the Function-constructor
// row of sloppy_nullish_receivers_use_the_task_global_object (line 2710) and
// js_source.rs rest_spread_and_iteration_patterns row 6 (line 34)
// (light-cljs-857). Not ported: `typeof anonymous` (QuickJS and the Pants
// engine disagree) and a lone surrogate in the text (light-cljs-864).
// Function(p1, ..., pn, body) and new Function(...) compile
// "function anonymous(p1,...,pn\n) {\nbody\n}" in the global scope. Each row
// is a `row(...)` check; the script completes with the number of rows, or
// minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row(Function("return 7")() === 7);
row(new Function("a", "b", "return a + b;")(2, 3) === 5);
row(new Function("eval", "return eval(1, 2)")((a, b) => a + b) === 3);
row(Function()() === undefined);
// The parameter strings may each hold several names and defaults.
row(new Function("a, b", "c", "return a + b + c")(1, 2, 3) === 6);
row(new Function("a = 4", "b = a * 2", "return a + b")() === 12);
row(new Function("...rest", "return rest.length")(1, 2, 3) === 3);
row(new Function("[a, b]", "{c}", "return a + b + c")([1, 2], { c: 3 }) === 6);
row(Function("a = 2", "b", "return a + b")(undefined, 3) === 5);
row((function () { let f = Function("a", "...r", "return a + r.length"); return f.length === 1 && f(7, 8, 9) === 9; })());
row(Function("eval", "return eval(...[1, 2])")((a, b) => a + b) === 3);
row(Function(123)() === undefined);
// Comments in the text: the "\n)" ends a line comment in the parameters.
row(Function("a // comment", "return a")(3) === 3);
row(Function("return 3 // comment")() === 3);
row(Function("/* \u{1F600} */ a", "return a")(4) === 4);
row(Function("return (0, eval)(\"3\")")() === 3);
row(Function("return (() => 4)")()() === 4);
row(Function("return () => this")()() === globalThis);
// Metadata.
const f = new Function("a", "b", "");
row(f.length === 2 && f.name === "anonymous");
row(new Function("a = 1", "b", "").length === 0);
row(typeof f === "function" && Object.getPrototypeOf(f) === Function.prototype);
row(f instanceof Function);
// The global scope, never the caller's.
var g = 11;
row((function () { const g = 1; return new Function("return g")(); })() === 11);
row((function () { const k = 1; try { new Function("return k")(); return false; } catch (e) { return e instanceof ReferenceError; } })());
// Sloppy unless the body says otherwise; `this` of a plain call.
row(new Function("return this")() === globalThis);
row(new Function("'use strict'; return this")() === undefined);
row(new Function("return this").call(null) === globalThis);
row(new Function("'use strict'; return typeof this").call(5) === "number");
// Constructors: a fresh prototype, new.target and construction.
const C = new Function("x", "this.x = x");
const c = new C(4);
row(c.x === 4 && c instanceof C && Object.getPrototypeOf(c) === C.prototype);
row(C.prototype.constructor === C);
row(new Function("return new.target")() === undefined);
row((function () { const F = Function("return new.target"); return new F() === F; })());
row(Function("\"use strict\"; return this").call(7) === 7);
row(Function.prototype.constructor === Function && Object.getPrototypeOf(Function()) === Function.prototype);
// Arguments convert by ToString, in order.
const order = [];
const p = { toString() { order.push("p"); return "q"; } };
const body = { toString() { order.push("body"); return "return q * 2"; } };
row(Function(p, body)(5) === 10 && order.join() === "p,body");
row(Function("return " + undefined)() === undefined);
// Called through call, apply, Reflect and a spread.
row(Function.call(null, "return 8")() === 8);
row(Function.apply(null, ["a", "return a + 1"])(1) === 2);
row(Reflect.apply(Function, undefined, ["return 9"])() === 9);
row(Reflect.construct(Function, ["return 10"])() === 10);
const args = ["a", "b", "return a * b"];
row(new Function(...args)(6, 7) === 42);
row(Function(...args)(2, 3) === 6);
// Reflect.construct's newTarget gives the function its prototype.
function NT() {}
NT.prototype = { marker: 1 };
const r = Reflect.construct(Function, ["return 1"], NT);
row(Object.getPrototypeOf(r) === NT.prototype && r() === 1 && r.marker === 1);
failed === 0 ? rows : -failed;
