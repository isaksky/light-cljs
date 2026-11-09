// Object spread, object rest, Object.getOwnPropertyDescriptor and
// Reflect.getOwnPropertyDescriptor through proxies (light-cljs-895): the
// ownKeys, getOwnPropertyDescriptor (read through a handler getter) and get
// traps above a CopyData frame, the proxy the receiver of the get trap and
// of a trapless proxy's accessor, object rest's OwnKeys and IsEnumerable of a
// proxy, both getOwnPropertyDescriptor built-ins above a Descriptor frame
// with getter-backed descriptor fields and a proxy as the trap's result,
// and a proxy target's getOwnPropertyDescriptor and isExtensible (through
// a handler getter) in that trap's invariants, which serial_tests.goose
// runs at every budget cut and collection.
function proxyCopy() {
  let log = "", receivers = true;
  const s = Symbol("s");
  const t = { a: 1, b: 2, [s]: 3, 7: 4 };
  Object.defineProperty(t, "hidden", { value: 5, enumerable: false });
  const p = new Proxy(t, {
    ownKeys(t) { log += "k"; return Reflect.ownKeys(t); },
    get getOwnPropertyDescriptor() { log += "h"; return function (t, k) { log += "d"; return Object.getOwnPropertyDescriptor(t, k); }; },
    get(t, k, r) { log += "g"; receivers = receivers && r === p; return Reflect.get(t, k, r); },
  });
  // A trapless proxy's accessor sees the proxy as its receiver.
  const q = new Proxy({ get x() { return this === q ? 1 : 0; } }, {});
  const { ...restQ } = q;
  const accessors = ({ ...q }).x === 1 && restQ.x === 1;
  const spread = { ...p };
  const copied = spread[7] === 4 && spread.a === 1 && spread.b === 2 && spread[s] === 3 && !("hidden" in spread) && Object.keys(spread).join() === "7,a,b";
  const spreadLog = log === "khdghdghdghdhdg";
  log = "";
  const { a, ...rest } = p;
  const rested = a === 1 && rest[7] === 4 && rest.b === 2 && rest[s] === 3 && !("a" in rest) && !("hidden" in rest) && Object.keys(rest).join() === "7,b";
  const restLog = log === "gkhdghdghdhdg";
  log = "";
  const d = Object.getOwnPropertyDescriptor(p, "a"), e = Reflect.getOwnPropertyDescriptor(p, s);
  const described = d.value === 1 && d.writable && d.enumerable && d.configurable && e.value === 3 && Object.getOwnPropertyDescriptor(p, "zz") === undefined && log === "hdhdhd";
  log = "";
  const fields = new Proxy({}, { getOwnPropertyDescriptor() { return { get enumerable() { log += "e"; return true; }, get configurable() { log += "c"; return true; }, get value() { log += "v"; return 9; } }; } });
  const viaProxy = new Proxy({}, { getOwnPropertyDescriptor() { return new Proxy({ value: 8, configurable: true }, { has(t, k) { log += "H"; return k in t; } }); } });
  const getters = Object.getOwnPropertyDescriptor(fields, "q").value === 9 && Reflect.getOwnPropertyDescriptor(viaProxy, "q").value === 8 && log === "ecvHHHHHH";
  log = "";
  const inner = new Proxy({ x: 1 }, {
    getOwnPropertyDescriptor(t, k) { log += "D"; return Reflect.getOwnPropertyDescriptor(t, k); },
    get isExtensible() { log += "x"; return function (t) { log += "E"; return Reflect.isExtensible(t); }; },
  });
  const reported = Object.getOwnPropertyDescriptor(new Proxy(inner, { getOwnPropertyDescriptor() { log += "O"; return { value: 1, writable: true, enumerable: true, configurable: true }; } }), "x").value === 1;
  const missing = Reflect.getOwnPropertyDescriptor(new Proxy(inner, { getOwnPropertyDescriptor() { log += "U"; return undefined; } }), "x") === undefined;
  const forwarded = ({ ...new Proxy(inner, {}) }).x === 1;
  const invariantLog = log === "ODxEUDxED";
  let caught = false;
  try { Object.getOwnPropertyDescriptor(new Proxy(new Proxy(Object.preventExtensions({ x: 1 }), {}), { getOwnPropertyDescriptor() { return undefined; } }), "x"); } catch (x) { caught = x instanceof TypeError; }
  return copied && spreadLog && rested && restLog && receivers && accessors && described && getters && reported && missing && forwarded && invariantLog && caught;
}
