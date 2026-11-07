// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/src/frontend/lower.rs lines 5100-5118
// (`AppendHole` for an elided element, bbp-118) and
// crates/big-boy-pants-js/rust/src/vm/property.rs `append` (ECMAScript
// 13.2.4.1 ArrayAccumulation, Elision): an elided element adds one to the
// array literal's length without an element; `1 in` is false and a read
// gives undefined; a trailing comma adds nothing (`[1, ].length` is 1) but a
// trailing hole does (`[1, , ].length` is 2); elisions alone (`[, , ]`);
// for-in skips holes, for-of and the array iterator read them as undefined;
// `in` does not see them (no prototype holds an index); later elements keep their indices;
// writing a hole's index fills it; spreading a hole (`[...[ , ]]`) makes an
// own undefined element.
// Completes with
// "3,false,undefined,1,2,2:false:false,02,1undefined3,2:true:undefined,4:3:false,1:false:true,1:true:undefined".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  row(function () { return [1, , 3].length; });
  row(function () { return 1 in [1, , 3]; });
  row(function () { return [1, , 3][1]; });
  // A trailing comma is not a hole; a trailing hole is.
  row(function () { return [1, ].length; });
  row(function () { return [1, , ].length; });
  row(function () { const a = [, , ]; return a.length + ":" + (0 in a) + ":" + (1 in a); });
  // for-in skips holes; for-of reads them as undefined.
  row(function () { let s = ''; for (const k in [1, , 3]) s += k; return s; });
  row(function () { let s = ''; for (const v of [1, , 3]) s += v; return s; });
  row(function () { const it = [, 7].values(); const r = it.next(); return it.next().value === 7 ? "2:" + (r.done === false) + ":" + r.value : "no"; });
  // Later elements keep their indices; a hole between them stays absent.
  row(function () { const a = [0, , 2, 3]; return a.length + ":" + a[3] + ":" + (1 in a); });
  // Writing a hole's index fills it.
  row(function () { const a = [, 5]; const before = 0 in a; a[0] = 1; return a[0] + ":" + before + ":" + (0 in a); });
  // Spreading a hole makes an own undefined element (test_spread's row).
  row(function () { const a = [...[, ]]; return a.length + ":" + (0 in a) + ":" + a[0]; });
  return log;
})();
