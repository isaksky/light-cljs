// bbp-authored conformance case (light-cljs-869).
// Derives from: none (light-cljs-765's probe rows, after the Pants engine's
// unwinding tests in crates/big-boy-pants-js/rust/tests/exceptions.rs and
// bbp-44's rule that the error objects a job makes and drops stay within its
// limits): loops of 200,000 caught `throw i`, `null.x` TypeErrors,
// `throw new Error('e')` and try/finally with `continue`. Each ran on Goose
// well inside the harness's 30-second timeout at 258f870a. Completes with
// "19999900000|200000|200000|300000".
(function () {
  var r = [];
  var s = 0;
  for (var i = 0; i < 200000; i++) { try { throw i; } catch (e) { s += e; } }
  r.push(s);
  var n = 0;
  for (var j = 0; j < 200000; j++) { try { null.x; } catch (e) { if (e instanceof TypeError) n++; } }
  r.push(n);
  var m = 0;
  for (var k = 0; k < 200000; k++) { try { throw new Error("e"); } catch (e) { m += e.message.length; } }
  r.push(m);
  var f = 0;
  for (var l = 0; l < 200000; l++) { try { if (l % 2) continue; f++; } finally { f++; } }
  r.push(f);
  return r.join("|");
})();
