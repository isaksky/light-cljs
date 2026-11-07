// bbp-authored conformance case, run as a sloppy script (areas.edn :mode).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18257-18268,
// methods_on_primitive_bases (bbp-73), its sloppy rows: sloppy code ignores
// a write to a primitive base, including a string's read-only `length` and
// indices, and `delete` of those is false (light-cljs-833). Each row is a
// `row(...)` check; the script completes with the number of rows, or minus
// the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function quiet(f) { try { f(); } catch (e) { return false; } return true; }
var n = 1, s = 'abc', b = true;
row(quiet(function () { n.x = 2; }) && n.x === undefined);
row(quiet(function () { s[0] = 'y'; s[5] = 1; s.length = 0; s.x = 1; }) && s === 'abc' && s.length === 3 && s[0] === 'a' && s.x === undefined);
row(quiet(function () { b.x = 1; true.y = 2; }) && b.x === undefined);
row(quiet(function () { n.x += 1; n.y++; n.toString = 1; }) && n.toString === Number.prototype.toString);
row((n.x = 5) === 5 && (s[0] = 'q') === 'q');
row(delete s.length === false && delete s[0] === false && delete s[5] === true && delete n.x === true);
failed === 0 ? rows : -failed;
