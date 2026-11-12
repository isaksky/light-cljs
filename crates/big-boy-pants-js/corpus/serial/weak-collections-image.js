function weakCollectionsImage() {
  // WeakMaps and WeakSets a later job does not own (light-cljs-945;
  // serial_tests.goose weak_image_rows): one job makes them, and jobs whose
  // rows start after it (Job.owned_nodes, as a reused worker's image rows
  // are) read them, keys made by the first job included, while every write
  // (set, add, a delete that finds its key) faults NativeBoundary before it
  // changes anything. A delete of a missing key, or of one the collection
  // cannot hold, writes nothing; a key it cannot hold is still a TypeError
  // for set and add.
  const k = {}, s = Symbol("s");
  const m = new WeakMap([[k, "a"], [s, "b"]]);
  const w = new WeakSet([k, s]);
  return {
    m, w, k, s,
    read(m, w, k, s) {
      let refused = "";
      for (const f of [() => m.set(1, 1), () => w.add(Symbol.for("r"))]) { try { f(); } catch (e) { refused += e.name; } }
      return "" + m.get(k) + m.get(s) + m.has(k) + m.has({}) + w.has(k) + w.has(s) + m.delete({}) + w.delete(1) + m.get(1) + refused;
    },
    setMap(m, w, k) { m.set(k, "c"); },
    deleteMap(m, w, k) { m.delete(k); },
    addSet(m, w, k) { w.add({}); },
    deleteSet(m, w, k) { w.delete(k); },
  };
}
