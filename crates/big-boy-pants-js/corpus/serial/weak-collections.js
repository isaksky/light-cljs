function weakCollections() {
  // WeakMap and WeakSet (light-cljs-945): new WeakMap(iterable) and new
  // WeakSet(iterable) as Collection native frames over a user iterator
  // whose @@iterator, `next`, `done`, `value` and entries' "0" and "1" run
  // a loop in a frame above them, and a subclass's `set`, so
  // serial_tests.goose stops and resumes them at every budget cut and
  // collects inside the job at every cut. A key the realm's adder cannot
  // hold (a number, a registered symbol) is a TypeError that closes the
  // iterator (its `return` a getter); an array's fast path gives way to
  // the protocol at such a key. Garbage made before the keys and dropped
  // once the collections hold them lets a collection move them: the weak
  // collections keyed by job objects and symbols rebuild their indexes
  // (reclaim.goose rehash_collections) and every lookup still answers.
  // Until light-cljs-951 an entry is kept whatever else holds its key.
  "use strict";
  let log = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; log += tag; };
  let garbage = [];
  for (let i = 0; i < 64; i++) garbage.push({ i });
  const a = {}, b = Symbol("b"), c = [1];
  function iterable(items) {
    let i = 0;
    return {
      [Symbol.iterator]() {
        spin("i");
        return {
          next() { spin("n"); const k = i++; return { get done() { spin("d"); return k >= items.length; }, get value() { spin("v"); return items[k]; } }; },
          get return() { spin("g"); return () => { spin("r"); return {}; }; },
        };
      },
    };
  }
  const entry = (k, v) => ({ get 0() { spin("0"); return k; }, get 1() { spin("1"); return v; } });
  const m = new WeakMap(iterable([entry(a, 1), entry(b, 2), [c, 3], [a, 4]]));
  class W extends WeakMap { set(k, v) { spin("s"); return super.set(k, v * 10); } }
  const n = new W(iterable([[a, 1], [b, 2]]));
  let closed = false;
  try { new WeakMap(iterable([[a, 1], [1, 2]])); } catch (e) { closed = e instanceof TypeError; }
  const s = new WeakSet(iterable([a, b, a]));
  let refused = false;
  try { new WeakSet(iterable([a, Symbol.for("r")])); } catch (e) { refused = e instanceof TypeError; }
  let left = false;
  try { new WeakSet([a, b, Symbol.for("r"), c]); } catch (e) { left = e instanceof TypeError; }
  const f = new WeakMap([[a, "A"], [b, "B"], [c, "C"]]);
  const g = new WeakSet([a, b, c, a]);
  garbage = null;
  const keys = [];
  for (let i = 0; i < 40; i++) keys.push(i % 2 ? { i } : Symbol("k"));
  const big = new WeakMap();
  const bag = new WeakSet();
  for (let i = 0; i < 40; i++) { big.set(keys[i], { i }); bag.add(keys[i]); }
  for (let i = 0; i < 40; i += 3) { big.delete(keys[i]); bag.delete(keys[i]); }
  let more = [];
  for (let i = 0; i < 64; i++) more.push([i]);
  more = null;
  let lookups = true;
  for (let i = 0; i < 40; i++) {
    const v = big.get(keys[i]);
    if (i % 3 === 0 ? v !== undefined || bag.has(keys[i]) : v.i !== i || !bag.has(keys[i])) lookups = false;
  }
  return m.get(a) === 4 && m.get(b) === 2 && m.get(c) === 3 && n.get(a) === 10 && n.get(b) === 20 && closed && refused && left
    && s.has(a) && s.has(b) && !s.has(c) && f.get(a) === "A" && f.get(b) === "B" && f.get(c) === "C" && g.has(c) && !g.has({})
    && lookups && !big.has({}) && log === "indv01ndv01ndvndvndindvsndvsndindvndvgrindvndvndvndindvndvgr";
}
