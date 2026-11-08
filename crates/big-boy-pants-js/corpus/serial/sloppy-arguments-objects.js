// Arguments objects as ordinary objects (light-cljs-854), a sloppy script.
// Strict functions' unmapped objects take element and `length` writes,
// deletes, new named and symbol properties and for-in like any object, and
// their `callee` is the %ThrowTypeError% accessor; a sloppy function with a
// simple parameter list gets `callee` = itself, a writable data property,
// and one with a default or rest parameter the accessor. Objects kept by a
// closure across calls stay live at every cut and collection.
function failure(thunk) { try { thunk(); return "none"; } catch (e) { return e.name; } }
function keys(o) { var s = ""; for (var k in o) s += k + ","; return s; }
var kept = (function () { "use strict"; return (function () { var a = arguments; return function () { return a; }; })(1, 2, 3); })();
var mark = Symbol.split;
(function () {
  "use strict";
  var a = kept();
  a[0] = "w"; a.length = 2; delete a[1]; a.extra = 9; a[mark] = 1; a[6] = 6;
  return a[0] === "w" && a.length === 2 && !(1 in a) && a[2] === 3 && keys(a) === "0,2,6,extra," && a[mark] === 1 && kept() === a;
})() &&
(function () {
  "use strict";
  function f() { return arguments; }
  var a = f(1, 2);
  var d = Object.getOwnPropertyDescriptor(a, "callee");
  return Object.prototype.toString.call(a) === "[object Arguments]" && String(a) === "[object Arguments]" &&
    failure(function () { return a.callee; }) === "TypeError" && failure(function () { a.callee = 1; }) === "TypeError" &&
    d.get === d.set && !d.enumerable && !d.configurable &&
    delete a.length && a.length === undefined && delete a[Symbol.iterator] && failure(function () { return [...a]; }) === "TypeError";
})() &&
(function () {
  function f() { return arguments; }
  var a = f(4, 5), d = Object.getOwnPropertyDescriptor(a, "callee");
  var fact = function (n) { return n <= 1 ? 1 : n * arguments.callee(n - 1); };
  return a.callee === f && d.value === f && d.writable && d.configurable && !d.enumerable && keys(a) === "0,1," &&
    fact(6) === 720 && (a.callee = 3, a.callee === 3) && delete a.callee && !("callee" in a);
})() &&
(function () {
  function d(x = 1) { return arguments.callee; }
  function r(...xs) { return arguments.callee; }
  function s(x) { "use strict"; return arguments.callee; }
  return failure(d) === "TypeError" && failure(r) === "TypeError" && failure(s) === "TypeError";
})() &&
// Writes past the end and through an iterator, apply and join.
(function () {
  "use strict";
  function g(x, y, z) { return x + "" + y + z; }
  function f() { arguments[2] = "c"; arguments.length = 3; return g.apply(null, arguments) + [...arguments].length + Array.prototype.join.call(arguments, "|"); }
  return f("a", "b") === "abc3a|b|c";
})();
