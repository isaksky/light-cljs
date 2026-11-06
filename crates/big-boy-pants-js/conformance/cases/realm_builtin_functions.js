// bbp-authored conformance case.
// Derives from: none (light-cljs-788: built-in function objects as values: typeof, their `name` and `length`
// with the specification's attributes, calls through `.call` with a receiver, Function.prototype as a
// callable, and typeof of every primitive). Completes with "yyyyyyyyyyyyy".
(function () {
  var s = "";
  function check(c) { s += c ? "y" : "n"; }
  check(typeof Object.prototype.toString === "function" && typeof Object === "function" && typeof Array.isArray === "function");
  check(Object.getPrototypeOf.name === "getPrototypeOf" && Object.getPrototypeOf.length === 1);
  check(Object.prototype.toString.name === "toString" && Object.prototype.toString.length === 0 && Object.prototype.valueOf.name === "valueOf");
  check(Function.prototype.call.name === "call" && Function.prototype.call.length === 1 && Function.prototype.name === "" && Function.prototype.length === 0);
  check(Array.name === "Array" && Array.length === 1 && TypeError.name === "TypeError" && TypeError.length === 1 && Object.is.length === 2);
  check(!("prototype" in Object.getPrototypeOf) && "prototype" in Object && Object.prototype.toString.call([]) === "[object Array]" && Object.prototype.toString.call(null) === "[object Null]");
  check(Function.prototype() === undefined && Object.prototype.toString.call({}) === "[object Object]" && "" + {} === "[object Object]");
  check(typeof undefined === "undefined" && typeof null === "object" && typeof 1 === "number" && typeof "s" === "string" && typeof true === "boolean");
  check(typeof {} === "object" && typeof [] === "object" && typeof function () {} === "function" && typeof notDeclaredAnywhere === "undefined");
  var names = 0;
  for (var k in Object.getPrototypeOf) { names++; }
  for (var k2 in Array.prototype) { names++; }
  check(names === 0);
  var o = Object();
  check(Object.getPrototypeOf(o) === Object.prototype && Object(o) === o && Object(null) !== o);
  var a = Array(1, 2, 3); var b = Array(4);
  check(a.length === 3 && a[2] === 3 && b.length === 4 && !(0 in b) && Array("x")[0] === "x");
  var len = Object.getPrototypeOf.length;
  try { Object.getPrototypeOf.length = 5; } catch (e) {}
  check(Object.getPrototypeOf.length === len);
  return s;
})();
