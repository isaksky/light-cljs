// A proxy's [[Get]] and [[HasProperty]] (light-cljs-890): traps with their
// receivers, a trap read through a handler getter, forwarding through a
// nested proxy to the target's getter, a proxy on a prototype chain, and
// the invariant checks, one of them reading a proxy target's descriptor
// through its trap: each a Proxy frame above the reader, which
// serial_tests.goose runs at every budget cut and collection.
function proxyGet() {
  let log = "";
  let target = { x: 1, get y() { return this === child; } };
  let handler = {
    get get() { log += "g"; return function (t, k, r) { log += "t"; return k === "x" ? t.x + 1 : Reflect.get(t, k, r); }; },
    has(t, k) { log += "h"; return k === "virtual" || k in t; }
  };
  let p = new Proxy(target, handler);
  let child = Object.create(new Proxy(p, {}));
  let fixed = {};
  Object.defineProperty(fixed, "z", { value: 3 });
  let inner = new Proxy(fixed, { getOwnPropertyDescriptor(t, k) { log += "d"; return Reflect.getOwnPropertyDescriptor(t, k); } });
  let ok = p.x === 2 && child.y === true && "virtual" in child && !("w" in p) && new Proxy(inner, { get() { return 3; } }).z === 3;
  let caught = false;
  try { new Proxy(fixed, { get() { return 4; } }).z; } catch (e) { caught = e instanceof TypeError; }
  return ok && caught && log === "gtgthhd";
}
