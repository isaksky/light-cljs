// `with` statements (light-cljs-855), a sloppy script. Names resolve through
// the object (HasProperty, then @@unscopables, then [[Get]]/[[Set]]), so
// getters, setters and an @@unscopables getter run above the reads as
// native frames; closures made in the body keep the object environment; a
// call through the object passes it as `this`, a tagged template's too
// (light-cljs-862); Array.prototype's
// @@unscopables hides `values`. serial_tests.goose runs it at every budget
// cut and with forced collections at every cut.
var values = "outer", log = "";
(function () {
  var o = { x: 1 };
  with (o) { x = 2; var y = x + 1; }
  return o.x === 2 && o.y === undefined && y === 3;
})() &&
(function () {
  var n = 0, o = { get x() { n++; return 10; }, set x(v) { log += v; } };
  with (o) { x += 5; }
  return n === 1 && log === "15";
})() &&
(function () {
  var x = "outer", seen = "";
  var o = { x: "in", get [Symbol.unscopables]() { seen += "u"; return { x: seen.length > 1 }; } };
  with (o) { var first = x; var second = x; }
  return first === "in" && second === "outer" && seen === "uu";
})() &&
(function () {
  var fs = [], o = { i: 0, f() { return this === o; } };
  with (o) { while (i < 3) { fs.push(() => i); i++; } var called = f(); }
  o.i = 7;
  return called && fs.length === 3 && fs[0]() === 7 && fs[2]() === 7;
})() &&
(function () {
  var g;
  with ({ x: 2 }) { g = function () { "use strict"; return x; }; }
  return g() === 2;
})() &&
(function () {
  with ([1, 2]) { return values === "outer" && length === 2 && typeof push === "function"; }
})() &&
(function () {
  var o = { x: 2, f(s) { return this.x + s.length; } };
  with (o) { return f`` === 3 && f`a${1}` === 4; }
})();
