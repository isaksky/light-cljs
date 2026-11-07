// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 9339-9497
// (`while_break_and_continue`, bbp-78) and 9522-9673 (`do_while_loops`,
// bbp-80): their script rows, each run in its own function and its
// result appended to one log. Nested `break`/`continue`, per-iteration body
// bindings after `continue`, jumps that pop catch handlers and run finally
// blocks, `continue` in a `do`-`while` evaluating the test, and
// `do ... while (c)` followed by a statement on the same line (ASI). Not
// ported: the instruction-budget yield rows (a Pants embedding API) and the
// script completion-value rows (covered by pants/control_flow and the
// completion slices). Completes with
// "true,25,1010130,12,400,thrown:2,thrown:3,thrown:4,thrown:5,7,30,1,0,1,0,true,1,40,33,25,101201233,20,13,400,34,10,0".
(function () {
  var log = "";
  function row(f) { var v; try { v = f(); } catch (e) { v = "thrown:" + e; } log += (log === "" ? "" : ",") + v; }
  // while_break_and_continue
  row(function () {
    let i, c; i = 0; c = 0;
    while (i < 3) { c = c + 1; if (i == 1) break; i = i + 1; }
    return c === 2 && i === 1;
  });
  row(function () {
    let i = 0, sum = 0;
    while (i < 10) { i = i + 1; if (i % 2 === 0) continue; if (i >= 10) continue; sum = sum + i; }
    return sum;
  });
  row(function () {
    let i = 0, log = 0;
    while (i < 3) {
      i = i + 1;
      let j = 0;
      while (true) { j = j + 1; if (j === 2) continue; if (j > i) break; log = log * 10 + j; }
      log = log * 10;
    }
    return log;
  });
  row(function () {
    let fs = 0, i = 0;
    while (i < 3) { i = i + 1; const k = i; if (k === 1) { fs = () => k; continue; } if (k === 2) break; }
    return fs() * 10 + i;
  });
  row(function () {
    function find(n) { let i = 0; while (true) { if (i * i >= n) break; i = i + 1; } return i; }
    return find(10) * 100 + find(0);
  });
  // A `try` left by `break` or `continue` pops its handler: the later
  // `throw` escapes to the row's catch.
  row(function () { let n = 0; while (n === 0) { try { n = 1; break; } catch (e) { n = 99; } } throw n + 1; });
  row(function () { let n = 0; while (n < 3) { try { n = n + 1; continue; } catch (e) { n = 99; } } throw n; });
  row(function () { let n = 0; while (n === 0) { n = 1; try { try { break; } catch (e) { n = 50; } } catch (e) { n = 60; } } throw n + 3; });
  row(function () { let n = 0; while (n === 0) { n = 1; try { try { throw 1; } catch (e) { break; } } catch (e) { n = 60; } } throw n + 4; });
  row(function () {
    let r = 0, n = 0;
    try { while (n === 0) { n = 1; try { break; } catch (e) { r = 1; } } throw 7; } catch (e) { r = r * 10 + e; }
    return r;
  });
  row(function () {
    let r = 0;
    try { while (true) { r = r + 1; if (r < 3) continue; break; } } finally { r = r * 10; }
    return r;
  });
  row(function () { let x = 1; while (x) { try { break; } finally {} } return x; });
  row(function () { let x = 1; while (x) { try { throw 0; } catch (e) { x = 0; continue; } finally {} } return x; });
  row(function () { let x = 1; while (x) { try {} finally { break; } } return x; });
  row(function () { let x = 1; while (x) { try { try { x = 0; continue; } catch (e) {} } finally {} } return x; });
  // do_while_loops
  row(function () {
    let i, c; i = 0; c = 0;
    do { c = c + 1; i = i + 1; } while (i < 3);
    return c === 3 && i === 3;
  });
  row(function () { let n = 0; do { n = n + 1; } while (false); return n; });
  row(function () {
    let n = 0, m; do n = n + 1; while (n < 4) m = n * 10
    return m;
  });
  row(function () { let i = 0, n = 0; do { n = n + 1; continue; n = 99; } while ((i = i + 1) < 3); return n * 10 + i; });
  row(function () {
    let i = 0, sum = 0;
    do { i = i + 1; if (i % 2 === 0) continue; sum = sum + i; } while (i < 9);
    return sum;
  });
  row(function () {
    let i = 0, log = 0;
    do {
      i = i + 1;
      let j = 0;
      do { j = j + 1; if (j > i) break; log = log * 10 + j; } while (true);
      if (i === 3) break;
      log = log * 10;
    } while (true);
    return log * 10 + i;
  });
  row(function () {
    let a = 0, b = 0;
    while (a < 2) { a = a + 1; do { b = b + 1; continue; } while (false); }
    do { while (true) { break; } b = b * 10; } while (false);
    return b;
  });
  row(function () {
    let fs = 0, i = 0;
    do { i = i + 1; const k = i; if (k === 1) { fs = () => k; continue; } } while (i < 3);
    return fs() * 10 + i;
  });
  row(function () {
    function find(n) { let i = 0; do { if (i * i >= n) return i; i = i + 1; } while (true); }
    return find(10) * 100 + find(0);
  });
  row(function () {
    let r = 0, n = 0;
    do { try { n = n + 1; continue; } finally { r = r + 1; } } while (n < 3);
    do { try { break; } catch (e) { r = 99; } } while (true);
    try { throw 4; } catch (e) { r = r * 10 + e; }
    return r;
  });
  row(function () {
    let r = 0;
    do { try { throw 1; } catch (e) { r = e; break; } finally { r = r * 10; } } while (true);
    return r;
  });
  row(function () { let x = 2; do { let x = 0; } while ((x = x - 1) > 0); return x; });
  return log;
})();
