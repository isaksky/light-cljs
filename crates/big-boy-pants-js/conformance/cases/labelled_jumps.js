// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 10781-10998
// (`labelled_break_and_continue`, bbp-82): its script rows, each
// run in its own function and its result appended to one log. `break` and
// `continue` to outer labels from nested loops; labelled blocks, `if` and
// `try`; `break L` through nested finally blocks, innermost first; labelled
// `continue` through finally; a jump out of a catch body; a label in a
// function body; test_labels and test_labels2's unlabelled `break` in a
// labelled block; test_cyclic_labels' `l: break l;` statements inside a loop
// that a later `break` leaves. Not ported, because the oracles disagree
// (QuickJS deviates from the specification on both; the Pants engine and
// Goose agree): `a: b: while (i < 5) { ... continue a; ... }` is a
// SyntaxError on QuickJS, though the outer label of a label set is a valid
// `continue` target (Pants 5); and in `L: for (let i = 0; i < 3; i++) {
// const h = () => i; if (i === 0) { fs = h; continue L; } g = h; break L; }`
// QuickJS's first closure sees the next iteration's binding (11, against 1).
// Also not ported: the instruction-budget, cancellation and compile-depth
// rows (Pants embedding and parser tests) and the completion-value rows
// (pants/control_flow). Completes with
// "true,undefined,true,true,4,32,3,1,11,12,3,2,21,3".
(function () {
  var log = "";
  function row(f) { var v; try { v = f(); } catch (e) { v = "thrown:" + e; } log += (log === "" ? "" : ",") + v; }
  row(function () {
    let i, c; c = 0;
    L1: for (i = 0; i < 3; i = i + 1) { c = c + 1; if (i == 0) continue; while (1) { break L1; } }
    return c === 2 && i === 1;
  });
  row(function () {
    do x: { break x; } while (0);
    if (1)
      x: { break x; }
    else
      x: { break x; }
    while (0) x: { break x; };
  });
  row(function () {
    while (1) label: break
    let i = 0
    while (i < 3) label: {
      if (i > 0)
        break
      i++
    }
    return i === 1
  });
  row(function () {
    for (;;) label: break
    let i
    for (i = 0; i < 3; i++) label: {
      if (i > 0)
        break
    }
    return i === 1
  });
  row(function () {
    let s = 0;
    outer: for (let i = 0; i < 4; i++) {
      let j = 0;
      inner: while (true) {
        if (j === i) continue outer;
        do { s = s + j; j++; continue inner; } while (false);
      }
    }
    return s;
  });
  row(function () {
    let i = 0, t = 0;
    L: do { i++; while (true) { if (i < 3) continue L; break L; } } while ((t = t + 1) < 5);
    return i * 10 + t;
  });
  row(function () {
    let n = 0;
    L: { while (true) { do { n++; if (n > 2) break L; } while (true); } n = 99; }
    return n;
  });
  row(function () { let n = 0; L: if (true) { n = 1; break L; n = 2; } else n = 3; return n; });
  row(function () { let n = 0; L: try { n = 1; break L; } catch (e) { n = 2; } n = n + 10; return n; });
  row(function () {
    let log = 0;
    L: { try { try { break L; } finally { log = log * 10 + 1; } } finally { log = log * 10 + 2; } log = 99; }
    return log;
  });
  row(function () {
    let log = 0;
    outer: for (let i = 0; i < 3; i++) {
      for (;;) { try { if (i < 2) continue outer; break outer; } finally { log = log + 1; } }
    }
    return log;
  });
  row(function () {
    let r = 0;
    try { L: try { throw 1; } catch (e) { break L; } throw 2; } catch (e) { r = e; }
    return r;
  });
  row(function () {
    function f(x) { x: { if (x) break x; return 1; } return 2; }
    return f(true) * 10 + f(false);
  });
  // test_cyclic_labels' body, left after three passes.
  row(function () {
    let n = 0;
    for (;;) {
      l: break l;
      l: break l;
      l: break l;
      if (++n === 3) break;
    }
    return n;
  });
  return log;
})();
