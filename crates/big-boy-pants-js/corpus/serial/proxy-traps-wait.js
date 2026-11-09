function proxyTrapsWait(dep) {
  // The Proxy traps of light-cljs-891 across dependency waits: `dep` is a
  // host effect the test answers with "D", in place or after the job's
  // whole stack was saved as a continuation and restored in another heap
  // (snapshot_tests.goose). Each trap waits while its Proxy frame awaits
  // the trap's result: apply (a), construct (c), defineProperty under a
  // trapless [[Set]] at its receiver phase (d), isExtensible (e),
  // getPrototypeOf under instanceof's HasInstance frame (g), setPrototypeOf
  // (s) and a revocable proxy's get (r); the revoked proxy then refuses,
  // and one more wait (v) holds the revoked proxy and its spent revoke
  // function, which keep their state through a save and restore.
  // 8 waits; completes with
  // "aDcDdDeDgDsDrDvD:6,2,4,true,true,true,3,true,object,true,true".
  let log = "";
  const f = new Proxy(function (a) { return a + 1; }, { apply(t, th, args) { log += "a" + dep(); return Reflect.apply(t, th, args) * 2; } });
  const C = new Proxy(function () { this.k = 1; }, { construct(t, args, nt) { log += "c" + dep(); return { k: args.length }; } });
  const target = {};
  const p = new Proxy(target, {
    defineProperty(t, k, d) { log += "d" + dep(); return Reflect.defineProperty(t, k, d); },
    isExtensible(t) { log += "e" + dep(); return Reflect.isExtensible(t); },
    getPrototypeOf(t) { log += "g" + dep(); return Array.prototype; },
    setPrototypeOf(t, v) { log += "s" + dep(); return Reflect.setPrototypeOf(t, v); },
  });
  const r = Proxy.revocable({}, { get() { log += "r" + dep(); return 3; } });
  const a = f(2);
  const b = new C(1, 2).k;
  p.x = 4;
  const e = Object.isExtensible(p);
  const g = p instanceof Array;
  Object.setPrototypeOf(p, null);
  const h = r.proxy.v;
  r.revoke();
  let revoked = false;
  try { r.proxy.v; } catch (x) { revoked = x instanceof TypeError; log += "v" + dep(); }
  let array = false;
  try { Array.isArray(r.proxy); } catch (x) { array = x instanceof TypeError; }
  return log + ":" + [a, b, target.x, e, g, Object.getPrototypeOf(target) === null, h, revoked, typeof r.proxy, array, r.revoke() === undefined].join();
}
