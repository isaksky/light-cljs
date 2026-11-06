// Dependency waits with native frames on the job's stack (light-cljs-813):
// `dep` is a host effect the test answers with "D", twice, each time after
// the job's continuation was saved and restored in another heap. Both waits
// are inside a map callback (an ArrayCallbacks native frame) inside a
// running generator (its Resume native frame and its frame) under a finally
// block, so the saved stack is five frames, two of them native
// (snapshot_tests.goose). Completes with "a1D2Dybf".
function suspendedStack(dep) {
  var log = "";
  function* g() {
    try {
      log += "a";
      var got = ["1", "2"].map(function (x) { return x + dep("need"); });
      log += got[0] + got[1];
      yield "y";
      log += "b";
    } finally { log += "f"; }
  }
  var it = g();
  var v = it.next().value;
  log += v;
  it.next();
  return log;
}
