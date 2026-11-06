// A generator one job leaves suspended (light-cljs-790): the job returns it
// at its yield, and the next job on the same reused worker cannot reach it
// (snapshot_tests.goose).
function leftSuspended() {
  function* g() { try { yield "1"; } finally { } }
  var it = g(); it.next();
  return it;
}
