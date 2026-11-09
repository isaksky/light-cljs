// The Object statics that expose own keys (light-cljs-884): Object.values,
// entries and assign read through getters and write through setters that
// each run a loop above their Statics native frame; Object.fromEntries and
// Object.groupBy iterate a generator, convert keys in Convert frames and
// close the iterator on an abrupt completion through the frame's handler;
// a proxy's traps run in Proxy frames above it; toString's @@toStringTag
// getter and toLocaleString's toString run there too. serial_tests.goose
// stops and resumes those frames at every budget cut, and collects inside
// the job at every cut while they hold their key lists, results, sources,
// iterator records and items.
function objectStatics() {
  "use strict";
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const o = { get a() { spin("a"); delete this.c; return 1; }, b: 2, c: 3 };
  const values = Object.values(o).join();
  const entries = Object.entries({ get x() { spin("x"); return 7; }, y: 8 }).join(";");
  const target = { set s(v) { spin("s"); this.t = v; } };
  Object.assign(target, null, { get s() { spin("g"); return 5; } }, "q");
  function* pairs() { spin("1"); yield [{ toString() { spin("k"); return "p"; } }, 1]; spin("2"); yield ["q", 2]; }
  const from = Object.fromEntries(pairs());
  const groups = Object.groupBy([1, 2, 3], (v, i) => { spin("c"); return v % 2 ? "odd" : { toString() { spin("e"); return "even"; } }; });
  let closed = 0, caught = false;
  function* failing() { try { yield 1; } finally { spin("f"); closed++; } }
  try { Object.fromEntries(failing()); } catch (e) { caught = e instanceof TypeError; }
  const p = new Proxy({ m: 1, n: 2 }, { ownKeys() { spin("o"); return ["n", "m"]; }, getOwnPropertyDescriptor(t, k) { spin("d"); return { value: t[k], enumerable: k === "m", configurable: true, writable: true }; } });
  const keys = Object.keys(p).join() + Reflect.ownKeys(p).join();
  const tag = Object.prototype.toString.call({ get [Symbol.toStringTag]() { spin("t"); return "T"; } });
  const locale = ({ toString() { spin("l"); return "L"; } }).toLocaleString();
  const has = Object.hasOwn({ z: 1 }, { toString() { spin("h"); return "z"; } });
  const d = Object.getOwnPropertyDescriptors({ get w() { return 0; }, v: 1 });
  // Object.assign re-reads each key's enumerability; an exception from
  // `next` or `value` leaves the iterator open (no `return`).
  const copied = Object.assign({}, { get a() { spin("r"); Object.defineProperty(this, "b", { enumerable: false }); return 1; }, b: 2 });
  let returns = 0, errors = 0;
  const throwing = (part) => ({ [Symbol.iterator]() { return this; }, next() { if (part === "next") throw part; return part === "value" ? { get value() { throw part; } } : { get done() { throw part; } }; }, return() { returns++; return {}; } });
  for (const part of ["next", "value", "done"]) { try { Object.fromEntries(throwing(part)); } catch (e) { if (e === part) errors++; } }
  return values === "1,2" && entries === "x,7;y,8" && target.t === 5 && target[0] === "q" && from.p === 1 && from.q === 2
    && groups.odd.join() === "1,3" && groups.even.join() === "2" && caught && closed === 1
    && keys === "mn,m" && tag === "[object T]" && locale === "L" && has && typeof d.w.get === "function" && d.v.value === 1
    && copied.a === 1 && !("b" in copied) && returns === 0 && errors === 3
    && order === "axgs1k2ccecfoddotlhr";
}
