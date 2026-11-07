// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/vm/dispatch.rs lines 312-330,
// to_number (bbp-60) and ECMAScript ToNumber, whose step for a Symbol throws
// a TypeError (the conversion rows of bbp-127): unary `+` and `-`, `++` and
// `--`, the arithmetic operators, `+` without a string operand and the
// relational operators all throw a catchable TypeError for a symbol, after
// both operands are evaluated; `==` converts no symbol. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function message(thunk) { try { thunk(); return "none"; } catch (e) { return e.message; } }
const s = Symbol.iterator;
row(name(() => +s) === "TypeError");
row(name(() => -s) === "TypeError");
row(name(() => s * 1) === "TypeError");
row(name(() => 1 * s) === "TypeError");
row(name(() => s - 1) === "TypeError");
row(name(() => s / 2) === "TypeError");
row(name(() => s % 2) === "TypeError");
row(name(() => s + 1) === "TypeError");
row(name(() => true + s) === "TypeError");
row(name(() => null + s) === "TypeError");
row(name(() => s + s) === "TypeError");
row(name(() => s < 1) === "TypeError");
row(name(() => 1 > s) === "TypeError");
row(name(() => s <= "1") === "TypeError");
row(name(() => "1" >= s) === "TypeError");
row(name(() => s < s) === "TypeError");
row(name(() => { let t = s; t++; }) === "TypeError");
row(name(() => { let t = s; --t; }) === "TypeError");
row(name(() => { let t = s; t *= 2; }) === "TypeError");
row(name(() => { let t = 2; t -= s; }) === "TypeError");
{ let e; try { +s; } catch (x) { e = x; } row(typeof e === "object" && e.name === "TypeError" && typeof e.message === "string"); }
// The binding keeps its symbol when `++` throws.
{ let t = s; try { t++; } catch (e) {} row(t === s); }
// Both operands are evaluated before either converts.
{
  let log = "";
  function f(tag, x) { log += tag; return x; }
  try { f("a", s) * f("b", 1); } catch (e) { log += "!"; }
  try { f("c", 2) - f("d", s); } catch (e) { log += "!"; }
  try { f("e", s) < f("f", "3"); } catch (e) { log += "!"; }
  try { f("g", s) + f("h", 1); } catch (e) { log += "!"; }
  row(log === "ab!cd!ef!gh!");
}
// The message names the conversion: `+` with a string converts to a
// string, without one to a number, as `*` does.
row(message(() => s + 1) === message(() => s * 1));
row(message(() => s + 1) !== message(() => s + ""));
row(message(() => +s) === message(() => s < 1));
// Through ToPrimitive: the symbol valueOf gives converts like a direct one.
{
  const o = {valueOf() { return s; }};
  row(name(() => o + 1) === "TypeError");
  row(message(() => o + 1) === message(() => s + 1));
}
// `==` and `===` convert no symbol.
row((s == 1) === false && (s != 1) === true);
row((s == "x") === false && (s === s) === true);
row(name(() => s == 0) === "none");
row(typeof s === "symbol");
failed === 0 ? rows : -failed;
