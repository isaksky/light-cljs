// Async functions and promise jobs (light-cljs-790): awaiting fulfilled
// values, other async functions' promises and a rejected promise, the
// order of several queued reactions and a returned promise's adoption
// (conformance/cases/async_await_order.js as a function). The entry
// returns before any promise job runs; `check` rejects, unhandled, when the
// values or the order are wrong, which fails the run on every engine.
function asyncOrder() {
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
  return early === "b1a1t1r1k1|";
}
