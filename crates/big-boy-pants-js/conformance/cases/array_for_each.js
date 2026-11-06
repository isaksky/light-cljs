// bbp-authored conformance case.
// Derives from: rust/tests/fixtures/js/array_callbacks.js lines 9-11, 27-31, 35-39, 69-72
// (Array.prototype.forEach: thisArg and arguments, holes, mutation during iteration, a generic array-like with a length getter, an exception from the callback). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  var a = [1, 2, 3], sum = 0, ctx = { factor: 2 }, args = true;
  // thisArg, the (value, index, object) arguments; the result is undefined.
  var r = a.forEach(function (v, i, o) { args = args && this === ctx && o === a && v === i + 1; sum += v * this.factor; }, ctx);
  check(1, args && r === undefined && sum === 12);
  // Holes are skipped.
  var holes = Array(5); holes[1] = 2; holes[3] = 4; var seen = 0;
  holes.forEach(function (v, i) { seen = seen * 100 + i * 10 + v; });
  check(2, seen === 1234);
  // Mutation during iteration: deleted and cut-off indices are skipped, a
  // changed one read again, an appended one not visited.
  var changed = [1, 2, 3, 4], visited = 0;
  changed.forEach(function (v, i, o) { visited = visited * 10 + v; if (i === 0) { delete o[1]; o[2] = 8; o[4] = 9; } if (i === 2) { o.length = 3; } });
  check(3, visited === 18 && changed.length === 3);
  // A generic array-like with a `length` getter, read once, and a hole.
  var reads = 0, keys = "", receivers = true, generic = { 0: "a", 1: "b", 3: "d", get length() { reads++; return 4; } };
  Array.prototype.forEach.call(generic, function (v, i, o) { receivers = receivers && o === generic; keys += v; });
  check(4, keys === "abd" && reads === 1 && receivers);
  // HasProperty and Get see an inherited index.
  var inherited = "";
  Array.prototype.forEach.call({ __proto__: { 1: "p" }, 0: "o", length: 3 }, function (v) { inherited += v; });
  check(8, inherited === "op");
  // A getter on an element runs with the object as receiver.
  var got = "", withGetter = { length: 2, 0: "x", get 1() { return this[0] + "y"; } };
  Array.prototype.forEach.call(withGetter, function (v) { got += v; });
  check(5, got === "xxy");
  // An exception from the callback ends the iteration; a finally inside runs.
  var calls = 0, cleaned = 0, thrown;
  function fail() { calls++; try { if (calls === 2) { throw 42; } } finally { cleaned++; } }
  try { [1, 2, 3].forEach(fail); } catch (e) { thrown = e; }
  check(6, thrown === 42 && calls === 2 && cleaned === 2);
  // A non-callable callback is a TypeError, even for an empty array, and
  // so is an undefined receiver.
  var typeErrors = 0;
  try { [].forEach({}); } catch (e) { if (e.name === "TypeError") { typeErrors++; } }
  try { Array.prototype.forEach.call(undefined, function () {}); } catch (e) { if (e.name === "TypeError") { typeErrors++; } }
  check(7, typeErrors === 2 && Array.prototype.forEach.length === 1 && Array.prototype.forEach.name === "forEach");
  return failed === 0 ? "ok" : failed;
})();
