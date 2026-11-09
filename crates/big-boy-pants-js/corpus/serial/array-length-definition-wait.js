function arrayLengthDefinitionWait(dep) {
  // An array's `length` defined with an object value (light-cljs-898)
  // across dependency waits: `dep` is a host effect the test answers with
  // "D", in place or after the job's whole stack was saved as a
  // continuation and restored in another heap (snapshot_tests.goose). The
  // value's valueOf waits in each of ArraySetLength's two conversions
  // (ToUint32, then ToNumber): Object.defineProperty's (a) and
  // Object.defineProperties's (b) above the Descriptor frame, then a
  // trapless proxy's [[DefineOwnProperty]] forwarded to its array target,
  // from Reflect.defineProperty (c; its valueOf installs a defineProperty
  // trap the definition must not call) and from Reflect.set with the proxy
  // as receiver (d), above that Proxy frame. A proxy of a plain object
  // stays for the test's refusal checks.
  // 8 waits; completes with "aDaDbDbDcDcDdDdD:1,2,4,1,true,0,true,object".
  let log = "";
  const plain = new Proxy({}, {});
  const length = (tag, n) => ({ valueOf() { log += tag + dep(); return n; } });
  const a = [1, 2, 3];
  Object.defineProperty(a, "length", { value: length("a", 1) });
  const b = [1, 2, 3];
  Object.defineProperties(b, { x: { value: 4 }, length: { value: length("b", 2) } });
  const c = [1, 2, 3], h = {};
  const p = new Proxy(c, h);
  const ok = Reflect.defineProperty(p, "length", { value: { valueOf() { log += "c" + dep(); h.defineProperty = () => false; return 1; } } });
  const d = [1, 2, 3];
  const set = Reflect.set({}, "length", length("d", 0), new Proxy(d, {}));
  return log + ":" + [a.length, b.length, b.x, c.length, ok, d.length, set, typeof plain].join();
}
