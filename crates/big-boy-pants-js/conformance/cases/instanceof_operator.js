// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 14895-14970,
// instanceof_operator (bbp-75): constructed objects, two-level chains,
// primitives, the realm's constructors, functions, the chain read by
// identity when the operator runs, evaluation order, and catchable
// TypeErrors. Object.create and Object.setPrototypeOf (the objects area's)
// become `__proto__` literals; the Pants-only `__rt` row is left out. Caught
// errors are instances of their constructors (light-cljs-825's rows, which
// compared e.name until now). Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function F() {}
function G() {}
G.prototype = new F();
row(new F() instanceof F);
row(!(({}) instanceof F));
row(!(1 instanceof F));
row(!("s" instanceof F) && !(undefined instanceof F) && !(null instanceof F) && !(true instanceof F));
row(({}) instanceof Object);
row(!(({}) instanceof String) && !(({}) instanceof Number) && !(({}) instanceof Boolean));
row(new F() instanceof Object);
row(!(new F() instanceof G));
// A two-level chain: an instance of both, and of Object.
{ const g = new G(); row(g instanceof G && g instanceof F && g instanceof Object); }
{ const c = {__proto__: new F()}; row(c instanceof F && c instanceof Object); }
{ const c = {__proto__: G.prototype}; row(c instanceof G && c instanceof F); }
// A null-prototype object is an instance of nothing.
row(!({__proto__: null} instanceof Object));
// Arrays and the realm's Array constructor.
{ const A = [].constructor; row([] instanceof A && [] instanceof Array && [] instanceof Object && !(({}) instanceof A)); }
// Functions inherit from Function.prototype.
row(F instanceof Function && F instanceof Object && (() => 1) instanceof Function);
row(Object instanceof Function && Object.is instanceof Object && !(new F() instanceof Function));
row(!({__proto__: F} instanceof F) && {__proto__: F} instanceof Function);
// Wrapper objects are instances; primitives are not.
row(new Number(1) instanceof Number && new String("a") instanceof String && new Boolean(false) instanceof Boolean && new String("a") instanceof Object);
row(!(1 instanceof Number) && !("a" instanceof String) && !(false instanceof Boolean) && !(1 instanceof Object));
// The chain is read when the operator runs, by identity.
{ function K() {} const o = new K(); K.prototype = {}; row(!(o instanceof K) && new K() instanceof K); }
{ const p = {}; function H() {} H.prototype = p; row({__proto__: p} instanceof H); }
// A function whose prototype nothing has read has no instances yet.
{ function U() {} row(!(({}) instanceof U) && !([] instanceof U)); }
// A primitive left operand is false before `prototype` is read.
{ function P() {} P.prototype = 1; row(!(1 instanceof P) && !(undefined instanceof P)); }
// Relational precedence: below `+`, above `==`, left-associative.
row(new F() instanceof F == true);
row((0 + 1 instanceof F) === false);
row(new F() instanceof F === true);
row((new F() instanceof F instanceof Object) === false);
// The left operand is evaluated first, then the right one, then the check.
{
  let log = "";
  function l() { log += "l"; return {}; }
  function r() { log += "r"; return 1; }
  try { l() instanceof r(); } catch (e) { log += "e"; }
  row(log === "lre");
}
// Catchable TypeErrors: a right operand that is not an object, or not
// callable, and a `prototype` that is not an object.
row(name(() => ({}) instanceof 1) === "TypeError");
row(name(() => ({}) instanceof {}) === "TypeError");
row(name(() => 1 instanceof undefined) === "TypeError");
row(name(() => ({}) instanceof null) === "TypeError");
row(name(() => ({}) instanceof "s") === "TypeError");
row(name(() => ({}) instanceof Symbol.iterator) === "TypeError");
row(name(() => ({}) instanceof []) === "TypeError");
row(name(() => ({}) instanceof new Number(1)) === "TypeError" && name(() => ({}) instanceof new Error("e")) === "TypeError");
{ function P() {} P.prototype = 1; row(name(() => new Object() instanceof P) === "TypeError"); }
{ const arrow = () => 1; row(name(() => ({}) instanceof arrow) === "TypeError" && !(1 instanceof arrow)); }
row(name(() => nope instanceof F) === "ReferenceError");
row(name(() => ({}) instanceof nope) === "ReferenceError");
// Caught errors are instances of their constructors.
{ let e; try { nope; } catch (x) { e = x; } row(e instanceof ReferenceError && e instanceof Error && e instanceof Object && !(e instanceof TypeError)); }
{ let e; try { null.x; } catch (x) { e = x; } row(e instanceof TypeError && e instanceof Error && !(e instanceof RangeError)); }
{ let e; try { ({}) instanceof 1; } catch (x) { e = x; } row(e instanceof TypeError); }
row(new RangeError("r") instanceof RangeError && new Error("e") instanceof Error && !(new Error("e") instanceof TypeError));
failed === 0 ? rows : -failed;
