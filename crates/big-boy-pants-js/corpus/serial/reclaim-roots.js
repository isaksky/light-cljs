// Reclamation inside a job (light-cljs-847) while live values are held only
// by a running or suspended generator, a pending promise and the async
// functions suspended on it, a for-of loop over a user iterator, and
// Array.prototype.map's native frame. churn() allocates 60,000 objects,
// past half the 100,000-node job limit, so a collection runs during every
// call (serial_tests.goose counts them); each check reads values made
// before the churn. A wrong promise value rejects `check`'s promise, which
// nothing handles. Completes with true (the values otherwise).
function reclaimRoots() {
  function churn() { let o; for (let i = 0; i < 60000; i++) o = {i}; return o.i; }
  var log = "";
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
  async function slow() { await null; const before = {n: 41}; churn(); return before.n; }
  async function waiter(p) { const mine = {w: 1}; const v = await p; return mine.w + v; }
  const pending = waiter(slow());
  async function check() { const v = await pending; if (v !== 42) throw "promise " + v; }
  check();
  const it = { i: 0, next() { this.i++; return this.i <= 3 ? { value: { n: this.i }, done: false } : { value: undefined, done: true }; } };
  it[Symbol.iterator] = function () { return this; };
  let ns = "";
  for (const item of it) { churn(); ns += item.n; }
  const mapped = [10, 20].map(function (x) { const box = {x: x}; churn(); return box; });
  const result = first + "|" + second.v + "|" + log + "|" + ns + "|" + mapped[0].x + ":" + mapped[1].x;
  return result === "59999|g|g123!|123|10:20" || result;
}
