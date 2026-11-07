// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 13289-13420,
// bitwise_operators_follow_to_int32 (bbp-62; ECMAScript ToInt32 and
// ToUint32, the shift count masked to 5 bits): `&`, `|`, `^`, `<<`, `>>`,
// `>>>` and `~` on numbers, the other primitives (ToNumber, strings
// included) and objects with `valueOf` (ToPrimitive with hint number, left
// operand first). Results are never -0. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function positiveZero(n) { return n === 0 && 1 / n === Infinity; }
var inf = 1 / 0, nan = 0 / 0;
// `test_op1`.
row((4 << 2) === 16 && (1 << 0) === 1 && (1 << 31) === -2147483648 && (1 << 32) === 1);
row((-4 >> 1) === -2 && (-4 >>> 1) === 2147483646);
row((1 & 1) === 1 && (0 | 1) === 1 && (1 ^ 1) === 0 && ~1 === -2);
row((1 << 31) < 0);
// The numeric groups of `test_cvt`.
row((nan | 0) === 0 && (inf | 0) === 0 && (-inf | 0) === 0 && ((4294967296 * 3 - 4) | 0) === -4);
row((nan >>> 0) === 0 && (inf >>> 0) === 0 && (-inf >>> 0) === 0 && ((4294967296 * 3 - 4) >>> 0) === 4294967292);
// ToInt32 and ToUint32 edges.
row((2147483648 | 0) === -2147483648 && (2147483647 | 0) === 2147483647);
row((4294967295 | 0) === -1 && (-2147483649 | 0) === 2147483647);
row((1e21 | 0) === -559939584 && (-3.9 | 0) === -3);
row((1 << -1) === -2147483648 && (1 << 33) === 2 && (8 >> 4294967297) === 4);
row((1 >>> 32) === 1 && (-1 >>> 0) === 4294967295 && (-1 >>> 31) === 1 && (-1 >> 31) === -1);
row((4294967295 >>> 0) === 4294967295);
row(~~3.7 === 3 && ~~-3.7 === -3 && ~nan === -1 && ~inf === -1 && ~-1 === 0 && ~2147483647 === -2147483648);
row((6 & 3) === 2 && (6 | 3) === 7 && (6 ^ 3) === 5 && (5.9 & 7.9) === 5);
row((0xF0F0F0F0 & 0x0FF00FF0) === 0x00F000F0 && (0x80000000 ^ 1) === -2147483647);
// ToNumber of the other primitives.
row((true | 0) === 1 && (false | 2) === 2 && (null >>> 0) === 0 && (undefined | 0) === 0);
row(~undefined === -1 && ~true === -2 && ~null === -1);
{ let t = true; row((t << t) === 2); }
row(("12" | 0) === 12 && (" 0x10 " | 1) === 17 && ("abc" | 0) === 0 && ~"5" === -6 && ("-1" >>> "28") === 15);
// Precedence.
row((1 | 2 ^ 3 & 1) === 3 && (1 << 2 + 1) === 8 && (2 & 1 == 1) === 0 && 0 < 1 << 2);
// Results are never -0.
row(positiveZero(-0 | 0) && positiveZero(-0 & -0) && positiveZero(-0 ^ 0) && positiveZero(-0 << 1));
row(positiveZero(-0 >> 0) && positiveZero(-0 >>> 0) && positiveZero(-0.5 | 0) && positiveZero(~-1));
row(positiveZero(~~-0) && positiveZero(-1 & 0));
// Both operands are evaluated, left first.
{ let x = 0; let y = (x = 6) & (x = 3); row(y * 10 + x === 23); }
// Objects convert through valueOf (ToPrimitive, hint number), left first.
{
  var log = "";
  var a = { valueOf: function () { log += "a"; return 6; } }, b = { valueOf: function () { log += "b"; return "3"; } };
  row((a & b) === 2 && log === "ab");
  row((b << a) === 192 && log === "abba");
  row((a >>> b) === 0 && ~a === -7 && log === "abbaaba");
  row(({ toString: function () { return "9"; } } ^ 1) === 8);
}
// An object whose valueOf and toString both give objects: a TypeError.
{
  var r = "none";
  var stuck = { valueOf: function () { return {}; }, toString: function () { return {}; } };
  try { ~stuck; } catch (e) { r = e.name; }
  row(r === "TypeError");
}
failed === 0 ? rows : -failed;
