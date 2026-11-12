function mapSetImage() {
  // Maps and Sets a later job does not own (light-cljs-943;
  // serial_tests.goose collection_image_rows): one job makes them, and jobs
  // whose rows start after it (Job.owned_nodes, as a reused worker's image
  // rows are) read them, keys made by the first job included, while every
  // write (set, add, a delete that finds its key, clear) faults
  // NativeBoundary before it changes anything. A delete of a missing key
  // writes nothing. Keyword forms (`__rt`, Goose only) are keys by
  // namespace and name, as `===` compares them, also in a later job and in
  // a snapshot of the image (serial_tests.goose validates one).
  const k = {};
  const m = new Map([[1, "a"], [k, "b"], [__rt.formKeyword(null, "kw"), "c"], [__rt.formKeyword("ns", "kw"), "d"]]);
  const s = new Set(["x", k]);
  return {
    m, s, k, it: m.values(), si: s.values(),
    read(m, s, k) { return "" + m.get(1) + m.get(k) + m.size + m.has(k) + s.has("x") + s.has(k) + s.size + m.get(2) + m.delete(2) + s.delete("y") + m.get(__rt.formKeyword(null, "kw")) + m.get(__rt.formKeyword("ns", "kw")) + m.has(__rt.formKeyword("ns", "other")); },
    setMap(m) { m.set(3, "c"); },
    deleteMap(m, k) { m.delete(k); },
    clearMap(m) { m.clear(); },
    addSet(s) { s.add("z"); },
    deleteSet(s) { s.delete("x"); },
    clearSet(s) { s.clear(); },
    iterate(m, s, k) {
      let out = '', visits = 0;
      m.forEach(v => out += v);
      s.forEach((v, key, self) => { if (v === key && self === s) visits++; });
      return out === 'abcd' && [...m.values()].join() === 'a,b,c,d' && [...m.keys()][1] === k && [...s.keys()][1] === k && visits === 2;
    },
    advance(it) { return it.next(); },
  };
}
