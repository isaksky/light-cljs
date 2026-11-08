// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2627-2668,
// indirect_eval_global_lexicals (bbp-278), and lines 2673-2690,
// indirect_eval_declaration_error_precedence, without their
// Object.defineProperty and Object.preventExtensions rows (light-cljs-856).
// The script's lexical bindings are visible to eval code and assignable,
// `const` a TypeError and the temporal dead zone a ReferenceError, while
// they never become global properties; a sloppy eval `var` or function
// declaration that conflicts with one is a SyntaxError that creates no
// property, checked before any declaration. Each row is a `row(...)` check;
// the script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
let a = 7;
row((0, eval)("a") === 7 && Object.getOwnPropertyDescriptor(globalThis, "a") === undefined);
let b = 7;
(0, eval)("b = 9");
row(b === 9);
let c = 7;
globalThis.c = 3;
(0, eval)("c += 2");
row(c === 9 && globalThis.c === 3);
let d = 7;
let readD = () => d;
(0, eval)("d = 9");
row(readD() === 9);
let e = 7;
let bumpE = (0, eval)("() => ++e");
row(bumpE() === 8 && e === 8);
const f = 7;
row((0, eval)("f") === 7);
const g = 7;
row(name(() => (0, eval)("g = 9")) === "TypeError" && g === 7);
// Before its declaration runs, a script binding is in its dead zone.
row(name(() => (0, eval)("h")) === "ReferenceError");
// (`typeof` of a binding in its dead zone, a ReferenceError on the Pants engine,
// is "undefined" on QuickJS: not ported.)
row(name(() => (0, eval)("j = 9")) === "ReferenceError");
const h = 7;
const j = 7;
// Eval code's own declarations shadow the script's.
let k = 7;
row((0, eval)("let k = 9; k") === 9 && k === 7);
let l = 7;
row((0, eval)("'use strict'; var l = 9; l") === 9 && l === 7);
let m = 7;
row((function () { let m = 9; return (0, eval)("m"); })() === 7);
row((function () { { let m = 9; if ((0, eval)("m") !== 7) return false; } return m === 7; })());
let o = 7;
row((0, eval)("delete o") === false && o === 7);
let { p: p } = { p: 7 };
(0, eval)("p = 9");
row(p === 9);
const [r] = [7];
row((0, eval)("r") === 7);
// A conflicting declaration fails before any declaration takes effect.
let s = 7;
row(name(() => (0, eval)("var fresh1; var s")) === "SyntaxError" && s === 7 && Object.getOwnPropertyDescriptor(globalThis, "s") === undefined && Object.getOwnPropertyDescriptor(globalThis, "fresh1") === undefined);
row(name(() => (0, eval)("var fresh2; function s() {}")) === "SyntaxError" && s === 7 && Object.getOwnPropertyDescriptor(globalThis, "fresh2") === undefined);
row(name(() => (0, eval)("function fresh3() {} var s;")) === "SyntaxError" && Object.getOwnPropertyDescriptor(globalThis, "fresh3") === undefined);
failed === 0 ? rows : -failed;
