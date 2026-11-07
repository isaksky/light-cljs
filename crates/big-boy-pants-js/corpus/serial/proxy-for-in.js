// for-in over proxies whose traps run JavaScript (light-cljs-845; the
// Pants engine's proxy_traps_resume_with_small_budgets_and_gc and
// proxy_own_keys_and_descriptor_traps): handler getters, an ownKeys trap
// that loops before it returns an array-like with a `length` getter and an
// element getter, a getOwnPropertyDescriptor trap whose result's fields are
// getters (read in ToPropertyDescriptor's order, a key reported missing
// after the loop starts is skipped and hides no prototype name, one
// reported non-enumerable hides it), a proxy forwarding to a proxy target,
// typeof, descriptors whose every field is a getter (a callable proxy as
// `get`), Array.prototype.map and Function.prototype.call as traps, and
// TypeErrors from the constructor, a duplicate key and an
// omitted non-configurable key. serial_tests.goose checks it at every
// budget cut and limit point. Completes with true.
function proxyForIn() {
  let log = "";
  let removed = "";
  const handler = {
    get ownKeys() {
      log += "g";
      return function (t) {
        let n = 0; while (n < 5) n++;
        log += "k" + n;
        return { get length() { log += "l"; return 3; }, 0: "a", get 1() { log += "i"; return "b"; }, 2: "x" };
      };
    },
    getOwnPropertyDescriptor(t, k) {
      log += "d" + k;
      if (k === removed) return undefined;
      return { get enumerable() { log += "e"; return k !== "x"; }, configurable: true, get value() { log += "v"; return k; } };
    }
  };
  const p = new Proxy({ __proto__: { x: 1 } }, handler);
  let seen = "";
  for (const k in p) { seen += "[" + k + "]"; if (k === "a") removed = "b"; }
  if (seen !== "[a]" || log !== "gk5lidaevdbdxev") return false;
  // A key reported missing does not hide the prototype's.
  seen = "";
  for (const k in new Proxy({ __proto__: { m: 1 } }, { ownKeys() { return ["m"]; }, getOwnPropertyDescriptor() { return undefined; } })) seen += k;
  if (seen !== "m") return false;
  // Forwarding: no traps over a proxy target with traps.
  let calls = "";
  const inner = new Proxy({ q: 1 }, {
    ownKeys(t) { calls += "k"; return ["q"]; },
    getOwnPropertyDescriptor(t, k) { calls += "d"; return { value: t[k], writable: true, enumerable: true, configurable: true }; }
  });
  seen = "";
  for (const k in new Proxy(new Proxy(inner, {}), {})) seen += k;
  if (seen !== "q" || calls !== "kd") return false;
  if (typeof new Proxy(function () {}, {}) !== "function" || typeof new Proxy([], {}) !== "object") return false;
  // Every descriptor field a getter: `writable` of a data descriptor, `get`
  // (a callable proxy, never called) and `set` of an accessor one.
  log = "";
  const fields = {
    ownKeys() { return ["d", "e"]; },
    getOwnPropertyDescriptor(t, k) {
      if (k === "d") return { get enumerable() { log += "E"; return true; }, get configurable() { log += "C"; return true; }, get value() { log += "V"; return 1; }, get writable() { log += "W"; return true; } };
      return { get enumerable() { log += "e"; return true; }, get configurable() { log += "c"; return true; }, get get() { log += "g"; return new Proxy(function () { log += "!"; }, {}); }, get set() { log += "s"; return undefined; } };
    }
  };
  seen = "";
  for (const k in new Proxy({}, fields)) seen += k;
  if (seen !== "de" || log !== "ECVWecgs") return false;
  // Built-ins as traps: Array.prototype.map maps the handler, an
  // array-like, through the target, and Function.prototype.call calls its
  // receiver, a function handler, with the target as `this`.
  const mapped = { 0: 0, length: 1, ownKeys: Array.prototype.map, getOwnPropertyDescriptor() { return { enumerable: true, configurable: true }; } };
  seen = "";
  for (const k in new Proxy(() => "m", mapped)) seen += k;
  if (seen !== "m") return false;
  const called = function () { return [this === called ? "?" : "c"]; };
  called.ownKeys = Function.prototype.call;
  called.getOwnPropertyDescriptor = function () { return { enumerable: true, configurable: true }; };
  seen = "";
  for (const k in new Proxy({}, called)) seen += k;
  if (seen !== "c") return false;
  let errors = 0;
  const failing = [
    () => Proxy({}, {}),
    () => new Proxy({}, 1),
    () => { for (const k in new Proxy({}, { ownKeys() { return ["a", "a"]; } })) {} },
    () => { for (const k in new Proxy([], { ownKeys() { return ["0"]; } })) {} },
  ];
  for (const f of failing) { try { f(); } catch (e) { if (e instanceof TypeError) errors++; } }
  return errors === 4;
}
