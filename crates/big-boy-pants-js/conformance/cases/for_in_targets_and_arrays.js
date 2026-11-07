// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 10175-10222
// (`for_in_assigns_member_targets`, bbp-87) and 10466-10566
// (`for_in_over_arrays`, bbp-88, with bbp-264's length-assignment rows): their
// script rows, each run in its own function and its result appended to one
// log. Member and computed heads evaluated each iteration (object, key and its
// string conversion, then the setter, in that order), parenthesized heads, a
// head whose base changes in the body, a null base's TypeError, jumps through
// finally; array keys as strings, holes skipped, indices before names in
// creation order, index-like names (`01`, 4294967295, `-1`, `1.5`) among the
// names, an array shrunk by `a.length = 2` or to 0 during the loop, elements
// added during the loop not visited, an element deleted and put back before
// it is reached visited. `push` and `toString` are replaced by
// `a[a.length] = v` and `+=`, and the elision holes (`[1, , 3]`, which Goose
// does not build yet: light-cljs-844) by `delete`.
// Not ported:
// - the `for (using[0] in ...)` rows (three spellings): QuickJS rejects them
//   with "invalid destructuring target", though `using` followed by `[` is a
//   member expression there; the Pants engine and Goose agree;
// - every row that writes index 4294967294 (`b[4294967294] = 1`,
//   `a[4294967294] = 1`): Goose's dense element runs refuse an index that
//   large and end MemoryLimit (array exotic objects: light-cljs-766);
// - the rows that need Object.defineProperty (pending on Goose,
//   light-cljs-766) or Array.prototype.pop (pending, core built-ins planner
//   light-cljs-767), and the TypeError positions and instruction-budget rows
//   (Pants embedding API details).
// Completes with
// "true,true,true,true,true,true,true,true,TypeError,TypeError,true,true,string:true,string:true,,0,2,,1,4,0,3,1000,,0,1,2,extra,b,,0,01,4294967295,-1,1.5,,0,1,0,1,0,2,3,,3,extra,0,0=x;1=y;z=w;01z".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  // for_in_assigns_member_targets
  row(function () { let a = { x: 0 }, tab = ""; for (a.x in { x: 1, y: 2 }) tab += a.x; return tab === "xy" && a.x === "y"; });
  row(function () { let a = [0], tab = ""; for (a[0] in { x: 1, y: 2 }) tab += a[0]; return tab === "xy" && a[0] === "y"; });
  row(function () {
    let a = {}, calls = 0, tab = "";
    function obj() { calls++; return a; }
    for (obj().k in { a: 1, b: 1, c: 1 }) tab += a.k;
    return calls === 3 && tab === "abc" && a.k === "c";
  });
  row(function () {
    let log = "", a = { set k(v) { log += v; } };
    function obj() { log += "o"; return a; }
    function key() { log += "k"; return { toString() { log += "c"; return "k"; } }; }
    for (obj()[key()] in { a: 1, b: 1 }) log += "!";
    return log === "okca!okcb!";
  });
  row(function () { let n = 0; function obj() { n++; return {}; } for (obj().k in {}) n++; return n === 0; });
  row(function () { let a = {}; for (((a.x)) in { x: 1, y: 1 }) ; for ((a[0]) in { x: 1, y: 1 }) ; return a.x === "y" && a[0] === "y"; });
  row(function () { let a = {}, b = {}, target = a; for (target.x in { x: 1, y: 1 }) target = b; return a.x === "x" && b.x === "y"; });
  row(function () {
    let a = {}, log = "";
    for (a.x in { a: 1, b: 1, c: 1 }) { try { if (a.x === "b") continue; if (a.x === "c") break; log += a.x; } finally { log += "!"; } }
    return log === "a!!!" && a.x === "c";
  });
  row(function () { let a = null; for (a.x in { x: 1 }) ; });
  row(function () { let a = null; for (a[0] in { x: 1 }) ; });
  row(function () {
    let a = {}, log = "";
    function obj() { log += "o"; return a; }
    function key() { log += "k"; return "x"; }
    for (obj()[key()] in { a: 1, b: 1, c: 1 }) log += a.x;
    return log === "okaokbokc";
  });
  // for_in_over_arrays
  let keys = function (body) { let s = ""; body(function (k) { s += k + ","; }); return s; };
  row(function () {
    let i, tab, a; let strings = true;
    a = [];
    for (i = 0; i < 10; i++) a[a.length] = i;
    tab = "";
    for (i in a) { strings = strings && typeof i === "string"; tab += (tab === "" ? "" : ",") + i; }
    return strings && tab === "0,1,2,3,4,5,6,7,8,9";
  });
  row(function () { let s = ""; for (const k in [5, 6]) s = s + typeof k + ":" + (k === "0" || k === "1") + ","; return s; });
  row(function () { return keys(function (see) { const h = [1, 2, 3]; delete h[1]; for (const k in h) see(k); }); });
  row(function () {
    return keys(function (see) {
      const a = [0, 1, 2, 3, 4, 5]; delete a[0]; delete a[2]; delete a[3]; delete a[5];
      for (const k in a) see(k);
      const b = []; b[3] = 1; b[0] = 1; b[1000] = 1;
      for (const k in b) see(k);
    });
  });
  row(function () {
    return keys(function (see) { const a = [1, 2]; a.extra = 1; a.b = 1; a[2] = 1; for (const k in a) see(k); });
  });
  row(function () {
    return keys(function (see) {
      const a = [0]; a["01"] = 1; a[4294967295] = 1; a[-1] = 1; a[1.5] = 1;
      for (const k in a) see(k);
    });
  });
  row(function () {
    return keys(function (see) {
      const a = [0, 1, 2, 3, 4];
      for (const k in a) { if (k === "0") a.length = 2; see(k); }
      const c = [0, 1];
      for (const k in c) { c[c.length] = 9; see(k); }
      const d = [0, 1, 2, 3];
      for (const k in d) { if (k === "0") { delete d[1]; delete d[2]; d[2] = 2; } see(k); }
    });
  });
  row(function () {
    let s = "";
    const a = []; a[3] = 3; a[4000] = 4; a.extra = 1;
    for (const k in a) { if (k === "3") a.length = 0; s = s + k + ","; }
    return s + a.length;
  });
  row(function () {
    let s = "", k; const a = ["x", "y"]; a.z = "w";
    for (k in a) s = s + k + "=" + a[k] + ";";
    for (var v in a) s = s + v;
    return s;
  });
  return log;
})();
