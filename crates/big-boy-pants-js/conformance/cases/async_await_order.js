// bbp-authored conformance case (light-cljs-790).
// Derives from: none (the specification's Await and promise job order
// (ECMAScript 27.7.5.3 Await, 27.2.1.3.2 Promise Resolve Functions and
// 27.2.2.2 NewPromiseResolveThenableJob), the order QuickJS and the Pants
// engine run). Async functions awaiting fulfilled values, other async
// functions' promises and rejected promises, interleaved; an exception in
// an async function rejecting its promise and a caller's catch and finally
// blocks seeing it; and a returned promise adopted two jobs later. `check`
// compares the values and the whole order once every function finished: a
// wrong one rejects its promise, which nothing handles, so the case fails
// on every engine. Values are strings, so only string concatenation runs.
// Completes with the synchronous part of the log.
(function () {
  var log = "";
  async function a() { log += "a1"; await "0"; log += "a2"; await null; log += "a3"; return "A"; }
  async function b() { log += "b1"; var v = await a(); log += "b2" + v; return v + "B"; }
  async function thrower() { log += "t1"; await "0"; throw "7"; }
  async function catcher() {
    try { await thrower(); log += "unreachable"; }
    catch (e) { log += "c" + e; }
    finally { log += "f"; }
    return "1";
  }
  async function inner() { return "5"; }
  async function returnsPromise() { log += "r1"; return inner(); }
  async function ticks() { log += "k1"; await "0"; log += "k2"; await "0"; log += "k3"; await "0"; log += "k4"; await "0"; log += "k5"; }
  var pb = b(), pc = catcher(), pr = returnsPromise(), pk = ticks();
  log += "|";
  var early = log;
  async function check() {
    var x = await pb;
    var y = await pc;
    var z = await pr;
    await pk;
    if (x !== "AB" || y !== "1" || z !== "5") throw "values " + x + " " + y + " " + z;
    if (log !== "b1a1t1r1k1|a2k2a3c7fk3b2Ak4k5") throw "order " + log;
  }
  check();
  return early;
})();
