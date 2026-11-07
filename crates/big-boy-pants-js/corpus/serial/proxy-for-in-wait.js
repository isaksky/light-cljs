// Dependency waits inside a proxy's traps during for-in (light-cljs-845;
// the Pants engine's proxy_traps_wait_collect_and_migrate): `dep` is a host
// effect the test answers with "D", in place or after the job's whole stack
// was saved as a continuation and restored in another heap
// (snapshot_tests.goose). Every callback phase waits once: the ownKeys and
// getOwnPropertyDescriptor getters on the handler (the latter once a key),
// both traps, the key list's `length` and element getters, every field
// getter of a data descriptor (`a`: enumerable, configurable, value,
// writable) and an accessor one (`b`: enumerable, configurable, get, set),
// and the getPrototypeOf trap, which ends the walk: 17 waits. Completes with
// "ab:gDkDlDiDhDdDeDcDvDwDhDdDeDcDGDSDpD".
function proxyForInWait(dep) {
  let log = "";
  function pause(s, key) { log += s + dep(key); }
  const handler = {
    get ownKeys() {
      pause("g", "910");
      return function (t) { pause("k", "911"); return { get length() { pause("l", "912"); return 2; }, get 0() { pause("i", "913"); return "a"; }, 1: "b" }; };
    },
    get getOwnPropertyDescriptor() {
      pause("h", "914");
      return function (t, k) {
        pause("d", "915");
        if (k === "a") return { get enumerable() { pause("e", "916"); return true; }, get configurable() { pause("c", "917"); return true; }, get value() { pause("v", "918"); return k; }, get writable() { pause("w", "919"); return true; } };
        return { get enumerable() { pause("e", "920"); return true; }, get configurable() { pause("c", "921"); return true; }, get get() { pause("G", "922"); return undefined; }, get set() { pause("S", "923"); return undefined; } };
      };
    },
    getPrototypeOf(t) { pause("p", "924"); return null; }
  };
  let result = "";
  for (const k in new Proxy({}, handler)) result += k;
  return result + ":" + log;
}
