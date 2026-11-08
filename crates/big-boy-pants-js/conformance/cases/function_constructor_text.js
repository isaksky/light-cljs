// bbp-authored conformance case.
// Derives from: ~/src/quickjs/quickjs.c lines 41907-41975
// (js_function_constructor, revision 7d1c13c) and the outputs of its qjs
// for each row (light-cljs-857): Function.prototype.toString of a function
// the Function constructor made is the source it synthesized,
// "function anonymous(" + the parameters joined by "," + "\n) {\n" + the
// body + "\n}". Each row is a `row(...)` check; the script completes with
// the number of rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
row(new Function("a", "b", "return a").toString() === "function anonymous(a,b\n) {\nreturn a\n}");
row(Function().toString() === "function anonymous(\n) {\n\n}");
row(Function("return 1").toString() === "function anonymous(\n) {\nreturn 1\n}");
row(Function("a, b", "c = 1", "").toString() === "function anonymous(a, b,c = 1\n) {\n\n}");
// The source keeps its exact UTF-16 code units: a lone surrogate in a
// comment or a string literal beside a genuine U+FFFD (light-cljs-864).
row(Function("a /*\udc00*/", "/*\ud800�*/ return '\udc00�' + a").toString() === "function anonymous(a /*\udc00*/\n) {\n/*\ud800�*/ return '\udc00�' + a\n}");
failed === 0 ? rows : -failed;
