// A dependency wait while continuations are suspended (light-cljs-790):
// `dep` is a host effect the test answers with "D", in place or after the
// job's continuation was saved and restored in another heap. A generator
// is suspended at its yield and an async function awaits across the wait;
// both resume afterwards (snapshot_tests.goose). Completes with
// "axDbfypz".
async function suspendedWait(dep) {
  var log = "";
  function* g() { try { log += "a"; yield "1"; log += "b"; yield "2"; } finally { log += "f"; } }
  async function af() { log += "x"; var v = await "p"; log += "y" + v; return "z"; }
  var it = g(); it.next();
  var pending = af();
  // Two more awaiters: two reactions pending on af's promise across the wait.
  async function also() { return await pending; }
  also(); also();
  log += dep("need");
  it.next(); it.return("r");
  var z = await pending;
  log += z;
  return log;
}
