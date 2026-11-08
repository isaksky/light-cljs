// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 322-381
// (`return_runs_finally_blocks_innermost_first`), 383-449
// (`finally_overrides_a_prior_return_or_throw`), 451-479 (`catch_blocks_rethrow`)
// and 807-821 (`saved_completions_survive_calls_from_finally`) (bbp-11, bbp-27):
// a return skips the rest of its try block and runs finally blocks innermost
// first, also from a catch block; the return value is evaluated before the
// finally block runs; a normally completing finally block keeps a pending
// throw or return; a return or throw out of a finally block replaces a
// pending return or throw (also `try { throw 1 } catch { throw 2 } finally
// { return 3 }`); a return overriding a return still runs the outer finally;
// a caught exception inside a finally block keeps the pending return; the
// finally blocks of unwound frames run on the way out; a rethrow leaves
// through the finally block of its statement; finally blocks that call a
// function two frames deep while the saved completion is the only reference
// to its value. Logs go to local arrays (for `__rt.effect`), arrays and their
// lengths stand for `__rt.vector` and `__rt.count`.
// Not ported: the uncaught rethrow's reported source position (the harness
// compares no positions). Completes with
// "10:1,3|1:1,2|10:9|1:2|4:1|2|3|4|2|1|2:7|1:2|6:1,2|3|6|3:9|32:2,3,1".
(function () {
  var r = [];
  var log;
  function logged(v) { r.push(v + ":" + log.join(",")); }
  // A return skips the rest of the try block; its finally block runs.
  log = [];
  function a1() { try { log.push(1); return 10; log.push(2); } finally { log.push(3); } log.push(4); }
  logged(a1());
  // Innermost first.
  log = [];
  function a2() { try { try { return 1; } finally { log.push(1); } } finally { log.push(2); } }
  logged(a2());
  // From a catch block, through its finally block.
  log = [];
  function a3() { try { throw 5; } catch (e) { return e * 2; } finally { log.push(9); } }
  logged(a3());
  // The return value is evaluated before the finally block runs.
  log = [];
  function a4() { var x = 1; try { return x; } finally { x = 2; log.push(x); } }
  logged(a4());
  // A normally completing finally block keeps a thrown exception.
  log = [];
  var a5 = 0;
  try { try { throw 4; } finally { log.push(1); } } catch (e) { a5 = e; }
  logged(a5);
  // An abrupt finally completion replaces the pending one.
  function b1() { try { return 1; } finally { return 2; } }
  function b2() { try { throw 1; } finally { return 3; } }
  function b3() { try { return 1; } finally { throw 4; } }
  var b3r = 0;
  try { b3(); } catch (e) { b3r = e; }
  var b4 = 0;
  try { try { throw 1; } finally { throw 2; } } catch (e) { b4 = e; }
  function b5() { try { return 1; } finally { 5; } }
  r.push(b1(), b2(), b3r, b4, b5());
  // A return overriding a pending return still runs the outer finally.
  log = [];
  function b6() { try { try { return 1; } finally { return 2; } } finally { log.push(7); } }
  logged(b6());
  // A caught exception inside a finally block keeps its pending completion.
  log = [];
  function b7() { try { return 1; } finally { try { throw 2; } catch (e) { log.push(e); } } }
  logged(b7());
  // Finally blocks of unwound frames run on the way out.
  log = [];
  function inner() { try { throw 6; } finally { log.push(1); } }
  function outer() { try { inner(); } finally { log.push(2); } }
  var b8 = 0;
  try { outer(); } catch (e) { b8 = e; }
  logged(b8);
  // A throw out of catch replaced by a return out of finally.
  function b9() { try { throw 1; } catch (e) { throw 2; } finally { return 3; } }
  r.push(b9());
  // Catch blocks rethrow, also through their statement's finally block.
  function c1() { try { throw 5; } catch (e) { throw e + 1; } }
  var c1r = 0;
  try { c1(); } catch (e) { c1r = e; }
  r.push(c1r);
  log = [];
  var c2 = 0;
  try { try { throw 1; } catch (e) { throw e * 3; } finally { log.push(9); } } catch (e) { c2 = e; }
  logged(c2);
  // Saved completions survive calls from finally blocks.
  log = [];
  function busy(n) { var i = 0; while (i < n) { i = i + 1; } return i; }
  function cleanup(n) { busy(3); log.push(n); return busy(2); }
  function make() { try { return [1, 2, 3]; } finally { cleanup(1); } }
  function fail() { try { throw [4, 5]; } finally { cleanup(2); } }
  var thrown;
  try { fail(); } catch (e) { thrown = e; } finally { cleanup(3); }
  logged(make().length * 10 + thrown.length);
  return r.join("|");
})();
