// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 12684-12777,
// typeof_operator (bbp-69): the rows that need no Pants-only `__rt`
// intrinsic, plus a well-known symbol, generator, async and built-in
// functions. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// Every primitive kind.
row(typeof 1 === "number");
row(typeof NaN === "number");
row(typeof -0 === "number");
row(typeof true === "boolean");
row(typeof false === "boolean");
row(typeof "s" === "string");
row(typeof "" === "string");
row(typeof undefined === "undefined");
row(typeof null === "object");
row(typeof Symbol.iterator === "symbol");
// Objects and functions.
row(typeof {} === "object");
row(typeof [] === "object");
row(typeof [1, 2] === "object");
row(typeof function () {} === "function");
function f() {}
row(typeof f === "function");
row(typeof (() => 1) === "function");
{ let k = 2; row(typeof (() => k) === "function"); }
function g() { return () => 1; }
row(typeof g() === "function");
row(typeof function* () {} === "function");
row(typeof async function () {} === "function");
row(typeof (async () => 1) === "function");
row(typeof Object === "function");
row(typeof Object.prototype.toString === "function");
row(typeof Object.is === "function");
row(typeof Object.prototype === "object");
// Caught values.
{ let t; try { missing; } catch (e) { t = typeof e; } row(t === "object"); }
{ let t; try { throw "x"; } catch (e) { t = typeof e; } row(t === "string"); }
// Bindings: the type of the current value.
{ let u; row(typeof u === "undefined"); }
{ let n = null; row(typeof n === "object"); }
{ let a = 1; a = "one"; row(typeof a === "string"); }
// An undeclared name, also parenthesized, inside a function and a closure.
row(typeof unknown_var === "undefined");
row(typeof (unknown_var) === "undefined");
row(typeof ( /* c */ (nope)) === "undefined");
function h() { return typeof nope; }
row(h() === "undefined");
{ let c = () => typeof nope; row(c() === "undefined"); }
// `typeof` inside an expression; the result is an ordinary string.
row(typeof typeof 1 === "string");
row(typeof typeof unknown_var === "string");
row(typeof 1 + typeof "a" === "numberstring");
row((typeof 1 === "number") + 1 === 2);
row((typeof null === "object" ? 3 : 4) === 3);
row(typeof 1 === "number" && typeof null === "object" && typeof typeof 1 === "string");
// The operand is evaluated: its effects and failures happen.
{ let i = 0; typeof (i = i + 1); typeof i++; row(i === 2); }
{ let t = "none"; try { typeof (0, nope); } catch (e) { t = e.name; } row(t === "ReferenceError"); }
{ let t = "none"; try { typeof nope(); } catch (e) { t = e.name; } row(t === "ReferenceError"); }
{ let t = "none"; try { typeof x == "undefined" && !x; } catch (e) { t = e.name; } row(t === "ReferenceError"); }
// A binding in its temporal dead zone still throws.
{ let t = "none"; try { typeof tdz; let tdz; } catch (e) { t = e.name; } row(t === "ReferenceError"); }
{
  let late = () => typeof y, before;
  try { late(); before = "ran"; } catch (e) { before = e.name; }
  let y = 1;
  row(before === "ReferenceError" && late() === "number");
}
failed === 0 ? rows : -failed;
