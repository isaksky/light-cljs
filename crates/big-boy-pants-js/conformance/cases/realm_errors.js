// bbp-authored conformance case.
// Derives from: none (light-cljs-788: error objects. A VM fault caught as an error object of its type's
// prototype with a string `message`; the native error constructors called as functions, their prototype
// chain, `message`, `cause`, Error.prototype.toString and ToPrimitive through it; a RangeError from
// Array). Completes with "yyyyyyyyyy".
(function () {
  var s = "";
  function check(c) { s += c ? "y" : "n"; }
  var t = null;
  try { var o = null; o.x; } catch (e) { t = e; }
  check(Object.getPrototypeOf(t) === TypeError.prototype && t.name === "TypeError" && typeof t.message === "string" && t.message.length > 0);
  var r = null;
  try { undefinedNameForRealm; } catch (e) { r = e; }
  check(Object.getPrototypeOf(r) === ReferenceError.prototype && r.name === "ReferenceError" && typeof r.message === "string");
  var c = null;
  try { var notCallable = 1; notCallable(); } catch (e) { c = e; }
  check(Object.getPrototypeOf(c) === TypeError.prototype);
  var e = TypeError("boom");
  check(e.message === "boom" && Object.getPrototypeOf(e) === TypeError.prototype && e.toString() === "TypeError: boom");
  check(Object.getPrototypeOf(TypeError) === Error && Object.getPrototypeOf(TypeError.prototype) === Error.prototype && Object.getPrototypeOf(Error.prototype) === Object.prototype);
  check(Error.prototype.message === "" && Error.prototype.name === "Error" && RangeError.prototype.name === "RangeError" && Error().message === "");
  check(Error("x", { cause: 7 }).cause === 7 && !("cause" in Error("x")) && Object.prototype.toString.call(Error("x")) === "[object Error]");
  check("" + RangeError("m") === "RangeError: m" && SyntaxError().toString() === "SyntaxError" && URIError.prototype.constructor === URIError && EvalError.name === "EvalError");
  var thrown = null;
  try { throw Error("own"); } catch (x) { thrown = x; }
  check(thrown.message === "own" && Object.getPrototypeOf(thrown) === Error.prototype);
  var range = null;
  try { Array(-1); } catch (x) { range = x; }
  check(Object.getPrototypeOf(range) === RangeError.prototype && range.name === "RangeError");
  return s;
})();
