// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9046-9096,
// truthiness_and_equality_follow_javascript (the strict rows), and lines
// 8971-9021, numbers_follow_ieee_double_semantics; plus `Object.is`, symbols
// and strings that are equal whether they came from a Latin-1 or a UTF-16
// literal or concatenation. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function negativeZero(n) { return n === 0 && 1 / n === -Infinity; }
function positiveZero(n) { return n === 0 && 1 / n === Infinity; }
function isNaNValue(v) { return v !== v; }
// Strict equality.
row((1 === 1) === true);
row((1 === true) === false);
row((null === undefined) === false);
row((undefined === undefined) === true);
row((null === null) === true);
row((0 !== -0) === false);
row((-0 === 0) === true);
row((true !== false) === true);
row((NaN === NaN) === false);
row((NaN !== NaN) === true);
row(("1" === 1) === false);
{ let o = {}; row(o === o && o !== {} && [] !== []); }
// SameValue: Object.is.
row(Object.is(NaN, NaN) === true);
row(Object.is(0, -0) === false);
row(Object.is(-0, -0) === true);
row(Object.is(0, 0) === true);
row(Object.is(1, 1) === true);
row(Object.is("a", "a") === true);
row(Object.is(null, undefined) === false);
row(Object.is(undefined, undefined) === true);
{ let o = {}; row(Object.is(o, o) === true && Object.is(o, {}) === false); }
row(Object.is(0 / 0, NaN) === true);
// Symbols.
row(Symbol.iterator === Symbol.iterator);
row(Symbol.iterator !== Symbol.asyncIterator);
row(Object.is(Symbol.iterator, Symbol.iterator) === true);
row(Object.is(Symbol.iterator, Symbol.asyncIterator) === false);
// Strings: equal by code units, however each operand was made.
row("é" === "é");
row("\xe9t\xe9" === "été");
row("Ā" === "Ā");
row("caf" + "é" === "café");
row("Ā" + "a" === "Āa");
row("a" + "Ā" + "b" === "aĀb");
row("\u{1F600}" === "😀");
row("\uD83D" + "\uDE00" === "\u{1F600}");
row("é" !== "e");
row("a" !== "Ā");
row("" === "" + "");
row(Object.is("é" + "Ā", "éĀ") === true);
// IEEE arithmetic: NaN and Infinity.
row(isNaNValue(0 / 0));
row(isNaNValue(NaN + 1));
row(1 / 0 === Infinity);
row(-1 / 0 === -Infinity);
row(1e308 * 10 === Infinity);
row(isNaNValue(Infinity - Infinity));
row(isNaNValue(Infinity * 0));
row(Infinity + 1 === Infinity);
row(Infinity / 2 === Infinity);
row(isNaNValue(Infinity / Infinity));
row(-Infinity < -1e308);
row((NaN < 1 || NaN >= 1 || NaN <= NaN) === false);
// Negative zero is produced and preserved, but equals zero.
row(negativeZero(-0));
row(negativeZero(0 * -1));
row(negativeZero(-0 + -0));
row(negativeZero(-1 % 1));
row(negativeZero(-0 % 5));
row(negativeZero(0 / -3));
row(negativeZero(-(0)));
row(positiveZero(-0 + 0));
row(1 / -0 === -Infinity);
row(1 / (0 * -1) === -Infinity);
// Remainder takes the dividend's sign.
row(-5 % 3 === -2);
row(5 % -3 === 2);
row(-5 % -3 === -2);
row(5.5 % 2 === 1.5);
row(isNaNValue(5 % 0));
row(isNaNValue(Infinity % 2));
row(7 % Infinity === 7);
row(negativeZero(-7 % 7));
// Integers are doubles: exact up to 2^53, then rounding to even.
row(9007199254740991 + 1 === 9007199254740992);
row(9007199254740992 + 1 === 9007199254740992);
row(9007199254740993 === 9007199254740992);
row(0.1 + 0.2 !== 0.3);
row(0.1 + 0.2 === 0.30000000000000004);
row(7 / 2 === 3.5);
row(0x10 + 0o10 + 0b10 === 26);
row(positiveZero(5e-324 / 2));
row((1 <= 1 && 2 >= 1 && 2 > 1 && !(1 > 1)) === true);
failed === 0 ? rows : -failed;
