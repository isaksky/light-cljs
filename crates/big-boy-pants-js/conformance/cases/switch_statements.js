// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 11027-11334
// (`switch_statements`, bbp-83): its script rows, each run in its
// own function and its result appended to one log. Fallthrough; `default`
// first, in the middle and last; the discriminant evaluated once and case
// expressions in order only until one matches; strict equality (`NaN` never
// matches, `-0` matches `0`); `break`, `continue` and labelled `break` out
// of a switch in a loop, also through finally blocks; the case block as one
// scope: a hoisted function, a `let` shared across clauses, its
// temporal-dead-zone ReferenceError from a later clause, fresh bindings per
// execution and closures over it. Not ported: the instruction-budget yield
// row and the error positions (Pants embedding API details) and the
// compile-depth check. Completes with
// "true,true,true,true,1,2,2,2,2,2,0,1291,12378,12,3,134,12,30,1,135,212,30,7,ReferenceError,ReferenceError,ReferenceError,ReferenceError,ReferenceError,2,1,ReferenceError,5,ReferenceError,124db".
(function () {
  var log = "";
  function row(f) {
    var v;
    try { v = f(); } catch (e) { v = e instanceof Error ? e.name : "thrown:" + e; }
    log += (log === "" ? "" : ",") + v;
  }
  row(function () {
    let i, a, s; s = 0;
    for (i = 0; i < 3; i++) {
      a = 0;
      switch (i) { case 0: a = 1; break; case 1: a = 2; break; default: a = 3; break; }
      s = s * 10 + a;
    }
    return s === 123 && i === 3;
  });
  row(function () {
    let i, a, s; s = 0;
    for (i = 0; i < 4; i++) {
      a = 0;
      switch (i) { case 0: a = 1; break; case 1: a = 2; break; case 2: continue; default: a = i; break; }
      s = s * 10 + a;
    }
    return s === 123 && i === 4;
  });
  row(function () {
    let i, a, s; s = "";
    for (i = 0; i < 3; i++) {
      a = "?";
      switch (i) { case 0: a = "a"; break; case 1: a = "b"; break; default: a = "c"; break; }
      s += a;
    }
    return s === "abc" && i === 3;
  });
  row(function () {
    let i, a, s; s = "";
    for (i = 0; i < 4; i++) {
      a = "?";
      switch (i) { case 0: a = "a"; break; case 1: a = "b"; break; case 2: continue; default: a = "" + i; break; }
      s += a;
    }
    return s === "ab3" && i === 4;
  });
  row(function () { let r = 0; switch (0) { case -0: r = 1; } return r; });
  row(function () { let r = 0; switch (NaN) { case NaN: r = 1; break; default: r = 2; } return r; });
  row(function () { let r = 0; switch ("1") { case 1: r = 1; break; default: r = 2; } return r; });
  row(function () { let r = 0; switch ("ab") { case "a": r = 1; break; case "a" + "b": r = 2; } return r; });
  row(function () { let r = 0; switch (null) { case undefined: r = 1; break; case null: r = 2; } return r; });
  row(function () { let r = 0; const f = () => 1; switch (f) { case () => 1: r = 1; break; case f: r = 2; } return r; });
  row(function () { let r = 0; switch (3) { case 1: r = 1; case 2: r = 2; } return r; });
  row(function () {
    let n = 0, d = 0;
    function t(v) { n = n * 10 + v; return v; }
    switch ((d = d + 1, 2)) { case t(1): case t(2): n = n * 10 + 9; break; case t(3): n = 0; }
    return n * 10 + d;
  });
  row(function () {
    let n = 0;
    function t(v) { n = n * 10 + v; return v; }
    switch (5) { case t(1): n = 0; default: n = n * 10 + 7; case t(2): n = n * 10 + 8; break; case t(3): n = 0; }
    return n;
  });
  row(function () { let s = 0; switch (9) { default: s = s * 10 + 1; case 1: s = s * 10 + 2; break; case 2: s = 99; } return s; });
  row(function () { let s = 0; switch (3) { case 1: s = 1; default: s = s * 10 + 2; case 3: s = s * 10 + 3; } return s; });
  row(function () { let s = 0; switch (1) { case 1: s = s * 10 + 1; case 2: case 3: s = s * 10 + 3; default: s = s * 10 + 4; } return s; });
  row(function () { let s = 0; switch (1) { case 1: switch (2) { case 2: s = 1; break; } s = s * 10 + 2; } return s; });
  row(function () { let s = 0; switch (1) { case 1: for (let i = 0; ; i++) { if (i === 3) break; s++; } s = s * 10; } return s; });
  row(function () { let s = 0; L: switch (1) { case 1: while (true) { s++; break L; } s = 99; } return s; });
  row(function () {
    let s = 0, i = 0;
    L: while (i < 5) { i++; switch (i % 2) { case 0: continue L; default: s = s * 10 + i; } }
    return s;
  });
  row(function () {
    let log = 0;
    for (let i = 0; i < 3; i++) {
      switch (i) {
        case 1: try { continue; } finally { log = log * 10 + 1; }
        default: try { break; } finally { log = log * 10 + 2; }
      }
    }
    return log;
  });
  row(function () { function f(x) { switch (x) { case 1: return 10; default: return 20; } } return f(1) + f(2); });
  row(function () { let r = 0; switch (1) { case 0: function f() { return 7; } case 1: r = f(); } return r; });
  row(function () { switch (1) { case 0: let y = 1; case 1: y; } });
  row(function () { switch (1) { case 0: let y = 1; case 1: y = 2; } });
  row(function () { switch (1) { case 0: const c = 1; default: case 1: c; } });
  row(function () { switch (1) { case y: let y = 1; } });
  row(function () { switch (1) { case 0: let y = 1; case 1: function g() { return y; } g(); } });
  row(function () { let r = 0; switch (0) { case 0: let y = 1; case 1: y = y + 1; r = y; } return r; });
  row(function () {
    let s = 0;
    for (let i = 0; i < 3; i++) { switch (i) { case 0: let y = 5; case 1: s = s * 10 + i; default: } }
    return s;
  });
  row(function () { for (let i = 0; i < 2; i++) { switch (i) { case 0: let y = 5; case 1: y; } } });
  row(function () { let f; switch (0) { case 0: f = () => x; case 1: let x = 1; case 2: x = 5; } return f(); });
  row(function () { switch (0) { case 0: let g = () => x; g(); case 1: let x = 1; } });
  // Case expressions run in source order, `default` last, until one matches.
  row(function () {
    let log = "";
    function k(v) { log += v; return v; }
    switch (3) { case k(1): log += "a"; default: log += "d"; case k(2): log += "b"; break; case k(4): log += "c"; }
    return log;
  });
  return log;
})();
