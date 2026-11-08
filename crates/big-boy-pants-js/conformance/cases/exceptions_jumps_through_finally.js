// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 855-1008
// (`break_and_continue_run_finally_blocks`) (bbp-79), the rows that are not
// script completion values (those are the statements area's
// completion_finally_* slices) and not test_try_catch5 (QuickJS's
// test_loop.js/test_try_catch5 case): `continue` through a finally block once
// per iteration, after the protected code and before the test; a `break`
// through two finally blocks and the catch handler between them, which does
// not catch the later throw; `continue` from a catch body; a return or throw
// in a finally block replacing a pending `break`; `break` or `continue` in a
// finally block discarding a pending exception, return or jump; a jump that
// stays inside a finally block keeping its pending return. Plus rows written
// for this slice (light-cljs-765's probe): `continue outer` through a finally
// block, a labelled block left by `break lbl` through a finally block, and
// `continue` in a finally block over a thrown loop counter. Logs go to local
// arrays (for `__rt.effect`). Completes with
// "3:t0,b1,f1,a1,t1,f2,t2,b3,f3,a3,t3|5:1,2,3,4|2:c1,F,c2,F|5:1,2|7:3,4|1|6|2|4:1,2,3|1|00,f,f,10,f,f|t,f,after|0,1,2".
(function () {
  var r = [];
  var log;
  function logged(v) { r.push(v + ":" + log.join(",")); }
  // `continue` through a finally block, once per iteration.
  log = [];
  var i = 0;
  while ((log.push("t" + i), i) < 3) {
    i = i + 1;
    try { if (i === 2) continue; log.push("b" + i); } finally { log.push("f" + i); }
    log.push("a" + i);
  }
  logged(i);
  // A `break` through two finally blocks and the catch handler between them.
  log = [];
  var br = 0;
  try {
    while (true) {
      try {
        try {
          try { log.push(1); break; } catch (e) { log.push(91); }
          log.push(92);
        } finally { log.push(2); }
        log.push(93);
      } finally { log.push(3); }
      log.push(94);
    }
    log.push(4);
    throw 5;
  } catch (e) { br = e; }
  logged(br);
  // `continue` from a catch body runs the finally block too.
  log = [];
  var j = 0;
  while (j < 2) {
    j = j + 1;
    try { throw j; } catch (e) { log.push("c" + e); continue; } finally { log.push("F"); }
    log.push(99);
  }
  logged(j);
  // A return in the finally block overrides the pending `break`.
  log = [];
  function f1() {
    while (true) {
      try { try { break; } finally { log.push(1); return 5; } } finally { log.push(2); }
    }
    return 6;
  }
  logged(f1());
  // A throw in the finally block replaces the pending `break`.
  log = [];
  var tr = 0;
  try {
    while (true) {
      try { try { break; } finally { log.push(3); throw 7; } } finally { log.push(4); }
    }
    tr = 1;
  } catch (e) { tr = e; }
  logged(tr);
  // `break` or `continue` in a finally block discards a pending exception,
  // return or jump.
  var d1 = 0;
  while (true) { try { throw 7; } finally { d1 = 1; break; } }
  r.push(d1);
  var k = 0, d2 = 0;
  while (k < 3) {
    k = k + 1;
    try { throw k; } catch (e) { d2 = d2 + e; throw e; } finally { continue; }
  }
  r.push(d2);
  function f2() { while (true) { try { return 1; } finally { break; } } return 2; }
  r.push(f2());
  log = [];
  var d3 = 0;
  while (true) {
    try {
      while (true) { try { break; } finally { log.push(1); break; } }
      log.push(2);
    } finally { log.push(3); }
    d3 = 4;
    break;
  }
  logged(d3);
  // A jump that stays inside a finally block keeps its pending return.
  function f3() {
    try { return 1; }
    finally { var n = 0; while (true) { try { n = n + 1; break; } finally {} } }
  }
  r.push(f3());
  // `continue outer` through a finally block.
  var lab = [];
  outer: for (var a = 0; a < 2; a++) {
    for (var b = 0; b < 2; b++) {
      try { if (b === 1) continue outer; lab.push("" + a + b); } finally { lab.push("f"); }
    }
  }
  r.push(lab.join(","));
  // A labelled block left by `break lbl` through a finally block.
  var blk = [];
  lbl: { try { blk.push("t"); break lbl; } finally { blk.push("f"); } blk.push("x"); }
  blk.push("after");
  r.push(blk.join(","));
  // `continue` in a finally block over a thrown loop counter.
  var cs = [];
  for (var m = 0; m < 3; m++) { try { throw m; } finally { cs.push(m); continue; } }
  r.push(cs.join(","));
  return r.join("|");
})();
