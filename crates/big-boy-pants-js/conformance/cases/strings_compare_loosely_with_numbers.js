// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 12640-12680,
// strings_compare_loosely_with_numbers (bbp-67's loose-equality rows and
// QuickJS test_language.js test_eq): a string compared with a number, or a
// boolean compared with a string or number, compares ToNumber values; a
// string never equals null or undefined, and two strings compare by content.
// Rows added here: `!=` of each, whitespace and radix strings, a string that
// is not numeric against NaN and 0, and a symbol, which equals no other
// type. Each row is a `row(...)` check; the script completes with the
// number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
// test_eq.
row(("" == 0) === true);
row(("123" == 123) === true);
row(("122" != 123) === true);
// A number against a string compares with StringToNumber.
row((123 == "123") === true);
row(("abc" == NaN) === false);
row(("abc" != NaN) === true);
row(("NaN" == NaN) === false);
row((" 1 " == 1) === true);
row(("1e3" == 1000) === true);
row(("0x10" == 16) === true);
row(("0b101" == 5) === true);
row(("-0" == 0) === true);
row((0 == "-0") === true);
row(("1" != 1) === false);
row(("abc" == 0) === false);
row(("  " == 0) === true);
row(("\n" == 0) === true);
row(("0.1" == 0.1) === true);
row(("Infinity" == 1 / 0) === true);
row(("-Infinity" == -1 / 0) === true);
row(("1_0" == 10) === false);
row(("12px" == 12) === false);
// A boolean converts to a number first.
row(("1" == true) === true);
row(("0" == false) === true);
row(("2" == true) === false);
row((true == "1") === true);
row((false == "") === true);
row((false != " ") === false);
row(("true" == true) === false);
row(("false" == false) === false);
row(("0x1" == true) === true);
row((true == 1) === true && (false == 0) === true);
// A string never equals `null` or `undefined`.
row(("" == null) === false);
row((undefined == "") === false);
row(("null" == null) === false);
row(("undefined" == undefined) === false);
row(("" != null) === true);
// Two strings still compare by content, not as numbers.
row(("1" == "01") === false);
row(("1" == "1") === true);
row(("" == " ") === false);
// A symbol equals no other type, and itself.
{
  const s = Symbol.iterator;
  row((s == "Symbol(Symbol.iterator)") === false);
  row((s != 1) === true);
  row((s == true) === false);
  row((s == null) === false && (undefined == s) === false);
  row((s == s) === true);
}
// Operands that are not statically known compare at run time.
{ let f = (a, b) => a == b; row(f("5", 5) && f(5, "5") && !f("5", 6) && f(true, "1")); }
{ let g = (a, b) => a != b; row(g("5", 6) && !g(" 5", 5)); }
failed === 0 ? rows : -failed;
