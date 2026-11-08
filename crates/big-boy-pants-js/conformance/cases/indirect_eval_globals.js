// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2541-2618,
// indirect_eval (bbp-106), its global-declaration rows without
// Object.preventExtensions or Object.defineProperty (light-cljs-856). Sloppy
// eval code's `var` and function declarations create configurable global
// properties (ECMAScript EvalDeclarationInstantiation), an existing global
// `var` stays as it was, a strict eval keeps its declarations local and a
// `let` stays local to the eval. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
(0, eval)("var q = 1");
row(q === 1 && Object.getOwnPropertyDescriptor(globalThis, "q").configurable === true);
row((function () { const e = eval; e("var x1 = 3; function g1() { return x1; }"); return g1() === 3 && Object.getOwnPropertyDescriptor(globalThis, "g1").configurable && delete globalThis.x1 && delete globalThis.g1 && typeof g1 === "undefined"; })());
row((function () { (0, eval)("'use strict'; var x2 = 1; function g2() {}"); return typeof x2 === "undefined" && typeof g2 === "undefined"; })());
(0, eval)("var n = 1");
(0, eval)("n += 2");
row(n === 3);
var kept = 1;
(0, eval)("var kept;");
row(kept === 1 && !Object.getOwnPropertyDescriptor(globalThis, "kept").configurable);
// A configurable global function is redefined; the script's own stays.
function scripted() { return 1; }
(0, eval)("function scripted() { return 2; }");
row(scripted() === 2 && !Object.getOwnPropertyDescriptor(globalThis, "scripted").configurable);
globalThis.loose = 1;
(0, eval)("function loose() { return 3; }");
row(loose() === 3 && Object.getOwnPropertyDescriptor(globalThis, "loose").configurable && Object.getOwnPropertyDescriptor(globalThis, "loose").enumerable);
row(name(() => (0, eval)("let w = 1; w") && w) === "ReferenceError");
row((function () { let d = Object.getOwnPropertyDescriptor(globalThis, "eval"); return d.writable && d.configurable && !d.enumerable; })());
failed === 0 ? rows : -failed;
