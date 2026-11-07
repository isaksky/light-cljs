// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9024-9043,
// primitives_convert_with_to_number (bbp-60), and the relational rows of
// lines 12146-12251, strings_convert_with_string_to_number (bbp-67), with
// rust/src/vm/dispatch.rs is_less_than (bbp-68): `<`, `>`, `<=` and `>=`
// compare two strings by code units and any other pair of primitives by
// their ToNumber values, where NaN (undefined, a string that is not
// numeric) makes every comparison false. ToNumber of undefined is NaN, of
// null +0, of a boolean 0 or 1. Rows added here: each operator over mixed
// string, number, boolean, null and undefined operands, and the arithmetic
// operators over the same. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function nan(x) { return x !== x; }
// primitives_convert_with_to_number.
row(true + 1 === 2);
row(null + 1 === 1);
row(nan(undefined + 1));
row(+true === 1);
row(+false === 0);
row(nan(+undefined));
row(-null === 0 && 1 / -null < 0);
row(+null === 0 && 1 / +null > 0);
{ let t = true; row(t * 3 - false === 3); }
row((null < 1 && false < true) === true);
row((undefined < 1 || undefined > 1) === false);
// A string and a number: the string converts.
row(("10" < 9) === false);
row((9 < "10") === true);
row(("10" > 9) === true);
row(("10" >= 10) === true);
row(("10" <= 10) === true);
row(("abc" < 1) === false);
row(("abc" >= 1) === false);
row(("abc" <= 1) === false);
row(("abc" > 1) === false);
row(("" < 1) === true);
row((" 2 " > 1) === true);
row(("0x10" > 15) === true);
row(("1e3" >= 1000) === true);
row(("-Infinity" < -1e308) === true);
{ let t = true; row(("0.5" < t) === true); }
// Two strings compare by code units, never as numbers.
row(("10" < "9") === true);
row(("9" <= "10") === false);
{ let a = "1", b = "2"; row((a >= b) === false); }
{ let f = (a, b) => a > b; row(f("b", "a") === true && f("2", 10) === false && f("20", 10) === true); }
// Booleans, null and undefined.
row((true > false) === true);
row((true >= 1) === true);
row((false <= 0) === true);
row((true < 2) === true);
row((null >= 0) === true);
row((null <= 0) === true);
row((null > 0) === false);
row((null < true) === true);
row((null < "1") === true);
row((null >= "") === true);
row((true > "0") === true);
row((false < "") === false);
row((undefined < "1") === false);
row((undefined >= undefined) === false);
row((undefined <= null) === false);
row((null <= null) === true);
row(("x" < undefined) === false);
row(("1" > null) === true);
row((1 / 0 > "1e308") === true);
row((0 / 0 < "1") === false);
row(("NaN" <= 1 / 0) === false);
{ let u; row((u < 1) === false && (u >= 1) === false); }
// Arithmetic over the same operands.
row(true + true === 2);
row(null * 5 === 0);
row(nan(undefined * 5));
row(true - "1" === 0);
row("4" / true === 4);
row(false % "3" === 0);
row(null - "2" === -2);
row(nan(undefined - null));
row("2" * null === 0);
row(true + null === 1);
row(-true === -1 && -"" === 0 && 1 / -"" < 0);
{ let b = true; b++; row(b === 2); }
{ let n = null; n--; row(n === -1); }
{ let u; u++; row(nan(u)); }
{ let f = false; row(++f === 1 && typeof f === "number"); }
failed === 0 ? rows : -failed;
