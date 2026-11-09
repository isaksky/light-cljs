function proxyCopyWait(dep) {
  // Object spread, object rest and getOwnPropertyDescriptor through proxies
  // (light-cljs-895) across dependency waits: `dep` is a host effect the
  // test answers with "D", in place or after the job's whole stack was
  // saved as a continuation and restored in another heap
  // (snapshot_tests.goose). Each trap waits while a frame awaits it: an
  // object spread's ownKeys (k), getOwnPropertyDescriptor (o) and get (g)
  // under its CopyData frame; object rest's ownKeys (K) and
  // getOwnPropertyDescriptor (O) under the CopyData frames of its OwnKeys
  // and IsEnumerable; Object.getOwnPropertyDescriptor's (d) and
  // Reflect.getOwnPropertyDescriptor's (r) under their Descriptor frames;
  // then a proxy target's getOwnPropertyDescriptor (i) and isExtensible (e)
  // under the outer proxy's [[GetOwnProperty]] frame checking its
  // invariants. 9 waits; completes with
  // "kDoDgDKDODdDrDiDeD:1,1,2,b,1,true,2,true".
  let log = "";
  const sp = new Proxy({ a: 1 }, {
    ownKeys(t) { log += "k" + dep(); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t, k) { log += "o" + dep(); return Reflect.getOwnPropertyDescriptor(t, k); },
    get(t, k, r) { log += "g" + dep(); return Reflect.get(t, k, r); },
  });
  const spread = { ...sp };
  const rp = new Proxy({ a: 1, b: 2 }, {
    ownKeys(t) { log += "K" + dep(); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t, k) { log += "O" + dep(); return Reflect.getOwnPropertyDescriptor(t, k); },
  });
  const { a, ...rest } = rp;
  let reading = "d";
  const dp = new Proxy({ x: 1 }, { getOwnPropertyDescriptor(t, k) { log += reading + dep(); return Reflect.getOwnPropertyDescriptor(t, k); } });
  const d1 = Object.getOwnPropertyDescriptor(dp, "x");
  reading = "r";
  const d2 = Reflect.getOwnPropertyDescriptor(dp, "x");
  const inner = new Proxy({ y: 2 }, {
    getOwnPropertyDescriptor(t, k) { log += "i" + dep(); return Reflect.getOwnPropertyDescriptor(t, k); },
    isExtensible(t) { log += "e" + dep(); return Reflect.isExtensible(t); },
  });
  const outer = new Proxy(inner, { getOwnPropertyDescriptor() { return { value: 2, writable: true, enumerable: true, configurable: true }; } });
  const d3 = Object.getOwnPropertyDescriptor(outer, "y");
  return log + ":" + [spread.a, a, rest.b, Object.keys(rest).join(), d1.value, d2.writable, d3.value, d3.configurable].join();
}
