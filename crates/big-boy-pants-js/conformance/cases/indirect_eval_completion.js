// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2541-2618,
// indirect_eval (bbp-106), the rows without Object.preventExtensions,
// Object.defineProperty, e.stack or the 16,384-unit source cap, and the
// indirect-eval rows of
// rust/tests/js_functions.rs super_in_arrows_and_eval (249) and
// new_target_in_functions_and_arrows (334) (light-cljs-856). The global
// `eval` called other than as a direct-eval site returns a non-string
// argument unchanged and runs a string as global script code in a new
// frame, completing with the script's completion value. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row((0, eval)("1 + 1") === 2);
row((0, eval)(5) === 5);
row((0, eval)("if (0) 2; else 3;") === 3);
row((function () { const e = eval; let o = {}; return e(o) === o && e() === undefined && e(null) === null; })());
row(eval.length === 1 && eval.name === "eval" && globalThis.eval === eval);
row((0, eval)("this") === globalThis);
// Every route to %eval% but a direct-eval site is indirect.
row(eval.call(null, "2") === 2 && eval.apply(null, ["3"]) === 3 && eval.bind(null)("4") === 4 && (0, eval)(...["5"]) === 5);
row((0, eval)("function f(eval) { return eval(2); } f(x => x + 1)") === 3);
row(name(() => new eval("1")) === "TypeError");
// Eval code's own bindings: a closure over a `let`, a strict eval's `var`.
row((function () { const f = (0, eval)("let x = 3; () => ++x"); return f() === 4 && f() === 5 && typeof x === "undefined"; })());
row((function () { const f = (0, eval)("'use strict'; var y = 3; () => ++y"); return f() === 4 && f() === 5 && typeof y === "undefined"; })());
// The caller's scope is not eval code's.
row((function () { const k = 1; return name(() => (0, eval)("k")); })() === "ReferenceError");
// A loop inside eval code runs in the job's dispatch loop.
row((0, eval)("var n = 0; while (n < 25) n++; n") === 25);
// super and new.target are early errors in global code.
row(name(() => ({ m() { return (0, eval)("super.x"); } }).m()) === "SyntaxError");
row((function () { function F() { return (0, eval)("new.target"); } return name(() => new F()); })() === "SyntaxError");
failed === 0 ? rows : -failed;
