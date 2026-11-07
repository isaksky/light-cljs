// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 12069-12119,
// addition_concatenates_strings (bbp-66): when either operand of `+` is a
// string, the ToString of both concatenate, left to right; otherwise the
// operands add as numbers. Rows added here: a string of UTF-16 units beside
// a number, and an object operand, which converts by ToPrimitive first.
// Infinity and NaN are spelled 1 / 0 and 0 / 0. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
row("a" + 1 === "a1");
row(1 + "a" === "1a");
row(1 + 2 + "3" === "33");
row("3" + 1 + 2 === "312");
row("" + true === "true");
row(false + "" === "false");
row("" + null === "null");
row("" + undefined === "undefined");
row("" + -0 === "0");
// `test_cvt`.
row("" + 19686109595169230000 === "19686109595169230000");
row("" + 1e21 === "1e+21");
row("" + 1.5e-7 === "1.5e-7");
row("" + 0.000001 === "0.000001");
row("" + 0.1 === "0.1");
// An exact decimal tie takes the even last digit.
row("" + 1000000000000000.25 === "1000000000000000.2");
row("" + -1000000000000000.25 === "-1000000000000000.2");
row("" + (0.1 + 0.2) === "0.30000000000000004");
row("" + 0 / 0 === "NaN");
row("" + -1 / 0 === "-Infinity");
// Operands that are not statically known still convert at run time.
{ let f = (a, b) => a + b; row(f(2, "x") + f(null, "") === "2xnull"); }
// The shared `assert` helper's message.
{
  let actual = 1, expected = true;
  row("assertion failed: got |" + actual + "|" + ", expected |" + expected + "|" === "assertion failed: got |1|, expected |true|");
}
// A string of UTF-16 units keeps them; the number's text joins it.
row("ā" + 1 === "ā1" && 1 + "ā" + 2 === "1ā2");
row(("x" + 1.5 + "☃").length === 5);
// An object converts by ToPrimitive, then concatenates or adds.
row("a" + {valueOf() { return 7; }} === "a7");
row({valueOf() { return 1.5; }} + "" === "1.5");
row({valueOf() { return null; }} + "x" === "nullx");
row({valueOf() { return 1; }} + 2 === 3);
// Without a string operand, `+` stays numeric.
row(1 + 2 === 3);
row(true + 1 === 2);
{ let n = null, t = true; row(n + t === 1); }
{ let u = undefined + 1; row(u !== u); }
// Both operands are evaluated before the conversion: an unresolvable right
// operand fails with its ReferenceError.
row(name(() => "x" + nope) === "ReferenceError");
{
  let log = "";
  function l() { log += "l"; return 1; }
  function r() { log += "r"; return "s"; }
  row(l() + r() === "1s" && log === "lr");
}
// `+=` on a string binding.
{ let s = "n="; s += 4.5; s += true; row(s === "n=4.5true"); }
failed === 0 ? rows : -failed;
