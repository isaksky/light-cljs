function mapSetConstruction() {
  // Map and Set (light-cljs-943): new Map(iterable) and new Set(iterable)
  // as Collection native frames over a user iterator whose @@iterator,
  // `next`, `done`, `value` and entries' "0" and "1" run a loop in a frame
  // above them (its `return` too, a getter), and a subclass's `set`, so
  // serial_tests.goose stops and resumes them at every budget cut, and
  // collects inside the job at every cut while the frames hold the
  // iterator record, the result and the entry they read. Garbage made
  // before the keys and dropped once the maps hold them lets a collection
  // move them: the maps keyed by job objects and symbols rebuild their
  // indexes (reclaim.goose rehash_collections) and every lookup still
  // answers. A non-object entry is a TypeError that closes the iterator;
  // arrays take the fast path, a hole leaving it.
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
  const m = new Map(iterable([entry(a, 1), entry(b, 2), [c, 3], [a, 4]]));
  class M extends Map { set(k, v) { spin("s"); return super.set(k, v * 10); } }
  const n = new M(iterable([[a, 1], [b, 2]]));
  let closed = false;
  try { new Map(iterable([[1, 1], 2])); } catch (e) { closed = e instanceof TypeError; }
  const s = new Set(iterable([a, b, a, "x"]));
  const f = new Map([[a, "A"], [b, "B"], [c, "C"]]);
  const g = new Set([a, b, c, a, , b]);
  garbage = null;
  const keys = [];
  for (let i = 0; i < 40; i++) keys.push(i % 2 ? { i } : Symbol("k"));
  const big = new Map();
  for (let i = 0; i < 40; i++) big.set(keys[i], i);
  for (let i = 0; i < 40; i += 3) big.delete(keys[i]);
  let more = [];
  for (let i = 0; i < 64; i++) more.push([i]);
  more = null;
  let lookups = true;
  for (let i = 0; i < 40; i++) { if (big.get(keys[i]) !== (i % 3 === 0 ? undefined : i)) lookups = false; }
  return m.size === 3 && m.get(a) === 4 && m.get(b) === 2 && m.get(c) === 3 && n.get(a) === 10 && n.get(b) === 20 && n.size === 2 && closed
    && s.size === 3 && s.has(a) && s.has(b) && s.has("x") && f.get(a) === "A" && f.get(b) === "B" && f.get(c) === "C" && g.size === 4 && g.has(undefined)
    && lookups && big.size === 26 && log === "indv01ndv01ndvndvndindvsndvsndindvndvgrindvndvndvndvnd";
}
