// bbp-authored conformance case.
// Derives from: none (light-cljs-847: reclamation inside a job while live
// values are held only by a suspended or running generator, a pending
// promise and an async function's activation, a for-of loop over a user
// iterator and a native frame). churn() allocates 60,000 objects and keeps
// one, past half the Goose VM's 100,000-node job limit, so the job reclaims
// its unreachable nodes during every call; each check below reads values
// made before the churn. A wrong promise value rejects `check`'s promise,
// which nothing handles, so the case fails on every engine.
// Completes with "59999|g|g123!|123|10:20".
(function () {
  function churn() { let o; for (let i = 0; i < 60000; i++) o = {i}; return o.i; }
  var log = "";
  // A generator running churn, then suspended across one.
  function* gen() {
    const kept = {v: "g"}; const digits = [1, 2, 3];
    const x = yield churn();
    log += kept.v + digits.join("") + x;
    yield kept;
  }
  const g = gen();
  const first = g.next().value;
  churn();
  const second = g.next("!").value;
  // A promise pending while a promise job churns, with an async function
  // suspended on it holding its own object.
  async function slow() { await null; const before = {n: 41}; churn(); return before.n; }
  async function waiter(p) { const mine = {w: 1}; const v = await p; return mine.w + v; }
  const pending = waiter(slow());
  async function check() { const v = await pending; if (v !== 42) throw "promise " + v; }
  check();
  // A for-of over a user iterator, churning in its body.
  const it = { i: 0, next() { this.i++; return this.i <= 3 ? { value: { n: this.i }, done: false } : { value: undefined, done: true }; } };
  it[Symbol.iterator] = function () { return this; };
  let ns = "";
  for (const item of it) { churn(); ns += item.n; }
  // Array.prototype.map's native frame, its callback churning.
  const mapped = [10, 20].map(function (x) { const box = {x: x}; churn(); return box; });
  return first + "|" + second.v + "|" + log + "|" + ns + "|" + mapped[0].x + ":" + mapped[1].x;
})();
