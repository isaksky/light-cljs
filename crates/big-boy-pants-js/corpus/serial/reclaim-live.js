// A live set over a job limit (light-cljs-847): every object stays
// reachable from the array, so reclamation frees nothing that matters and
// the job still ends MemoryLimit at the 100,000-node limit, as it did
// without reclamation (serial_tests.goose). V8 and the Pants engine have no
// such limit, so the outcome is Goose's own.
function reclaimLive() {
  const kept = [];
  for (let i = 0; i < 150000; i++) kept[i] = {i};
  return kept.length;
}
