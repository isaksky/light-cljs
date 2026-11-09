// Math.random's stream continues across the job's promise jobs
// (light-cljs-901): the entry draws a and b, an async function's
// continuation draws c in a promise job, and the entry draws d after its
// await. The values are the Pants engine's stream from its fixed seed, the
// first four of every job. The entry is async, so the job completes with
// true (serial_tests.goose, at every budget cut and collection; the
// reference engines return the entry's promise).
async function mathRandom() {
  const a = Math.random();
  async function later() { await null; return Math.random(); }
  const p = later();
  const b = Math.random();
  const c = await p;
  const d = Math.random();
  return [a, b, c, d].join() === "0.8833108082136426,0.43152799704850997,0.026433771592597743,0.9708819781538285";
}
