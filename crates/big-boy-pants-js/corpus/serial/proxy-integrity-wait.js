function proxyIntegrityWait(dep) {
  // Object.seal, freeze, isSealed and isFrozen of a proxy and a proxy
  // target in the ownKeys trap's invariants (light-cljs-897) across
  // dependency waits: `dep` is a host effect the test answers with "D", in
  // place or after the job's whole stack was saved as a continuation and
  // restored in another heap (snapshot_tests.goose). Each trap waits while
  // a frame awaits it: seal's preventExtensions (p), ownKeys (k) and
  // defineProperty (d) under its Statics frame; freeze's and isSealed's
  // getOwnPropertyDescriptor (o, s); isFrozen's isExtensible (e), whose
  // true answer ends the test before any key is read; then an inner
  // proxy's isExtensible (E), ownKeys (K) and getOwnPropertyDescriptor (G)
  // under the outer proxy's [[OwnPropertyKeys]] frame checking its
  // invariants. Two specification steps where QuickJS departs are checked
  // without waits: freeze skips a key whose descriptor is gone, and defines
  // {writable: false} on a data property that is already read-only.
  // 9 waits; completes with
  // "pDkDdDoDsDeDEDKDGD:true,true,true,false,x,true,writable/configurable".
  let log = "";
  const st = { a: 1 };
  const s = new Proxy(st, {
    preventExtensions(t) { log += "p" + dep(); return Reflect.preventExtensions(t); },
    ownKeys(t) { log += "k" + dep(); return Reflect.ownKeys(t); },
    defineProperty(t, k, d) { log += "d" + dep(); return Reflect.defineProperty(t, k, d); },
  });
  let reading = "o";
  const ft = { b: 1 };
  const f = new Proxy(ft, {
    getOwnPropertyDescriptor(t, k) { log += reading + dep(); return Object.getOwnPropertyDescriptor(t, k); },
  });
  const e = new Proxy({}, { isExtensible(t) { log += "e" + dep(); return Reflect.isExtensible(t); } });
  const inner = new Proxy({ x: 1 }, {
    isExtensible(t) { log += "E" + dep(); return Reflect.isExtensible(t); },
    ownKeys(t) { log += "K" + dep(); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t, k) { log += "G" + dep(); return Object.getOwnPropertyDescriptor(t, k); },
  });
  const outer = new Proxy(inner, { ownKeys() { return ["x"]; } });
  Object.seal(s);
  Object.freeze(f);
  reading = "s";
  const sealed = Object.isSealed(f);
  const frozen = Object.isFrozen(e);
  const keys = Reflect.ownKeys(outer).join();
  const gt = { a: 1, b: 2 };
  const gone = new Proxy(gt, { getOwnPropertyDescriptor(t, k) { if (k === "a") { delete t.a; return undefined; } return Object.getOwnPropertyDescriptor(t, k); } });
  Object.freeze(gone);
  let defined = "";
  const ro = new Proxy(Object.defineProperty({}, "a", { value: 1 }), { defineProperty(t, k, d) { defined = Object.keys(d).join("/"); return Reflect.defineProperty(t, k, d); } });
  Object.freeze(ro);
  return log + ":" + [Object.isSealed(st), Object.isFrozen(ft), sealed && !Object.isFrozen(st), frozen, keys, !("a" in gt) && Object.isFrozen(gt), defined].join();
}
