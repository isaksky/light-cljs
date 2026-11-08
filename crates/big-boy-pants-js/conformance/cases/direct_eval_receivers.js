// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2266-2296,
// direct_eval_receiver_semantics, its rows without a primitive receiver
// (which the Pants engine refuses), lines 2218-2235 (strict_direct_eval's
// sloppy rows) and js_functions.rs lines 2699-2734,
// sloppy_nullish_receivers_use_the_task_global_object's eval rows. Eval code
// reads a sloppy caller's receiver (CheckSloppyThis: undefined and null are
// the global object), a strict caller's as it is; arrows and nested evals
// keep it; a shadowing `eval` binding is an ordinary call. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Each row runs as one script: `run` as eval code of a direct eval in a
// fresh function, so its declarations stay its own; `script` through an
// indirect eval, as global code.
function run(source) { return eval(source); }
function script(source) { return (0, eval)(source); }
row(run("function h(eval) { return (() => eval(1, 2))(); } h((a,b) => a+b)") === 3);
row(run("function f() { return (() => eval(3))(); } f()") === 3);
row(run("function f() { return (() => eval('1+2'))(); } f()") === 3);
row(run("function f() { return (() => eval('\"use strict\"; 1+2'))(); } f()") === 3);
row(run("function f() { return eval('() => this'); } typeof f.call(7) === 'function'") === true);
row(run("function f() { return (() => eval('this'))(); } let o={}; f.call(o) === o") === true);
row(run("function f() { return eval('\"use strict\"; this'); } let o={}; f.call(o) === o") === true);
row(run("function f() { let x=3; return eval('\"use strict\"; var y=2; x+y'); } f()") === 5);
row(run("function h(eval) { return eval(1, 2); } h((a, b) => a + b)") === 3);
row(run("function f() { return eval('this'); } f() === globalThis && f.call(null) === globalThis && f.call({x: 7}).x === 7") === true);
row(run("function f() { return eval('\"use strict\"; this'); } f() === globalThis") === true);
row(run("function f() { 'use strict'; return eval('this'); } f() === undefined && f.call(null) === null") === true);
// Direct sites of this file's own code.
row((function () { return eval("this"); })() === globalThis);
row((function () { return eval("() => this"); }).call(null)() === globalThis);
failed === 0 ? rows : -failed;
