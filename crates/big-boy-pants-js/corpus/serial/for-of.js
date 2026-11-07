// for-of (light-cljs-843): GetIterator over user iterables and generators,
// `next` and `return` called as methods, RequireObject on their results.
// A user @@iterator method and a `next` getter run above GetIterator's
// native frame; `next` runs n + 1 times; `return` exactly once on `break`,
// a `return` from the function and a `throw` from the body, never on
// exhaustion or `continue`; a `return` that throws after `break` replaces
// it; a member target is evaluated each iteration; a generator suspended in
// try/finally runs its finally block when the loop breaks; per-iteration
// `let` bindings; acquisition failures (a primitive from the method, a
// method or `next` getter that throws) and Function.prototype.call as the
// method. serial_tests.goose checks it at every budget cut, and
// with setup 7 also runs `hooked`, an object it installs whose
// Symbol.iterator is an accessor with hookGet as its getter (no computed
// accessor names on Goose yet). Completes with true.
function forOf() {
  function hookGet() { this.reads = this.reads + 1; return this.method; }
  let log = "";
  function counting(limit) {
    const it = {
      n: 0,
      next() { this.n++; log += "n"; return { value: this.n, done: this.n > limit }; },
      return() { log += "R"; return {}; }
    };
    it[Symbol.iterator] = function () { log += "i"; return it; };
    return it;
  }
  let sum = 0;
  for (const x of counting(3)) { if (x === 2) continue; sum += x; }
  const exhausted = log === "innnn" && sum === 4;
  log = "";
  for (const x of counting(3)) { if (x === 2) break; }
  const broke = log === "innR";
  log = "";
  function early() { for (const x of counting(3)) { return x * 10; } return 0; }
  const returned = early() === 10 && log === "inR";
  log = "";
  let caught;
  try { for (const x of counting(3)) { throw "body"; } } catch (e) { caught = e; }
  const thrown = caught === "body" && log === "inR";
  // A `return` that throws after `break` replaces it.
  const bad = { next() { return { value: 1, done: false }; }, return() { throw "close"; } };
  bad[Symbol.iterator] = function () { return this; };
  let replaced;
  try { for (const x of bad) break; } catch (e) { replaced = e; }
  // `next` through a getter, read once.
  let gets = 0, total = 0;
  const lazy = { get next() { gets++; let k = 0; return function () { k++; return { value: k, done: k > 2 }; }; } };
  lazy[Symbol.iterator] = function () { return this; };
  for (const x of lazy) total += x;
  // Acquisition failures: a primitive from the method, a method that throws
  // and a `next` getter that throws; the body never runs, nothing closes.
  let failures = "";
  function attempt(method) {
    const o = {};
    o[Symbol.iterator] = method;
    try { for (const x of o) failures += "body"; } catch (e) { failures += (e instanceof TypeError ? "T" : e) + ";"; }
  }
  attempt(function () { return 1; });
  attempt(function () { throw "m"; });
  attempt(function () { return { get next() { throw "g"; }, return() { failures += "R"; return {}; } }; });
  // Function.prototype.call as the method calls the iterable itself.
  function callable() { let k = 0; return { next() { k++; return { value: k, done: k > 2 }; } }; }
  callable[Symbol.iterator] = Function.prototype.call;
  let called = 0;
  for (const x of callable) called += x;
  // A member target, evaluated each iteration.
  const target = { x: 0 }; let writes = "";
  for (target.x of counting(2)) writes += target.x;
  // Generators.
  let fin = "";
  function* g() { try { yield 1; yield 2; yield 3; } finally { fin += "F"; } }
  let gsum = 0;
  for (const x of g()) { gsum += x; if (x === 2) break; }
  const fs = [];
  for (let x of g()) fs.push(() => x);
  const finished = fin === "FF";
  // The hooked iterable (setup 7): its @@iterator getter runs as a frame.
  let hooked = true;
  const h = globalThis.hooked;
  if (h !== undefined) {
    h.reads = 0; h.method = function () { return g(); };
    let hs = 0; for (const x of h) hs += x;
    hooked = h.reads === 1 && hs === 6;
  }
  return exhausted && broke && returned && thrown && replaced === "close" && gets === 1 && total === 3 && failures === "T;m;g;" && called === 3 &&
    writes === "12" && target.x === 2 && gsum === 3 && finished && fs[0]() === 1 && fs[2]() === 3 && hooked;
}
