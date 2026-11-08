// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 2694-2760,
// indirect_eval_utf16_source (bbp-279), the rows without RegExp, the Function
// constructor, e.stack or direct eval (light-cljs-856), with lone surrogates
// written as `\u` escapes in string literals (the Pants rows build them with
// String.fromCharCode). Eval source is the string's exact UTF-16 code units:
// a lone surrogate inside a string or template literal or a comment survives
// unchanged, a surrogate pair is one code point (an identifier character
// too), and a lone surrogate where a token is expected is a SyntaxError.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
const hi = "\ud800", lo = "\udc00";
row((0, eval)('"' + hi + '"') === hi && (0, eval)('"' + lo + '"') === lo);
row((0, eval)('"\\ud800"') === hi);
const s = "𐀀\ud800�\udc00";
row((0, eval)('"' + s + '"') === s);
row((0, eval)("/*" + "𐀀\udc00\ud800" + "*/ 1 // " + "𐀀\udc00\ud800" + "\r\n + 2") === 3);
const t = "\ud800A\udc00";
row((0, eval)("`" + t + "`") === t && (0, eval)("`" + t + "${2}" + t + "`") === t + "2" + t);
row((0, eval)('"\\' + t + '"') === t);
// A pair split at the frontend's 256-unit preparation slices.
let spaces = "";
while (spaces.length < 254) spaces += " ";
row((0, eval)(spaces + '"😀"') === "😀" && (0, eval)(spaces + ' "\ud800�\udc00"') === "\ud800�\udc00");
row((0, eval)('"π😀"') === "π😀");
row((0, eval)("var 𐐀 = 7; 𐐀") === 7);
row(name(() => (0, eval)("var " + hi + " = 1")) === "SyntaxError");
row(name(() => (0, eval)("var a" + hi + " = 1")) === "SyntaxError");
row(name(() => (0, eval)(lo)) === "SyntaxError");
failed === 0 ? rows : -failed;
