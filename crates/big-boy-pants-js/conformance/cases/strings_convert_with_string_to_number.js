// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 12146-12251,
// strings_convert_with_string_to_number (bbp-67, StringToNumber), and the
// rows of rust/src/value/string.rs string_to_number's grammar: whitespace
// (including U+FEFF, U+3000 and NBSP) trims, an empty string is 0, `0x`,
// `0o` and `0b` integers are unsigned, decimals take a sign, fraction and
// exponent, `Infinity` takes a sign, and anything else is NaN. Rows added
// here: long inputs (parse_numbers_long_inputs_are_budgeted_and_rounded's
// lengths, as StringToNumber), radix integers past 2^53 rounded half to even
// against their literals, decimal strings against the same literal, and a
// round trip of about 1,500 generated doubles through their ToString. The
// rows with `**`, `~` and the bitwise and shift operators wait for those
// operators. Infinity and NaN are spelled 1 / 0 and 0 / 0. Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function nan(x) { return x !== x; }
function negzero(x) { return x === 0 && 1 / x < 0; }
var inf = 1 / 0;
// bbp-67's acceptance rows.
row(+"12345" === 12345);
row(+"" === 0 && !negzero(+""));
row(+"  \n12\t" === 12);
row(+" 1\uFEFF" === 1);
row(+" 1﻿" === 1);
row(+"  \u3000 7 \r" === 7);
row(+"0x12345" === 74565);
row(+"0X1f" === 31);
row(+"0b11" === 3);
row(+"0o17" === 15);
row(nan(+"-0x1"));
row(+"1e3" === 1000);
row(+".5" === 0.5);
row(+"5." === 5);
row(negzero(+"-0"));
row(+"Infinity" === inf);
row(+"+Infinity" === inf);
row(+"-Infinity" === -inf);
row(nan(+"infinity"));
row(nan(+"1_000"));
row(nan(+"1n"));
row(+"010" === 10);
row(nan(+"12px"));
// The other operators convert their string operands the same way.
row("6" * "7" === 42);
row("3" - 1 === 2);
row(-"2" === -2);
row("7" / "2" === 3.5);
row("7" % "4" === 3);
{ let s = "41"; s++; row(s === 42); }
{ let s = " 8 "; row(--s === 7 && s === 7); }
{ let s = "5"; let old = s++; row(old === 5 && typeof old === "number" && s === 6); }
{ let s = "3"; s *= "4"; row(s === 12); s -= "2"; row(s === 10); }
{ let f = (a, b) => a * b; row(f("3", " 4 ") === 12); }
// More that are NaN.
row(nan(+"0x"));
row(nan(+"0b"));
row(nan(+"0o"));
row(nan(+"."));
row(nan(+"1e"));
row(nan(+"1e+"));
row(nan(+"--1"));
row(nan(+"+-1"));
row(nan(+"0x1g"));
row(nan(+"0b2"));
row(nan(+"0o8"));
row(nan(+"+0x1"));
row(nan(+"1 2"));
row(nan(+"Infinityx"));
row(nan(+"inf"));
row(nan(+"NaN") && nan(+"nan"));
row(nan(+"e5"));
row(nan(+"\u00E9"));
row(nan(+"\uFF11"));
row(nan(+"1\u200B"));
row(nan("abc" * 1));
{ let a = "a"; row(nan(a - 1)); }
{ let s = "x"; s++; row(nan(s)); }
// More that convert.
row(+"-.5" === -0.5);
row(+"+.5e1" === 5);
row(+"5.e2" === 500);
row(+"1E+2" === 100);
row(+"1e-2" === 0.01);
row(+"-1.5e-3" === -0.0015);
row(+"00012" === 12);
row(+"0.0" === 0);
row(negzero(+"-0.0e5"));
row(+"\u00A012\u00A0" === 12);
row(+"\u2028\u2029\u1680\u2000\u200A\u202F\u205F 3" === 3);
row(+"\t\v\f\r\n 4" === 4);
row(+" \u00A0 " === 0);
row(+"\u3000" === 0);
row(+"0x0" === 0 && +"0b0" === 0 && +"0o0" === 0);
row(+"0xABCDEF" === 11259375 && +"0xabcdef" === 11259375);
row(+"0O777" === 511 && +"0B101" === 5);
row(+"0x00000000000000000001" === 1);
// Decimals are correctly rounded, as the literal is.
row(+"9007199254740993" === 9007199254740993);
row(+"9007199254740993" === 9007199254740992);
row(+"9007199254740995" === 9007199254740996);
row(+"9007199254740993.0000000000000000000001" === 9007199254740994);
row(+"0.1" === 0.1 && +"0.2" === 0.2 && +"0.3" === 0.3);
row(+"1.7976931348623157e308" === 1.7976931348623157e308);
row(+"1.7976931348623158e308" === 1.7976931348623157e308);
row(+"1.7976931348623159e308" === inf);
row(+"-1e400" === -inf);
row(+"5e-324" === 5e-324);
row(+"2.4703282292062328e-324" === 5e-324);
row(+"2.4703282292062327e-324" === 0);
row(negzero(+"-1e-400"));
row(+"2.2250738585072011e-308" === 2.2250738585072011e-308);
row(+"123456789012345678901234567890" === 123456789012345678901234567890);
row(+"1e99999999999999999999" === inf);
row(+"0e99999999999999999999" === 0);
row(+"0.000001" === 1e-6 && +"1e21" === 1e21);
// Radix integers past 2^53 round half to even, as their literals do.
row(+"0x1fffffffffffff" === 0x1fffffffffffff);
row(+"0x20000000000001" === 0x20000000000000);
row(+"0x20000000000003" === 0x20000000000004);
row(+"0x20000000000005" === 0x20000000000004);
row(+"0x20000000000001000000000" === 0x20000000000000000000000);
row(+"0x20000000000003000000000" === 0x20000000000004000000000);
row(+"0x20000000000001000000001" === 0x20000000000002000000000);
row(+"0x20000000000001000000001" === 0x20000000000001000000001);
row(+"0x20000000000002800000000" === 0x20000000000002000000000);
row(+"0b100000000000000000000000000000000000000000000000000011" === 0b100000000000000000000000000000000000000000000000000011);
row(+"0b100000000000000000000000000000000000000000000000000011" === 0b100000000000000000000000000000000000000000000000000100);
row(+"0o777777777777777777777777" === 0o777777777777777777777777);
row(+"0xfffffffffffff800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000" === 1.7976931348623157e308);
row(+"0xfffffffffffffc00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000" === inf);
// Long inputs, built by doubling (8,192 units each; 250 for f).
{
  let z = "0"; for (let i = 0; i < 13; i++) z = z + z;
  let w = " "; for (let i = 0; i < 13; i++) w = w + w;
  let f = "fffff"; f = f + f + f + f + f; f = f + f; f = f + f + f + f + f;
  let p = 1; for (let i = 0; i < 1000; i++) p = p * 2;
  row(+("0." + z + "1e8193") === 1);
  row(+("1." + z + "1") === 1);
  row(+(z + "1") === 1);
  row(+("0b" + z + "1") === 1);
  row(+(w + "0xff" + w) === 255);
  row(nan(+(w + "12" + w + "3")));
  row(+(w) === 0);
  row(+("0x" + f) === p);
  row(+("0x" + f + f) === inf);
  // (2^53 - 1) * 2^972 is past the largest double, which is that times
  // 2^971; and 2^968 times it is finite, an eighth of the largest.
  let y = ""; for (let i = 0; i < 242; i++) y = y + "0";
  row(+("0x1fffffffffffff" + y + "0") === inf);
  row(+("0x1fffffffffffff" + y) === 1.7976931348623157e308 / 8);
  row(+("0x1fffffffffffff" + y + "0") !== 1.7976931348623157e308);
  row(+("1" + z) === inf);
  row(+("-1" + z) === -inf);
  row(+("0." + z + "1") === 0);
}
// A round trip: ToString of a double reads back as the same double.
{
  let x, i, ok = true, n = 0;
  function check(v) { n++; if (+("" + v) !== v || +(" " + v + "\n") !== v) ok = false; }
  x = 1; for (i = 0; i < 300; i++) { x = x * 1.7; check(x); }
  x = 1; for (i = 0; i < 300; i++) { x = x / 1.7; check(x); }
  x = 1; for (i = 0; i < 100; i++) { x = x * -3.3; check(x); }
  x = 1; for (i = 0; i < 308; i++) { x = x * 10; check(x); }
  x = 1; for (i = 0; i < 323; i++) { x = x / 10; check(x); }
  x = 5e-324; for (i = 0; i < 70; i++) { x = x * 3; check(x); }
  let seed = 42;
  for (i = 0; i < 200; i++) { seed = (seed * 48271) % 2147483647; check(seed / 2147483647); check(seed); }
  row(ok && n === 1801);
}
failed === 0 ? rows : -failed;
