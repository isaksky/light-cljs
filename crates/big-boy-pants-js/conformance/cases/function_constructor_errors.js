// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_functions.rs lines 113-133,
// function_constructor (bbp-111), its error rows (light-cljs-857). Source
// the Function constructor cannot compile throws a catchable SyntaxError
// before anything runs, including a parameter text that would close the
// parameter list early; a conversion that throws propagates unchanged and
// stops the later conversions. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function caught(f) { try { f(); return null; } catch (e) { return e; } }
row(caught(() => new Function("(")) instanceof SyntaxError);
row(caught(() => new Function("a){", "")) instanceof SyntaxError);
row(caught(() => Function("a){ return 1 }; function b(", "")) instanceof SyntaxError);
row(caught(() => Function("a", "a", "'use strict';")) instanceof SyntaxError);
row(caught(() => Function("return 1 +")) instanceof SyntaxError);
row(caught(() => new Function("a", "} return 1; {")) instanceof SyntaxError);
row(caught(() => new Function("a /*", "*/ return 1")) instanceof SyntaxError);
row(caught(() => new Function("a) { /*", "*/ }")) instanceof SyntaxError);
row(caught(() => new Function("eval", "\"use strict\"; return eval")) instanceof SyntaxError);
row(caught(() => new Function("a = 1", "\"use strict\"; return a")) instanceof SyntaxError);
row(caught(() => new Function("return super.x")) instanceof SyntaxError);
row((function () { const e = caught(() => Function("(")); return e.name === "SyntaxError" && typeof e.message === "string" && e.message.length > 0; })());
// Nothing in a body that fails to compile runs.
var ran = 0;
row(caught(() => Function("ran = 1; )")) instanceof SyntaxError && ran === 0);
// A conversion that throws propagates and stops the rest.
const seen = [];
const bad = { toString() { seen.push("bad"); throw 7; } };
const later = { toString() { seen.push("later"); return "x"; } };
row(caught(() => Function(bad, later)) === 7 && seen.join() === "bad");
row(caught(() => Function(Symbol.iterator)) instanceof TypeError);
// A run-time exception of the new function is its caller's.
row(caught(() => Function("throw 8")()) === 8);
row(caught(() => Function("return missing")()) instanceof ReferenceError);
failed === 0 ? rows : -failed;
