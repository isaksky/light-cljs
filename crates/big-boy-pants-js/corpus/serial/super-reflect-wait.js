function superReflectWait(dep) {
  // Super property access and Reflect.get and Reflect.set with a receiver
  // across dependency waits (light-cljs-888): `dep` is a host effect the test
  // answers with "D", in place or after the job's whole stack was saved as a
  // continuation and restored in another heap (snapshot_tests.goose). A
  // super getter runs with the method's `this` as receiver (g), a super key
  // converts (k), a super setter runs with the receiver (s); Reflect.get
  // converts its key (K, the Statics frame at pc Key) and runs a getter with
  // a separate receiver (pc Result); Reflect.set runs a setter with one (pc
  // Stored) and converts an array receiver's new `length` twice (pc
  // Length). The methods' home objects survive every restore: `h.x()` reads
  // super after the last wait. 8 waits; completes with
  // "gDkDsDKDgDsDlDlD:11,5,10,3,true,7,true,1,9".
  let log = "";
  const base = { n: 10, get g() { log += "g" + dep(); return this.n; }, set s(v) { log += "s" + dep(); this.t = v; } };
  const o = { __proto__: base, n: 1, m() { return super.g + super[{ toString() { log += "k" + dep(); return "n"; } }]; }, w(v) { super.s = v; return this.t; } };
  const h = { __proto__: { x: 9 }, x() { return super.x; } };
  const a = o.m();
  const b = o.w(5);
  const c = Reflect.get(base, { toString() { log += "K" + dep(); return "n"; } });
  const d = Reflect.get(base, "g", { n: 3 });
  const r = {};
  const e = Reflect.set(base, "s", 7, r);
  const arr = [1, 2, 3];
  const f = Reflect.set({}, "length", { valueOf() { log += "l" + dep(); return 1; } }, arr);
  return log + ":" + [a, b, c, d, e, r.t, f, arr.length, h.x()].join();
}
