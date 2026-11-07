// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/vm/dispatch.rs lines 349-371,
// to_js_string (bbp-66) and ECMAScript ToString, whose step for a Symbol
// throws a TypeError: an implicit conversion of a symbol to a string, by `+`
// with a string or by a template substitution, is a catchable TypeError, and
// the conversion happens after both operands are evaluated. A symbol as a
// property key is no conversion. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
const s = Symbol.iterator;
row(name(() => "" + s) === "TypeError");
row(name(() => s + "") === "TypeError");
row(name(() => "a" + s + "b") === "TypeError");
row(name(() => `${s}`) === "TypeError");
row(name(() => `a${s}b`) === "TypeError");
row(name(() => { let t = "x"; t += s; }) === "TypeError");
{ let e; try { "" + s; } catch (x) { e = x; } row(typeof e === "object" && e.name === "TypeError"); }
{ let e; try { `${s}`; } catch (x) { e = x; } row(typeof e === "object" && typeof e.message === "string"); }
// A substitution before the symbol still runs. (QuickJS evaluates every
// substitution before it converts any, so none follows the symbol here.)
{
  let log = "";
  function f(x) { log += x; return x; }
  try { `${f("a")}${s}`; } catch (e) { log += e.name; }
  row(log === "aTypeError");
}
// Through ToPrimitive: the symbol valueOf gives converts like a direct
// one, with the same error and message.
{
  const o = {valueOf() { return s; }};
  let direct, mediated;
  try { "" + s; } catch (e) { direct = e; }
  try { "" + o; } catch (e) { mediated = e; }
  row(mediated.name === "TypeError" && mediated.message === direct.message);
}
// A built-in's ToString of a symbol (an Error message) throws too.
row(name(() => Error(s)) === "TypeError");
// A symbol key is no conversion.
{ let o = {}; o[s] = 1; row(o[s] === 1 && s in o && !("Symbol(Symbol.iterator)" in o)); }
row(typeof s === "symbol");
failed === 0 ? rows : -failed;
