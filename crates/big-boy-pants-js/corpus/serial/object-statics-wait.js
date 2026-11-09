// Dependency waits inside the Statics native frame (light-cljs-884): `dep`
// is a host effect the test answers with "D", in place or after the job's
// whole stack was saved as a continuation and restored in another heap
// (snapshot_tests.goose). Each phase waits once: Object.values's getter
// (pc Value), Object.assign's setter (pc Set), Object.fromEntries's `next`
// (pc Step), an entry's getter while the frame's close handler is held (pc
// First) and its key's conversion (pc Key), Object.groupBy's callback (pc
// Group), an iterator's `return` while the frame closes it (pc Closed, its
// own handler held), a proxy's ownKeys trap (pc List), toString's
// @@toStringTag getter (pc Tag), Object.hasOwn's key conversion (pc Key)
// and toLocaleString's toString (pc Result): 11 waits. Completes with
// "vDsDnDeDkDgDrDoDtDhDlD:1,5,7,2,true,m,[object T],true,L".
function objectStaticsWait(dep) {
  let log = "";
  const values = Object.values({ get a() { log += "v" + dep("940"); return 1; } });
  const target = { set s(v) { log += "s" + dep("941"); this.t = v; } };
  Object.assign(target, { s: 5 });
  function* pairs() { log += "n" + dep("942"); yield { get 0() { log += "e" + dep("943"); return { toString() { log += "k" + dep("944"); return "p"; } }; }, 1: 7 }; }
  const from = Object.fromEntries(pairs());
  const groups = Object.groupBy([2], (v) => { log += "g" + dep("945"); return "g"; });
  let caught = false;
  const failing = { [Symbol.iterator]() { return this; }, next() { return { value: 1 }; }, return() { log += "r" + dep("946"); return {}; } };
  try { Object.fromEntries(failing); } catch (e) { caught = e instanceof TypeError; }
  const p = new Proxy({ m: 1 }, { ownKeys() { log += "o" + dep("947"); return ["m"]; } });
  const keys = Reflect.ownKeys(p).join();
  const tag = Object.prototype.toString.call({ get [Symbol.toStringTag]() { log += "t" + dep("948"); return "T"; } });
  const has = Object.hasOwn({ z: 1 }, { toString() { log += "h" + dep("949"); return "z"; } });
  const locale = ({ toString() { log += "l" + dep("950"); return "L"; } }).toLocaleString();
  return log + ":" + [values.join(), target.t, from.p, groups.g.join(), caught, keys, tag, has, locale].join();
}
