// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 18642-18646,
// the `a - b` order row of objects_convert_with_to_primitive (bbp-70), and
// rust/src/vm/native.rs NativeState::Convert (ECMAScript ToPrimitive and OrdinaryToPrimitive; ApplyStringOrNumericBinaryOperator
// and IsLessThan with LeftFirst): two object operands convert left to
// right, the left one completely (each method looked up and called at most
// once) before the right one starts; methods supplied by getters run their
// getter, then the method, in that order; hint string asks toString first.
// Each row is a `row(...)` check; the script completes with the number of
// rows, or minus the first failing row's number.
var rows = 0, failed = 0;
function row(ok) { rows++; if (ok !== true && failed === 0) failed = rows; }
var log = "";
function logged(tag, value) {
  return {
    get valueOf() { log += tag + "g"; return function () { log += tag + "v"; return value; }; },
    get toString() { log += tag + "G"; return function () { log += tag + "t"; return "s" + tag; }; },
  };
}
function run(f) { log = ""; f(); return log; }
row(run(function () { return logged("a", 1) - logged("b", 2); }) === "agavbgbv");
row(run(function () { return logged("a", 1) + logged("b", 2); }) === "agavbgbv");
row(run(function () { return logged("a", 1) < logged("b", 2); }) === "agavbgbv");
row(run(function () { return logged("a", 1) > logged("b", 2); }) === "agavbgbv");
row(run(function () { return logged("a", 1) * logged("b", 2); }) === "agavbgbv");
row(run(function () { return logged("a", 1) == 1; }) === "agav");
row(run(function () { return `${logged("a", 1)}${logged("b", 2)}`; }) === "aGatbGbt");
row(run(function () { var o = {}; o[logged("a", 1)] = 1; }) === "aGat");
// valueOf giving an object falls back to toString, still before the right
// operand starts.
{
  var both = {
    get valueOf() { log += "1"; return function () { log += "2"; return {}; }; },
    get toString() { log += "3"; return function () { log += "4"; return "x"; }; },
  };
  row(run(function () { return both + logged("b", 2); }) === "1234bgbv");
}
// A non-callable method found through a getter is skipped after one read.
{
  var skip = {
    get valueOf() { log += "v"; return 5; },
    get toString() { log += "t"; return function () { log += "c"; return "ok"; }; },
  };
  row(run(function () { return skip + ""; }) === "vtc");
}
// Symbol.toPrimitive is read before valueOf and toString, and stops them.
{
  var hooked = { valueOf: function () { log += "v"; return 1; } };
  hooked[Symbol.toPrimitive] = function (h) { log += "p" + h[0]; return 2; };
  row(run(function () { return hooked - logged("b", 3); }) === "pnbgbv");
}
// An exception in the left operand's conversion stops before the right.
{
  var throws = { valueOf: function () { log += "x"; throw "stop"; } };
  row(run(function () { try { return throws - logged("b", 2); } catch (e) { log += e; } }) === "xstop");
}
// ++ converts once and stores the number.
{ var counter = { valueOf: function () { log += "c"; return 1; } }; row(run(function () { counter++; counter++; }) === "c" && counter === 3); }
failed === 0 ? rows : -failed;
