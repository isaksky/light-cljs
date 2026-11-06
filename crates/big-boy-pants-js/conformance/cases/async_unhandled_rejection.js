// bbp-authored conformance case (light-cljs-790).
// Derives from: ~/src/quickjs/tests/test_std.js lines 312-335 (the
// HostPromiseRejectionTracker report QuickJS and the Pants engine give,
// test_async_promise_rejection, inverted): an async function's exception
// rejects its promise, a handled rejection (an awaiting caller's catch) is
// no report, and a rejection nothing ever handles is one, once the job
// queue has drained.
(function () {
  var seen = "";
  async function fails(v) { await "0"; throw v; }
  async function handles() { try { await fails("1"); } catch (e) { seen += e; } }
  handles();
  fails("2");
  return seen;
})();
