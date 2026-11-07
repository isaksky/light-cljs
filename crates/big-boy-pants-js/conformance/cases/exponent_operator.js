// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13450-13560,
// exponent_operator (bbp-63; ECMAScript Number::exponentiate, which is C's
// pow except that a non-finite exponent with base 1 or -1 is NaN), and line
// 13584 of update_expressions_on_bindings (`a++ ** 2`, the row
// light-cljs-825 left for this operator): `**` on numbers, the other
// primitives and objects with `valueOf`, and its right associativity.
// Infinity and NaN are spelled 1 / 0 and 0 / 0. Each row is a `row(...)`
// check; the script completes with the number of rows, or minus the first
// failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function nan(x) { return x !== x; }
function positiveZero(n) { return n === 0 && 1 / n > 0; }
function negativeZero(n) { return n === 0 && 1 / n < 0; }
var inf = 1 / 0, notNumber = 0 / 0;
// `test_op1`.
row(2 ** 8 === 256);
row(2 ** 3 ** 2 === 512 && (2 ** 3) ** 2 === 64 && (-2) ** 2 === 4 && (-2) ** 3 === -8);
row(2 ** -1 === 0.5 && -(2 ** 2) === -4 && 2 * 3 ** 2 === 18);
row(notNumber ** 0 === 1 && notNumber ** -0 === 1 && 0 ** 0 === 1);
row(0 ** -1 === inf && (-0) ** -1 === -inf && (-0) ** -2 === inf);
row(positiveZero((-0) ** 2) && negativeZero((-0) ** 3));
row(positiveZero(inf ** -1) && (-inf) ** 3 === -inf && negativeZero((-inf) ** -3));
row(2 ** inf === inf && positiveZero(0.5 ** inf) && positiveZero(2 ** -inf) && (-1) ** 2 === 1 && 1 ** 1e308 === 1);
// NaN, including where C's pow gives 1.
row(nan((-8) ** (1 / 3)) && nan(1 ** inf) && nan(1 ** -inf) && nan(1 ** notNumber));
row(nan((-1) ** inf) && nan((-1) ** -inf) && nan((-1) ** notNumber) && nan(notNumber ** 1) && nan(2 ** notNumber));
// ToNumber of the other primitives.
row(true ** 2 === 1 && null ** 0 === 1 && 2 ** true === 2 && 2 ** null === 1 && false ** false === 1);
row(nan(2 ** undefined) && undefined ** 0 === 1 && "3" ** "2" === 9 && nan("x" ** 1));
// Both operands are evaluated, left first.
{ let x = 0; let y = (x = 2) ** (x = 3); row(y * 10 + x === 83); }
{ const sq = (x) => x ** 2; let b = 2; row(sq(b ** b ** b) / 2 ** -1 === 512); }
// An update expression as the base.
{ let a = 3; row(a++ ** 2 === 9 && ++a ** 2 === 25 && a === 5); }
{ let a = 3; row(a-- ** 2 === 9 && (--a) ** 3 === 1); }
// Objects convert through valueOf, left first.
{
  var log = "";
  var a = { valueOf: function () { log += "a"; return 2; } }, b = { valueOf: function () { log += "b"; return "10"; } };
  row(a ** b === 1024 && log === "ab" && b ** a === 100 && log === "abba");
}
failed === 0 ? rows : -failed;
