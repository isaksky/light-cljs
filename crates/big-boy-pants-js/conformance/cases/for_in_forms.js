// bbp-authored conformance case.
// Derives from: none (light-cljs-804: for-in over array elements with a hole,
// a string's indices, null, undefined and a number (no iterations), with
// break, continue, a `let` head bound afresh each iteration and a member
// head). Completes with "02|01|none|q|last|p,q,".
(function () {
  var h = [5, 6, 7]; delete h[1];
  var holes = ""; for (var i in h) holes += i;
  var chars = ""; for (var j in "ab") chars += j;
  var none = "none";
  for (var n in null) none = "some";
  for (var u in undefined) none = "some";
  for (var f in 5) none = "some";
  var picked = "";
  for (var k in { p: 1, q: 2, r: 3 }) { if (k === "r") break; if (k === "p") continue; picked += k; }
  var target = {};
  for (target.last in { first: 1, last: 2 }) {}
  var fns = [];
  for (let key in { p: 1, q: 2 }) fns[fns.length] = function () { return key; };
  return holes + "|" + chars + "|" + none + "|" + picked + "|" + target.last + "|" + fns[0]() + "," + fns[1]() + ",";
})();
