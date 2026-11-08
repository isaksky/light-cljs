// bbp-authored conformance case (sloppy script).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 6200-6205,
// tagged_templates_descriptor_operations (its sloppy delete row), and
// light-cljs-862. In sloppy code a refused write to a frozen template
// object does nothing and a refused delete answers false; a delete of a key
// the object lacks answers true. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function tag(s) { return s; }
row((function () { var s = tag`a`; s[0] = "z"; s[4] = "z"; s.length = 0; s.extra = 1; return s[0] === "a" && s.length === 1 && !(4 in s) && s.extra === undefined; })());
row((function () { var s = tag`a`; s.raw = 1; s.raw[0] = "z"; s.raw.length = 7; return Array.isArray(s.raw) && s.raw[0] === "a" && s.raw.length === 1; })());
row((function () { var s = tag`a`; return delete s[0] === false && delete s.raw === false && delete s.length === false && delete s.absent === true && delete s[2] === true && s.length === 1 && s[0] === "a"; })());
row((function () { var s = tag`a`.raw; return delete s[0] === false && delete s.length === false && delete s.absent === true; })());
row((function () { var s = tag`a${1}b`; s[1] += "!"; s[0]++; return s[0] === "a" && s[1] === "b"; })());
// Assignment through `with`: the binding is the template object's own
// property, and the refused write does nothing.
row((function () { var s = tag`a${1}b`; with (s) { length = 0; raw = null; } return s.length === 2 && Array.isArray(s.raw); })());
failed === 0 ? rows : -failed;
