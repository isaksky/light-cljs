// The write log across a collection (light-cljs-847): a string written to
// globalThis.observed, then overwritten, so only the log still names it,
// then 60,000 short-lived objects, past half the 100,000-node job limit.
// A host that reads the log's values (execute.goose's typed outcome) keeps
// them through the collection; otherwise the overwritten string goes and
// its log entry reads undefined (serial_tests.goose). Completes with 2.
function reclaimObserved() {
  globalThis.observed = "v" + 1;
  globalThis.observed = 0;
  let o;
  for (let i = 0; i < 60000; i++) o = {i};
  globalThis.observed = o.i === 59999 ? 2 : -1;
  return globalThis.observed;
}
