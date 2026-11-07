// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/value/string.rs lines 2756-2799,
// number_to_string_matches_javascript, and bbp-66's acceptance formatter
// corpus (Number::toString in radix 10): each number converts by `+` with a
// string and by an untagged template substitution, both ToString. Infinity
// and NaN are spelled 1 / 0 and 0 / 0. Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function spells(x, s) { return "" + x === s && `${x}` === s && x + "" === s; }
row(spells(0, "0"));
row(spells(-0, "0"));
row(spells(1, "1"));
row(spells(-1, "-1"));
row(spells(0.1, "0.1"));
row(spells(-0.1, "-0.1"));
row(spells(1.5, "1.5"));
row(spells(1e21, "1e+21"));
row(spells(-1e21, "-1e+21"));
row(spells(1e20, "100000000000000000000"));
row(spells(999999999999999900000, "999999999999999900000"));
row(spells(1e-7, "1e-7"));
row(spells(1.5e-7, "1.5e-7"));
row(spells(0.000001, "0.000001"));
row(spells(0.0000015, "0.0000015"));
row(spells(0.00001, "0.00001"));
row(spells(1.5e300, "1.5e+300"));
row(spells(5e-324, "5e-324"));
row(spells(-5e-324, "-5e-324"));
row(spells(2.2250738585072014e-308, "2.2250738585072014e-308"));
row(spells(1.7976931348623157e308, "1.7976931348623157e+308"));
row(spells(9007199254740992, "9007199254740992"));
row(spells(9007199254740993, "9007199254740992"));
row(spells(19686109595169230000, "19686109595169230000"));
row(spells(123e-20, "1.23e-18"));
row(spells(2147483648, "2147483648"));
row(spells(4294967295, "4294967295"));
row(spells(4294967296, "4294967296"));
row(spells(123456789012345680000, "123456789012345680000"));
row(spells(1 / 3, "0.3333333333333333"));
row(spells(2 / 3, "0.6666666666666666"));
row(spells(0.1 + 0.2, "0.30000000000000004"));
row(spells(100, "100"));
row(spells(1e15, "1000000000000000"));
row(spells(12345.678, "12345.678"));
row(spells(4.35, "4.35"));
row(spells(1.0000000000000002, "1.0000000000000002"));
// An exact decimal tie takes the even last digit.
row(spells(1000000000000000.25, "1000000000000000.2"));
row(spells(-1000000000000000.25, "-1000000000000000.2"));
row(spells(1000000000000000.75, "1000000000000000.8"));
row(spells(0.5, "0.5") && spells(0.25, "0.25") && spells(0.5e-6, "5e-7"));
row(spells(123.456, "123.456") && spells(-123.456, "-123.456"));
row(spells(1.5e21, "1.5e+21") && spells(-1.5e-7, "-1.5e-7"));
row(spells(0 / 0, "NaN"));
row(spells(1 / 0, "Infinity"));
row(spells(-1 / 0, "-Infinity"));
// Built-ins convert numbers the same way: an Error message, and the name
// and message Error.prototype.toString joins.
row(Error(1.5).message === "1.5" && Error(-0).message === "0" && Error(1e21).message === "1e+21");
row(Error.prototype.toString.call({name: 0.5, message: 5e-324}) === "0.5: 5e-324");
failed === 0 ? rows : -failed;
