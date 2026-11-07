// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9699-9929
// (`for_loops`, bbp-81) and 10594-10751 (`var_heads_in_for_loops`,
// bbp-89): their script rows, each run in its own function and its result
// appended to one log. Expression, `let`, `const` and `var` heads; empty heads; comma
// updates; head scoping; per-iteration `let` copies seen by closures made in
// the body, the test, the update and the initializer; the head's
// temporal-dead-zone ReferenceErrors and a `const` head's assignment
// TypeError; `var` heads hoisted and shared by every closure. The
// `__rt.vectorBuilder` rows collect their closures in an array literal
// built with `a[a.length] = f`. Not ported: the instruction-budget yield
// rows (a Pants embedding API), the completion-value rows (pants/control_flow)
// and the nested-loop compile-depth check (a Pants parser stack test).
// Completes with
// "true,1,4,25,3,0,4,5,200,7,10,1113,3,400,12,3,213,22,0,3,ReferenceError,ReferenceError,ReferenceError,TypeError,ReferenceError,true,undefined,7,undefined,2400,600,5,333,412,4,true,true,y,true,undefined,b,abz,ccc,abcc".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  // for_loops
  row(function () { let i, c; c = 0; for (i = 0; i < 3; i++) { c = c + 1; } return c === 3 && i === 3; });
  row(function () { for (;;) { break; } return 1; });
  row(function () { let n = 0; for (;;) { n = n + 1; if (n === 4) break; } return n; });
  row(function () { let sum = 0; for (let i = 0; i < 10; i = i + 1) { if (i % 2 === 0) continue; sum = sum + i; } return sum; });
  row(function () { let n = 0; for (let i = 0; ; i = i + 1) { if (i > 2) break; n = n + 1; } return n; });
  row(function () { let n = 0; for (let i = 5; i < 3; ) { n = 1; } return n; });
  row(function () { let i = 0; for (; i < 4; ) { i = i + 1; } return i; });
  row(function () { let r = 0; for (let i = 0, j = 10; i < j; i = i + 1, j = j - 1) r = r + 1; return r; });
  row(function () { let a, b; for (a = 1, b = 2; a < 3; a = a + 1) b = b * 10; return b; });
  row(function () { let i = 7; for (let i = 0; i < 3; i = i + 1) { let x = i; } return i; });
  row(function () { let s = 0; for (let i = 0; i < 2; i = i + 1) { let i = 5; s = s + i; } return s; });
  row(function () {
    let log = 0;
    for (let i = 1; i <= 3; i = i + 1) {
      for (let j = 1; ; j = j + 1) { if (j > i) break; if (j === 2) continue; log = log * 10 + j; }
    }
    return log;
  });
  row(function () {
    let r = 0;
    for (let i = 0; i < 3; i = i + 1) { try { continue; } finally { r = r + 1; } }
    for (;;) { try { break; } catch (e) { r = 99; } }
    return r;
  });
  row(function () {
    function find(n) { for (let i = 0; ; i = i + 1) { if (i * i >= n) return i; } }
    return find(10) * 100 + find(0);
  });
  row(function () {
    const b = [];
    for (let i = 0; i < 3; i = i + 1) { b[b.length] = function () { return i; }; }
    return b[0]() * 100 + b[1]() * 10 + b[2]();
  });
  row(function () {
    let n = 0;
    for (let i = 0; i < 5; i = i + 1) { const f = function () { i = i + 1; }; f(); n = n + 1; }
    return n;
  });
  row(function () {
    const b = [];
    for (let i = 0; i < 3; i = i + 1) { b[b.length] = () => i; i = i + 1; }
    return b.length * 100 + b[0]() * 10 + b[1]();
  });
  row(function () {
    let g = 0, h = 0;
    for (let i = 0; (g = () => i, i < 2); h = () => i, i = i + 1) {}
    return g() * 10 + h();
  });
  row(function () { let f; for (let i = 0, g = () => i; i < 3; i = i + 1) { f = g; i = i + 10; } return f(); });
  row(function () { let n = 0; for (const k = 3; n < k; ) { const f = () => k; n = n + f(); } return n; });
  row(function () { for (let i = i; ;) {} });
  row(function () { for (let i = 0, j = k, k = 1; ;) {} });
  row(function () { for (let i = (() => i)(); ;) {} });
  row(function () { for (const k = 0; k < 1; k = k + 1) {} });
  row(function () { { for (let i = 0; i < 1; i = i + 1) {} } return typeof i === "undefined" ? i : "visible"; });
  // var_heads_in_for_loops
  row(function () { var c; c = 0; for (var j = 0; j < 3; j++) { c++; } return c === 3 && j === 3; });
  row(function () { const before = j; for (var j = 0; j < 3; j = j + 1) {} return before; });
  row(function () {
    function f() { const before = j; for (var j = 5; j < 7; j = j + 1) {} return before === undefined ? j : -1; }
    return f();
  });
  row(function () { function f(go) { if (go) for (var k = 1; ; ) break; return k; } return f(false); });
  row(function () { var b = 4; for (var a = 0, b; a < 2; a = a + 1) b = b * 10; return a * 1000 + b; });
  row(function () {
    function f() { for (var i = 0; i < 10; i = i + 1) { { if (i === 3) break; } } for (var i = i * 2; i < 100; i = i * 10) {} return i; }
    return f();
  });
  row(function () { function f(i) { for (var i = i + 1; i < 5; i = i + 1) {} return i; } return f(2); });
  row(function () {
    const b = [];
    for (var i = 0; i < 3; i = i + 1) { b[b.length] = function () { return i; }; }
    return b[0]() * 100 + b[1]() * 10 + b[2]();
  });
  row(function () {
    let n = 0;
    for (var i = 0; i < 10; i = i + 1) { const skip = function () { i = i + 2; }; skip(); n = n + 1; }
    return n * 100 + i;
  });
  row(function () { for (var i = 0; ; i = i + 1) { if (i < 4) continue; break; } return i; });
  row(function () { var tab; tab = ""; for (var j in { x: 1, y: 2 }) { tab = tab + j + ","; } return tab === "x,y,"; });
  row(function () { for (var k in { x: 1, y: 2 }) {} return k === "y"; });
  row(function () { function f() { for (var k in { x: 1, y: 2 }) {} return k; } return f(); });
  row(function () { const before = k; for (var k in { a: 1 }) {} return before === undefined && k === "a"; });
  row(function () { function f() { for (var k in {}) {} return k; } return f(); });
  row(function () { for (var k in { a: 1, b: 2, c: 3 }) { if (k === "b") break; } return k; });
  row(function () { let s = ""; for (var k in { a: 1, b: 2 }) { s = s + k; k = "z"; } return s + k; });
  row(function () {
    const b = [];
    for (var k in { a: 1, b: 2, c: 3 }) { b[b.length] = function () { return k; }; }
    return b[0]() + b[1]() + b[2]();
  });
  row(function () { function f() { var s = ""; for (var k in { a: 1, b: 2, c: 3 }) { s = s + k; } return s + k; } return f(); });
  return log;
})();
