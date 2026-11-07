// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 15020-15153,
// in_operator (bbp-74): own, inherited and index keys, array `length` and
// holes, functions, evaluation order, and a TypeError for a right operand
// that is not an object. The rows with a Pants-only `__rt` collection and
// number keys that need the number-to-string conversion are left out. Each
// row is a `row(...)` check; the script completes with the number of rows,
// or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
// Own keys.
row("x" in {x: 2});
row(!("y" in {x: 2}));
row("x" in {x: undefined});
row(1 in {1: 0});
row("1" in {1: 0} && 1 in {"1": 0});
row(!(2 in {1: 0}));
{ let a = {x: 2}; row(("x" in a) === true && ("y" in a) === false); }
{ const o = {}; const before = "k" in o; o.k = 0; row(!before && "k" in o); }
// Inherited keys, through any chain; the value does not matter.
row("toString" in {});
row("valueOf" in [] && "constructor" in {} && "constructor" in []);
{ const p = {a: undefined}; const o = {__proto__: p}; row("a" in o && !("b" in o)); }
{ const o = {__proto__: null}; row(!("toString" in o) && !("a" in o)); }
{ const o = {__proto__: {q: 0}}; row("q" in o); }
{ const p = {deep: 1}, q = {__proto__: p}, r = {__proto__: q}; row("deep" in r && !("deep" in {})); }
// Arrays: elements, holes, `length`.
row(0 in [5] && !(1 in [5]) && "length" in []);
{ const a = [1]; a[2] = 3; row(!(1 in a) && 2 in a && 0 in a); }
row("0" in ["a"] && !("1" in ["a"]));
{ const a = []; a[3] = 1; row(3 in a && !(0 in a) && "length" in a); }
// Functions.
{ function f() {} row("prototype" in f && "valueOf" in f && !("x" in f)); }
{ function f() {} row("call" in f && "constructor" in f); }
row("is" in Object && !("x" in Object) && "call" in Object && "apply" in Object.is);
{ let e; try { nope; } catch (x) { e = x; } row("message" in e); }
// Precedence: below `+`, above `==`.
row("ab" in {ab: 1} === true);
row(("a" + "b" in {ab: 1}) === true);
// The left operand is evaluated first, then the right one, then the right
// one is checked.
{
  let log = "";
  function l() { log += "l"; return "x"; }
  function r() { log += "r"; return 1; }
  try { l() in r(); } catch (e) { log += "e"; }
  row(log === "lre");
}
row(name(() => nope in {}) === "ReferenceError");
row(name(() => ({}) in nope) === "ReferenceError");
// A right operand that is not an object is a TypeError.
row(name(() => "x" in 1) === "TypeError");
row(name(() => "x" in null) === "TypeError");
row(name(() => "x" in undefined) === "TypeError");
row(name(() => "length" in "abc") === "TypeError");
row(name(() => 0 in true) === "TypeError");
row(name(() => "x" in Symbol.iterator) === "TypeError");
{ let r = false; try { "x" in 1; } catch (e) { r = e.name === "TypeError"; } row(r); }
failed === 0 ? rows : -failed;
