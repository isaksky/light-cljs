// Object.seal, freeze, isSealed and isFrozen of a proxy and a proxy target
// in the ownKeys trap's invariants (light-cljs-897): ownKeys and
// getOwnPropertyDescriptor traps (the latter read through a handler
// getter), symbol keys, trapless forwarding through a nested proxy to an
// array, and an inner proxy's isExtensible (through a handler getter),
// ownKeys and getOwnPropertyDescriptor deciding the outer trap's
// invariants: each a Proxy frame above an integrity built-in's Statics
// frame or an [[OwnPropertyKeys]] frame, which serial_tests.goose runs at
// every budget cut and collection.
function proxyIntegrity() {
  let log = "";
  const s = Symbol("s");
  const t = { a: 1, [s]: 2, get g() { return 3; } };
  const p = new Proxy(t, {
    ownKeys(t) { log += "k"; return Reflect.ownKeys(t); },
    get getOwnPropertyDescriptor() { log += "h"; return function (t, k) { log += "d"; return Object.getOwnPropertyDescriptor(t, k); }; },
  });
  const sealed = Object.seal(p) === p && Object.isSealed(p) && !Object.isFrozen(p) && Object.isSealed(t);
  const a = [1, 2, 3];
  const frozen = Object.isFrozen(Object.freeze(new Proxy(new Proxy(a, {}), {}))) && Object.isFrozen(a);
  const target = { x: 1 };
  const inner = new Proxy(target, {
    get isExtensible() { log += "e"; return function (t) { log += "E"; return Object.isExtensible(t); }; },
    ownKeys(t) { log += "K"; return ["x"]; },
    getOwnPropertyDescriptor(t, k) { log += "D"; return Object.getOwnPropertyDescriptor(t, k); },
  });
  const keys = Reflect.ownKeys(new Proxy(inner, { ownKeys() { return ["x", "y"]; } })).join();
  Object.preventExtensions(target);
  let caught = false;
  try { Reflect.ownKeys(new Proxy(inner, { ownKeys() { return ["x", "y"]; } })); } catch (e) { caught = e instanceof TypeError; }
  return sealed && frozen && keys === "x,y" && caught && log === "kkhdhdhdkhdeEKDeEKD";
}
