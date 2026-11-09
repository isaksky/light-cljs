function proxyPropertyWait(dep) {
  "use strict";
  // A proxy's [[Get]], [[Set]], [[HasProperty]] and [[Delete]]
  // (light-cljs-890) across dependency waits: `dep` is a host effect the
  // test answers with "D", in place after a forced collection or after the
  // job's whole stack was collected, saved as a continuation and restored
  // in another heap (snapshot_tests.goose). Each wait is above the Proxy
  // frame of the method at one of its phases: the `get` trap read through
  // a handler getter (g) and the trap with its receiver (t); a has trap
  // (h); a target's getter (G) and setter (Z) forwarded from a proxy on an
  // object's prototype chain, with that object as receiver; a set trap
  // given its value and receiver in an assignment (s, its false answer a
  // TypeError in this strict code) and in Reflect.set (S, false); a
  // deleteProperty trap in `delete` (x, a TypeError) and in
  // Reflect.deleteProperty (X, false); a proxy target's
  // getOwnPropertyDescriptor checking a get (i) and a set trap's (I)
  // result; a proxy target's isExtensible checking a has (e) and a
  // deleteProperty trap's (E) answer; a trap reached by restarting a
  // forwarded [[Get]] on a proxy of the target's prototype chain (r); and
  // both conversions of an array's new `length` under a forwarded [[Set]]
  // (n, twice). 16 waits; completes with
  // "gDtDhDGDZDsDSDxDXDiDIDeDEDrDnDnD:2,true,true,zz,5,true,true,false,true,false,true,false,3,true,true,true,true,true,1".
  let log = "";
  const target = {
    x: 1,
    get y() { log += "G" + dep(); return this === child; },
    set z(v) { log += "Z" + dep(); this.zz = v; },
  };
  const p = new Proxy(target, {
    get get() { log += "g" + dep(); return function (t, k, r) { log += "t" + dep(); return r === p ? t[k] + 1 : 0; }; },
    has(t, k) { log += "h" + dep(); return k === "virtual"; },
  });
  const a = p.x;
  const b = "virtual" in p;
  const child = Object.create(new Proxy(target, {}));
  const c = child.y;
  child.z = 5;
  let setMark = "s"; let seen = 0;
  const refusing = new Proxy({}, { set(t, k, v, r) { log += setMark + dep(); if (v === 7 && r === heir) { seen += 1; } return false; } });
  const heir = Object.create(refusing);
  let strictSet = false;
  try { heir.w = 7; } catch (e) { strictSet = e instanceof TypeError; }
  setMark = "S";
  const reflectSet = Reflect.set(heir, "w", 7);
  let deleteMark = "x";
  const keeping = new Proxy({ q: 1 }, { deleteProperty(t, k) { log += deleteMark + dep(); return false; } });
  let strictDelete = false;
  try { delete keeping.q; } catch (e) { strictDelete = e instanceof TypeError; }
  deleteMark = "X";
  const reflectDelete = Reflect.deleteProperty(keeping, "q");
  const fixed = {};
  Object.defineProperty(fixed, "f", { value: 3 });
  let describeMark = "i";
  const inner = new Proxy(fixed, { getOwnPropertyDescriptor(t, k) { log += describeMark + dep(); return Reflect.getOwnPropertyDescriptor(t, k); } });
  const d = new Proxy(inner, { get() { return 3; } }).f;
  describeMark = "I";
  const set = Reflect.set(new Proxy(inner, { set() { return true; } }), "f", 3);
  let extensibleMark = "e";
  const ext = new Proxy({ e: 1 }, { isExtensible(t) { log += extensibleMark + dep(); return Reflect.isExtensible(t); } });
  const hidden = !("e" in new Proxy(ext, { has() { return false; } }));
  extensibleMark = "E";
  const gone = delete new Proxy(ext, { deleteProperty() { return true; } }).e;
  const far = new Proxy({}, { get(t, k, r) { log += "r" + dep(); return r === near; } });
  const near = new Proxy(Object.create(far), {});
  const restarted = near.k;
  const array = [1, 2, 3];
  const length = Reflect.set(new Proxy(array, {}), "length", { valueOf() { log += "n" + dep(); return 1; } }, array);
  return log + ":" + [a, b, c, Object.keys(child).join(), child.zz, seen === 2, strictSet, reflectSet, strictDelete, reflectDelete, "q" in keeping, "zz" in target, d, set, hidden, gone, restarted, length, array.length].join();
}
