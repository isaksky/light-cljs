// bbp-authored conformance case.
// Derives from: rust/tests/fixtures/js/array_callbacks.js lines 9-12, 27-31, 35-39, 69-72
// (Array.prototype.map: thisArg and arguments, holes, mutation during iteration, a generic array-like whose length getter returns an object, an exception from the callback). Completes with "ok", or the number of the first failed check.
(function () {
  var failed = 0;
  function check(n, ok) { if (!ok && failed === 0) { failed = n; } }
  var a = [1, 2, 3], ctx = { factor: 2 }, args = true;
  // thisArg, the (value, index, object) arguments and a new array.
  var m = a.map(function (v, i, o) { args = args && this === ctx && o === a && v === i + 1 && arguments.length === 3; return v * this.factor; }, ctx);
  check(1, args && m !== a && Array.isArray(m) && m.length === 3 && m[0] === 2 && m[1] === 4 && m[2] === 6);
  // Holes are skipped and stay holes in the result, which keeps the length.
  var holes = Array(4); holes[1] = 2; var seen = "";
  var mapped = holes.map(function (v, i) { seen += i === 1 ? "1" : "?"; return v * 2; });
  check(2, seen === "1" && mapped.length === 4 && !(0 in mapped) && mapped[1] === 4 && !(2 in mapped) && !(3 in mapped));
  var sparse = Array(4), visits = 0; sparse[1] = 5; sparse[3] = 6;
  var s = sparse.map(function (v) { visits++; return v + 1; });
  check(3, visits === 2 && s.length === 4 && !(0 in s) && s[1] === 6 && !(2 in s) && s[3] === 7);
  // Mutation during iteration: a deleted index is skipped, a changed one
  // read again, an appended one not visited (the length was read first).
  var changed = [1, 2, 3], visited = 0;
  var changedResult = changed.map(function (v, i, o) { visited = visited * 10 + v; if (i === 0) { delete o[1]; o[2] = 8; o[3] = 9; } return v; });
  check(4, visited === 18 && changedResult.length === 3 && changedResult[0] === 1 && !(1 in changedResult) && changedResult[2] === 8 && changed.length === 4);
  // A generic array-like: one `length` read, through its getter, whose
  // object value converts by valueOf; ToLength truncates 3.9 to 3.
  var reads = 0, receivers = true, generic = { 0: 3, 2: 5, 3: 100, get length() { reads++; return { valueOf: function () { return 3.9; } }; } };
  var g = Array.prototype.map.call(generic, function (v, i, o) { receivers = receivers && o === generic; return v + i; });
  check(5, reads === 1 && receivers && Array.isArray(g) && g.length === 3 && g[0] === 3 && !(1 in g) && g[2] === 7);
  // A length getter that throws, and a negative length.
  var lengthError;
  try { Array.prototype.map.call({ get length() { throw 7; } }, function () {}); } catch (e) { lengthError = e; }
  check(6, lengthError === 7 && Array.prototype.map.call({ length: -2, 0: 1 }, function () { return 1; }).length === 0);
  // An exception from the callback ends the map; a finally inside runs.
  var calls = 0, cleaned = 0, thrown;
  function fail() { calls++; try { if (calls === 2) { throw 42; } } finally { cleaned++; } }
  try { [1, 2, 3].map(fail); } catch (e) { thrown = e; }
  check(7, thrown === 42 && calls === 2 && cleaned === 2);
  // A non-callable callback is a TypeError.
  var typeError = false;
  try { [1].map(5); } catch (e) { typeError = e.name === "TypeError"; }
  check(8, typeError && Array.prototype.map.length === 1 && Array.prototype.map.name === "map");
  // Nested maps, and a callback that is a built-in.
  var nested = [[1, 2], [3]].map(function (row) { return row.map(function (x) { return x * 10; }); });
  check(9, nested[0][1] === 20 && nested[1][0] === 30 && [[1], 2].map(Array.isArray)[0] === true);
  return failed === 0 ? "ok" : failed;
})();
