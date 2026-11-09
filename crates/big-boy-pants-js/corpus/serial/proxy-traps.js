// The Proxy traps of light-cljs-891: apply and construct (a callable
// proxy as another proxy's trap), defineProperty (from Object.defineProperty
// and from a trapless [[Set]] at its proxy receiver's phases),
// isExtensible, preventExtensions, getPrototypeOf (instanceof's walk) and
// setPrototypeOf, with a proxy target's isExtensible and getPrototypeOf
// deciding their invariants, and a revocable proxy before and after its
// revocation: each a Proxy frame above the operation, which
// serial_tests.goose runs at every budget cut and collection.
function proxyTraps() {
  let log = "";
  const f = new Proxy(function (a, b) { return a + b; }, { apply(t, th, args) { log += "a"; return Reflect.apply(t, th, args) * 2; } });
  const g = new Proxy({}, { get: new Proxy(function (t, k) { return k + "!"; }, { apply(t, th, args) { log += "A"; return Reflect.apply(t, th, args); } }) });
  const C = new Proxy(function () { this.k = 1; }, { construct(t, args, nt) { log += "c"; return { k: args.length, n: nt === C }; } });
  const target = {};
  const defining = new Proxy(target, { defineProperty(t, k, d) { log += "d"; return Reflect.defineProperty(t, k, d); } });
  Object.defineProperty(defining, "x", { value: 4, writable: true, configurable: true, enumerable: true });
  defining.y = 5;
  const inner = new Proxy({}, {
    isExtensible(t) { log += "e"; return Reflect.isExtensible(t); },
    preventExtensions(t) { log += "p"; return Reflect.preventExtensions(t); },
    getPrototypeOf(t) { log += "g"; return Reflect.getPrototypeOf(t); },
    setPrototypeOf(t, v) { log += "s"; return Reflect.setPrototypeOf(t, v); },
  });
  const outer = new Proxy(inner, {
    getPrototypeOf(t) { log += "G"; return Object.prototype; },
    setPrototypeOf(t, v) { log += "S"; return Reflect.setPrototypeOf(t, v); },
  });
  Object.setPrototypeOf(outer, Object.prototype);
  const extensible = Object.isExtensible(outer);
  const prevented = Object.preventExtensions(outer) === outer;
  const instance = outer instanceof Object;
  const r = Proxy.revocable({}, { get() { log += "r"; return 3; } });
  const before = r.proxy.v;
  r.revoke();
  let revoked = false;
  try { r.proxy.v; } catch (e) { revoked = e instanceof TypeError; }
  const built = new C(1, 2);
  return f(1, 2) === 6 && g.q === "q!" && built.k === 2 && built.n && target.x === 4 && target.y === 5 && extensible && prevented && instance
    && !Object.isExtensible(inner) && before === 3 && revoked && log === "ddSseepGegrcaAe";
}
