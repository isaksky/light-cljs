// bbp-authored conformance case (light-cljs-869).
// Derives from: crates/big-boy-pants-js/rust/tests/exceptions.rs lines 234-273
// (`a_thrown_value_is_caught_in_the_same_function`) and 275-320
// (`a_thrown_value_is_caught_in_an_outer_function`) (bbp-11): a thrown object
// caught by identity; undefined, null, false, 0 and '' thrown and caught; an
// omitted catch binding; code after a throw skipped and the catch body run
// once; a throw from an `if` body; a throw from two frames down; a handler
// popped when its try block finished is not reused; a closure that throws
// later reaches the handler live then; unwinding 1,000 frames, then with a
// try/catch and with a try/finally in every frame. Logs go to a local array
// (for `__rt.effect`), the thrown object is an array (for `__rt.vector`).
// Not ported: the 5,000-frame unwind (QuickJS's native stack in the harness
// driver overflows at 3,000 frames or fewer, where Goose and the Pants engine
// allow 10,000), and the thrown Symbol (`Symbol()` is pending on Goose, the
// objects area, light-cljs-766). Completes with
// "y|y,y,y,y,y|5|1,20,4|20|2|3|1,3|107|3|8|99|1000|deep|1001".
(function () {
  var r = [];
  // A thrown object, caught by identity.
  var v = [1];
  var c;
  try { throw v; } catch (e) { c = e; }
  r.push(c === v ? "y" : "n");
  // Falsy values thrown and caught.
  var falsy = [undefined, null, false, 0, ""];
  var got = [];
  for (var i = 0; i < falsy.length; i++) {
    var x = "unset";
    try { throw falsy[i]; } catch (e) { x = e; }
    got.push(x === falsy[i] ? "y" : "n");
  }
  r.push(got.join(","));
  // An omitted catch binding.
  var z = 0;
  try { throw 1; } catch { z = 5; }
  r.push(z);
  // Code after the throw does not run; the catch body runs once.
  var log = [];
  var w = 0;
  try { log.push(1); throw 2; log.push(3); } catch (e) { w = e * 10; log.push(w); }
  log.push(4);
  r.push(log.join(","), w);
  // No exception: the catch body is skipped.
  var s = 1;
  try { s = 2; } catch (e) { s = 3; }
  r.push(s);
  // `if` bodies may throw.
  var t = 0;
  try { if (t === 0) throw 3; } catch (e) { t = e; }
  r.push(t);
  // Thrown two frames down, caught in the outer function.
  var log2 = [];
  function inner() { log2.push(1); throw 7; }
  function middle() { inner(); log2.push(2); return 1; }
  function outer() { try { return middle(); } catch (e) { return e + 100; } }
  var o = outer();
  log2.push(3);
  r.push(log2.join(","), o);
  // A handler popped when its try block finished is not reused.
  function g() { try { return 1; } catch (e) { return 2; } }
  function h() { g(); throw 3; }
  var hr = 0;
  try { h(); } catch (e) { hr = e; }
  r.push(hr);
  // A closure called later throws to whatever handler is live then.
  var k;
  try { k = function () { throw 8; }; } catch (e) { k = 0; }
  var kr = 0;
  try { k(); } catch (e) { kr = e; }
  r.push(kr);
  // Unwinding 1,000 frames; then a try/catch and a try/finally in every frame.
  function f(n) { if (n === 0) { throw 99; } return f(n - 1) + 1; }
  var fr;
  try { fr = f(1000); } catch (e) { fr = e; }
  r.push(fr);
  function fc(n) { try { return n === 0 ? 0 : fc(n - 1) + 1; } catch (e) { throw e; } }
  r.push(fc(1000));
  var unwound = 0;
  function ff(d) { try { if (d === 0) throw "deep"; return ff(d - 1); } finally { unwound++; } }
  var dr;
  try { ff(1000); } catch (e) { dr = e; }
  r.push(dr, unwound);
  return r.join("|");
})();
