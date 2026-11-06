// bbp-authored conformance case.
// Derives from: none (light-cljs-805: a function's own properties, set, read, deleted and found by `in`; an ordinary function's `prototype`,
// made on first read, is the same object later, has the function as `constructor`, is not deletable and can be replaced; arrows and methods have none).
// Completes with "2|proto|refused|7|none".
(function () {
  function counter() { counter.count += 1; return counter.count; }
  counter.count = 0;
  counter(); counter();
  var count = counter.count === 2 && ("count" in counter) && (delete counter.count) && counter.count === undefined ? "2" : "wrong";
  function F() {}
  var p = F.prototype;
  var proto = p === F.prototype && p.constructor === F && ("prototype" in F) ? "proto" : "wrong";
  var refused = "kept";
  try { delete F.prototype; } catch (e) { refused = "refused"; }
  F.prototype = { mark: "7" };
  var arrow = () => 1;
  var method = { m() { return 2; } }.m;
  arrow.x = 3;
  var none = arrow.prototype === undefined && method.prototype === undefined && arrow.x === 3 ? "none" : "wrong";
  return count + "|" + proto + "|" + refused + "|" + F.prototype.mark + "|" + none;
})();
