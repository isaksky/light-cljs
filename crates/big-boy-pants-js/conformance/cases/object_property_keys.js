// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18743-18749,
// the property-key rows of symbol_to_primitive_general_dispatch (bbp-310),
// and rust/src/vm/property.rs to_property_key and element_key (ECMAScript ToPropertyKey: ToPrimitive with hint string, so toString
// before valueOf, then ToString unless the result is a symbol): an object
// used as a key converts in reads, writes, `in`, `delete` and computed
// literal keys, once each, after the base is checked; an array index key
// reaches the element. Compound assignment on such a key stays with the
// property-reference pellet. Each row is a `row(...)` check; the script
// completes with the number of rows, or minus the first failing row's
// number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function name(thunk) { try { thunk(); return "none"; } catch (e) { return typeof e === "object" && e !== null ? e.name : typeof e; } }
var calls = 0;
var key = { toString: function () { calls++; return "k"; }, valueOf: function () { throw "valueOf"; } };
var o = {};
o[key] = 1;
row(o.k === 1 && calls === 1);
row(o[key] === 1 && calls === 2);
row(key in o && calls === 3);
row(delete o[key] && calls === 4 && !("k" in o));
{ var lit = { [key]: 2 }; row(lit.k === 2 && calls === 5); }
// valueOf serves when toString is missing or gives an object.
{ var v = { toString: function () { return {}; }, valueOf: function () { return 3; } }; var t = {}; t[v] = "three"; row(t[3] === "three" && t["3"] === "three"); }
{ var w = { toString: null, valueOf: function () { return "w"; } }; var t2 = { w: 9 }; row(t2[w] === 9); }
// An index key reaches an array's element.
{ var i = { toString: function () { return "1"; } }; var arr = [10, 20, 30]; row(arr[i] === 20); arr[i] = 21; row(arr[1] === 21 && i in arr); }
// A string base and an object key.
{ var j = { toString: function () { return "length"; } }; row("abc"[j] === 3); }
// The default toString: "[object Object]".
{ var plain = {}; var t3 = {}; t3[plain] = 4; row(t3["[object Object]"] === 4); }
// Errors propagate; a key neither method converts is a TypeError.
{ var thrower = { toString: function () { throw "boom"; } }; var got; try { ({})[thrower]; } catch (e) { got = e; } row(got === "boom"); }
row(name(function () { return ({})[{ toString: function () { return {}; }, valueOf: function () { return {}; } }]; }) === "TypeError");
// `in` checks its right side before it converts the key.
{ var seen = 0; var k2 = { toString: function () { seen++; return "x"; } }; row(name(function () { return k2 in 1; }) === "TypeError" && seen === 0); }
failed === 0 ? rows : -failed;
