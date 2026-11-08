// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 5968-5995
// and 6169-6211, tagged_templates and tagged_templates_descriptor_operations
// (the cooked/raw, descriptor and refused-write rows; Object.isFrozen,
// Object.freeze, Object.defineProperty and String.raw are pending on
// Goose), and light-cljs-862. A template object is an array of the cooked
// strings (undefined for an invalid escape) whose `raw` is an array of the
// raw strings; both are frozen: elements enumerable but neither writable nor
// configurable, `length` and `raw` neither writable, enumerable nor
// configurable, no new properties. In this strict script a refused write
// or delete throws TypeError. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function tag(s) { return s; }
function throws(thunk) { try { thunk(); return false; } catch (e) { return e instanceof TypeError; } }
// Cooked and raw strings.
row((function () { var s = tag`a${1}b\x41`; return s.length === 2 && s[0] === "a" && s[1] === "bA" && s.raw.length === 2 && s.raw[0] === "a" && s.raw[1] === "b\\x41"; })());
row((function () { var s = tag`\unicode`; return s[0] === undefined && 0 in s && s.raw[0] === "\\unicode" && s.raw[0].length === 8; })());
row((function () { var s = tag`a${1}`; return s.length === 2 && s[1] === "" && s.raw[1] === ""; })());
row((function () { var s = tag``; return s.length === 1 && s[0] === "" && s.raw[0] === ""; })());
row((function () { var s = tag`line\
next\n${0}\u{1F600}`; return s[0] === "linenext\n" && s.raw[0] === "line\\\nnext\\n" && s[1] === "\u{1F600}" && s.raw[1] === "\\u{1F600}"; })());
row((function () { var s = tag`ā${2}Ă`; return s[0] === "ā" && s.raw[1] === "Ă"; })());
// Arrays of %Array.prototype%, read like any array.
row((function () { var s = tag`a${1}b`; return Array.isArray(s) && Array.isArray(s.raw) && Object.getPrototypeOf(s) === Array.prototype && Object.getPrototypeOf(s.raw) === Array.prototype; })());
row((function () { var s = tag`a${1}b`; return s.join("-") === "a-b" && String(s) === "a,b" && s.map(function (x) { return x + "!"; }).join() === "a!,b!"; })());
row((function () { var s = tag`a${1}b`, n = 0, v = []; for (var k in s) n++; for (var x of s) v.push(x); return n === 2 && v.join() === "a,b"; })());
row((function () { var s = tag`a${1}b`, keys = ""; for (var k in s.raw) keys += k; return keys === "01" && !("raw" in {...s}) && ({...s})[1] === "b"; })());
// Descriptors.
row((function () { var d = Object.getOwnPropertyDescriptor(tag`a${0}b`, 0); return d.value === "a" && !d.writable && d.enumerable && !d.configurable; })());
row((function () { var d = Object.getOwnPropertyDescriptor(tag`a${0}b`, "length"); return d.value === 2 && !d.writable && !d.enumerable && !d.configurable; })());
row((function () { var s = tag`a`, d = Object.getOwnPropertyDescriptor(s, "raw"); return d.value === s.raw && !d.writable && !d.enumerable && !d.configurable; })());
row((function () { var d = Object.getOwnPropertyDescriptor(tag`a${0}b`.raw, 1); return d.value === "b" && !d.writable && d.enumerable && !d.configurable; })());
row((function () { var d = Object.getOwnPropertyDescriptor(tag`a`.raw, "length"); return d.value === 1 && !d.writable && !d.enumerable && !d.configurable; })());
// Refused writes, definitions and deletes throw TypeError and change nothing.
row((function () { var s = tag`a`; return throws(function () { s[0] = "z"; }) && throws(function () { s[5] = "z"; }) && s[0] === "a" && s.length === 1 && !(5 in s); })());
row((function () { var s = tag`a`; return throws(function () { s.length = 0; }) && throws(function () { s.length = 3; }) && s.length === 1; })());
row((function () { var s = tag`a`; return throws(function () { s.extra = 1; }) && !("extra" in s) && throws(function () { s.raw = []; }) && Array.isArray(s.raw); })());
row((function () { var s = tag`a`; return throws(function () { s.raw[0] = "z"; }) && throws(function () { s.raw.extra = 1; }) && s.raw[0] === "a" && !("extra" in s.raw); })());
row((function () { var s = tag`a`; return throws(function () { delete s[0]; }) && throws(function () { delete s.raw; }) && throws(function () { delete s.length; }) && delete s.absent && delete s[3]; })());
row((function () { var s = tag`a`; return throws(function () { s.push("b"); }) && s.length === 1 && throws(function () { s.raw.push("b"); }) && s.raw.length === 1; })());
row((function () { var s = tag`a`; return throws(function () { s[Symbol.iterator] = null; }) && typeof s[Symbol.iterator] === "function"; })());
// A site's object keeps its contents across runs.
row((function () { function site() { return tag`k${0}v`; } var a = site(); try { a[0] = "z"; } catch (e) {} return site()[0] === "k" && site() === a; })());
failed === 0 ? rows : -failed;
