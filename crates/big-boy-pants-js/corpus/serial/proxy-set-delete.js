// A proxy's [[Set]] and [[Delete]] (light-cljs-890): set traps given their
// value and receiver, read through a handler getter, in an assignment (a
// false answer a TypeError in this strict code) and in Reflect.set (false);
// a target's setter forwarded from a proxy on an object's prototype chain;
// a forwarded [[Set]] restarting on a proxy of the target's prototype
// chain; an array's new `length` converted under a forwarded [[Set]];
// deleteProperty traps in `delete` and Reflect.deleteProperty; and the
// invariants, a proxy target's getOwnPropertyDescriptor and isExtensible
// deciding them: each a Proxy frame above the assignment, Reflect.set,
// `delete` or Reflect.deleteProperty, which serial_tests.goose runs at
// every budget cut and collection.
function proxySetDelete() {
  "use strict";
  let log = "";
  const target = { set z(v) { log += "Z"; this.zz = [v, this === heir]; } };
  let seen = [];
  const p = new Proxy(target, {
    get set() { log += "g"; return function (t, k, v, r) { log += "s"; seen.push(k + ":" + v + ":" + (r === heir || r === p)); return k !== "refused"; }; },
  });
  const heir = Object.create(p);
  heir.a = 1;
  p.b = 2;
  let strict = false;
  try { heir.refused = 3; } catch (e) { strict = e instanceof TypeError; }
  const reflected = Reflect.set(heir, "refused", 4);
  const forwarded = Object.create(new Proxy(target, {}));
  forwarded.z = 5;
  const far = new Proxy({}, { set(t, k, v, r) { log += "f"; return Reflect.set(t, k, v + 1, r); } });
  const near = new Proxy(Object.create(far), {});
  near.n = 6;
  const array = [1, 2, 3];
  const length = Reflect.set(new Proxy(array, {}), "length", { valueOf() { log += "v"; return 1; } }, array);
  const keeping = new Proxy({ q: 1 }, { deleteProperty(t, k) { log += "d"; return k !== "q" && Reflect.deleteProperty(t, k); } });
  let strictDelete = false;
  try { delete keeping.q; } catch (e) { strictDelete = e instanceof TypeError; }
  const reflectDelete = Reflect.deleteProperty(keeping, "q");
  const fixed = {};
  Object.defineProperty(fixed, "f", { value: 3 });
  const inner = new Proxy(fixed, { getOwnPropertyDescriptor(t, k) { log += "i"; return Reflect.getOwnPropertyDescriptor(t, k); } });
  const kept = Reflect.set(new Proxy(inner, { set() { return true; } }), "f", 3);
  let changed = false;
  try { Reflect.set(new Proxy(inner, { set() { return true; } }), "f", 4); } catch (e) { changed = e instanceof TypeError; }
  let undeletable = false;
  try { delete new Proxy(inner, { deleteProperty() { return true; } }).f; } catch (e) { undeletable = e instanceof TypeError; }
  const ext = new Proxy({ e: 1 }, { isExtensible(t) { log += "e"; return Reflect.isExtensible(t); } });
  const gone = delete new Proxy(ext, { deleteProperty() { return true; } }).e;
  Object.preventExtensions(ext);
  let frozen = false;
  try { delete new Proxy(ext, { deleteProperty() { return true; } }).e; } catch (e) { frozen = e instanceof TypeError; }
  return seen.join() === "a:1:true,b:2:true,refused:3:true,refused:4:true" && strict && !reflected && forwarded.zz.join() === "5,false" && !("zz" in target)
    && near.n === 7 && !("n" in far) && length && array.length === 1 && strictDelete && !reflectDelete && keeping.q === 1
    && kept && changed && undeletable && gone && frozen && log === "gsgsgsgsZfvvddiiiee";
}
