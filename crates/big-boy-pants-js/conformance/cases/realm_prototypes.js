// bbp-authored conformance case.
// Derives from: none (light-cljs-788: the realm's intrinsics and the prototypes literals, functions and
// constructors get: Object.getPrototypeOf over each, constructor links, the global names, and `in` reaching
// a standard method the realm defines). Completes with "yyyyyyyyyyyyyyyy".
(function () {
  var s = "";
  function check(c) { s += c ? "y" : "n"; }
  check(Object.getPrototypeOf([]) === Array.prototype);
  check(Object.getPrototypeOf({}) === Object.prototype);
  check(Object.getPrototypeOf(function () {}) === Function.prototype);
  check(Object.getPrototypeOf(() => 1) === Function.prototype);
  check(Object.getPrototypeOf(Array.prototype) === Object.prototype);
  check(Object.getPrototypeOf(Object.prototype) === null);
  check(Object.getPrototypeOf(Function.prototype) === Object.prototype);
  check(Object.getPrototypeOf(Object) === Function.prototype && Object.getPrototypeOf(Object.getPrototypeOf) === Function.prototype);
  check(Array.prototype.constructor === Array && Object.prototype.constructor === Object && Function.prototype.constructor === Function);
  check(Array.prototype.length === 0 && Array.isArray(Array.prototype) && !Array.isArray({}));
  check(globalThis.Array === Array && globalThis.globalThis === globalThis);
  check(Object.getPrototypeOf({ __proto__: null }) === null);
  check("map" in [] && "toString" in {} && !("map" in {}));
  check("length" in [] && "call" in function () {} && "getPrototypeOf" in Object && !("getPrototypeOf" in {}));
  check(Object.is(NaN, NaN) && !Object.is(0, -0) && Object.is(Object.prototype, Object.getPrototypeOf({})));
  check(typeof NaN === "number" && NaN !== NaN && Infinity > 1e308 && undefined === void 0);
  return s;
})();
