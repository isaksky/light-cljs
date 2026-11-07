// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 12463-12541,
// strings_compare_by_code_units (bbp-68): the rows with two string operands
// (a string against a number waits for the StringToNumber pellet); plus
// concatenation of strings (bbp-65). Each row is a `row(...)` check; the
// script completes with the number of rows, or minus the first failing
// row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row(("b" > "a") === true);
row(("a" < "b") === true);
// A proper prefix is smaller; the empty string is the smallest.
row(("a" < "aa") === true);
row(("aa" > "a") === true);
row(("" < "a") === true);
row(("" < "") === false);
row(("" <= "") === true);
// Code units, not case-insensitive: "B" is 0x42, "a" is 0x61.
row(("B" < "a") === true);
row(("a" <= "a") === true);
row(("a" >= "a") === true);
row(("a" > "a") === false);
row(("a" >= "b") === false);
row(("abd" > "abc") === true);
// Two strings compare as strings, not as numbers.
row(("10" < "9") === true);
row(("100" > "99") === false);
// Code-unit order: U+10000 is D800 DC00, below U+FFFF.
row(("\u{10000}" < "￿") === true);
row(("￿" < "\u{10000}") === false);
row(("\u{10000}" > "") === false);
row(("\u{10000}" > "퟿") === true);
row(("\u{10000}" < "\u{10001}") === true);
row(("\uD800" < "𐀀") === true);
// Latin-1 against UTF-16 text.
row(("\xff" < "Ā") === true);
row(("Ā" > "\xff\xff") === true);
row(("é" < "ê") === true);
row(("z" < "é") === true);
row(("aĀ" < "ab") === false);
row(("ab" < "aĀ") === true);
// Operands that are not known statically compare at run time, in all four
// operators.
{
  let f = (a, b) => (a < b) + 2 * (a > b) + 4 * (a <= b) + 8 * (a >= b);
  row(f("x", "y") === 5 && f("y", "x") === 10 && f("x", "x") === 12);
  row(f("", "a") === 5 && f("\u{10000}", "￿") === 5 && f("Ā", "Ā") === 12);
}
row("b" > "a" && "a" < "aa" && "" < "a" && "B" < "a" && "a" <= "a");
// Concatenation of strings.
row("a" + "b" === "ab");
row("" + "" === "");
row("" + "x" === "x" && "x" + "" === "x");
row("ab" + "c" < "abd");
row(("a" + "b") + "c" === "a" + ("b" + "c"));
row("é" + "Ā" === "éĀ");
row("\uD83D" + "\uDE00" === "\u{1F600}");
{ let s = "x"; s = s + s; s = s + s; row(s === "xxxx"); }
{ let s = ""; s += "a"; s += "Ā"; s += "b"; row(s === "aĀb"); }
row(("a" + "b").length === 2 && ("é" + "\u{1F600}").length === 3);
failed === 0 ? rows : -failed;
