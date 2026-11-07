// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 19053-19071,
// objects_compare_after_to_primitive (bbp-70; ECMAScript IsLessThan, which
// takes ToPrimitive with hint number of both operands, left first, and then
// compares two strings by code units and anything else by ToNumber): objects
// whose valueOf gives a string or a number compare as those primitives with
// `<`, `>`, `<=` and `>=`, and toString serves when valueOf gives an object.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
function boxed(v) { return { valueOf: function () { return v; } }; }
var pairs = [
  ["10", "2", [true, false, true, false]],
  ["2", "10", [false, true, false, true]],
  ["10", 2, [false, true, false, true]],
  [2, "10", [true, false, true, false]],
  ["2", "2", [false, false, true, true]],
  [undefined, 1, [false, false, false, false]],
  [1, undefined, [false, false, false, false]],
];
for (var i = 0; i < pairs.length; i++) {
  var a = boxed(pairs[i][0]), b = boxed(pairs[i][1]), want = pairs[i][2];
  row((a < b) === want[0] && (a > b) === want[1] && (a <= b) === want[2] && (a >= b) === want[3]);
}
// One side an object, the other a primitive.
row(boxed("b") > "a" && "a" < boxed("b") && boxed(3) >= 3 && 4 <= boxed("4"));
// toString when valueOf gives an object: two strings compare by code units.
{
  var s = function (t) { return { valueOf: function () { return {}; }, toString: function () { return t; } }; };
  row(s("10") < s("9") && !(s("10") < 9) && s("a") < s("b"));
}
// NaN from either side makes every comparison false.
row(!(boxed("x") < 1) && !(boxed("x") >= 1) && !(1 > boxed(NaN)));
// `a > b` still converts a first.
{
  var log = "";
  var l = { valueOf: function () { log += "l"; return 1; } }, r = { valueOf: function () { log += "r"; return 2; } };
  row(!(l > r) && log === "lr" && l <= r && log === "lrlr");
}
failed === 0 ? rows : -failed;
