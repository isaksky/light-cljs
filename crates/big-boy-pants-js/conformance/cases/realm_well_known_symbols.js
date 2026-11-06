// bbp-authored conformance case.
// Derives from: none (light-cljs-788: the realm's well-known symbols as values: typeof, identity, their
// attributes as properties of Symbol, use as property keys, and a symbol-keyed @@toStringTag that
// Object.prototype.toString reads). Completes with "yyyyy".
(function () {
  var s = "";
  function check(c) { s += c ? "y" : "n"; }
  check(typeof Symbol === "function" && Symbol.name === "Symbol" && Symbol.length === 0);
  check(typeof Symbol.iterator === "symbol" && Symbol.iterator === Symbol.iterator && Symbol.iterator !== Symbol.asyncIterator);
  var before = Symbol.iterator;
  try { Symbol.iterator = 1; } catch (e) {}
  check(Symbol.iterator === before && Symbol.toPrimitive !== Symbol.toStringTag && typeof Symbol.hasInstance === "symbol");
  var o = {};
  o[Symbol.toStringTag] = "Tagged";
  check(o[Symbol.toStringTag] === "Tagged" && Object.prototype.toString.call(o) === "[object Tagged]");
  var k = { [Symbol.species]: 7 };
  check(k[Symbol.species] === 7 && !(Symbol.species in {}));
  return s;
})();
