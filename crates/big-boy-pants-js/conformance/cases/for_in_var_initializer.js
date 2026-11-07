// bbp-authored conformance case (sloppy script: areas.edn :mode :sloppy).
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 16096-16110
// (`sloppy_for_in_var_initializer`, bbp-90): its rows, each run in
// its own function and its result appended to one log. `for (var k = 2 in o)`
// (Annex B) assigns 2 first, then each key; the initializer runs before the
// object expression; with no keys, `null` or `undefined` the binding keeps
// 2; an initializer that throws leaves it undefined; an anonymous function
// initializer is named after the binding; closures share the one binding.
// `tab.push` is replaced by `+=` and `fs.push(f)` by `fs[fs.length] = f`.
// The script then ends with the completion row, `5; for (var k = 2 in {}) {}`:
// a for-in that runs no iteration completes with undefined, not the 5 before
// it. Only the script's own completion can observe that (eval is not on
// Goose), so the rows above are checked first against their agreed log and a
// mismatch throws it. Completes with undefined; an engine that kept the 5
// completes with 5, and one that got a row wrong throws an Error.
var log = "";
function row(f) { var v; try { v = f(); } catch (e) { v = "thrown:" + e; } log += (log === "" ? "" : ",") + v; }
row(function () { var tab = ""; for (var k = 2 in { x: 1, y: 2 }) tab += k; return tab === "xy" && k === "y"; });
row(function () { for (var k = 2 in {}) {} return k === 2; });
row(function () { for (var k = 2 in null) {} return k === 2; });
row(function () { for (var k = 2 in undefined) {} return k === 2; });
row(function () {
  var log = "";
  function init() { log += "i"; return 2; }
  function obj() { log += "o"; return k === 2 ? { x: 1, y: 2 } : {}; }
  for (var k = init() in obj()) log += k;
  return log === "ioxy";
});
row(function () {
  function f() { var before = k; for (var k = 2 in {}) {} return before === undefined && k === 2; }
  return f();
});
row(function () {
  var n = 0;
  function init() { n++; throw 7; }
  function obj() { n += 100; return {}; }
  try { for (var k = init() in obj()) {} } catch (e) {}
  return n === 1 && k === undefined;
});
row(function () { var f; for (var k = function () {} in {}) f = k; return k.name === "k"; });
row(function () {
  var fs = [];
  for (var k = 2 in { x: 1, y: 2 }) fs[fs.length] = () => k;
  return fs[0]() === "y" && fs[1]() === "y";
});
for (var last = 3 in { x: 1 }) {}
if (log + "|" + last !== "true,true,true,true,true,true,true,true,true|x") throw new Error(log + "|" + last);
5; for (var k = 2 in {}) {}
