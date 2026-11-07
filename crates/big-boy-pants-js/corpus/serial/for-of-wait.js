// Dependency waits inside for-of (light-cljs-843; the Pants engine's
// for_of_waits_resume_iteration_and_close and
// for_of_get_iterator_waits_resume_once): `dep` is a host effect the test
// answers with "D", in place or after the job's whole stack was saved as a
// continuation and restored in another heap (snapshot_tests.goose). It waits
// in the user @@iterator method and in a `next` getter (each above
// GetIterator's native frame), in `next`, in the body and in `return`; the
// loop resumes where it waited and `return` runs exactly once. Completes
// with "iDnDbDRDgD12".
function forOfWait(dep) {
  let log = "";
  const it = {
    n: 0,
    next() { this.n++; if (this.n === 1) { log += "n" + dep("next"); } return { value: this.n, done: this.n > 3 }; },
    return() { log += "R" + dep("return"); return {}; }
  };
  it[Symbol.iterator] = function () { log += "i" + dep("method"); return this; };
  for (const x of it) { if (x === 2) { log += "b" + dep("body"); break; } }
  const lazy = { get next() { log += "g" + dep("getter"); let k = 0; return function () { k++; return { value: k, done: k > 2 }; }; } };
  lazy[Symbol.iterator] = function () { return this; };
  for (const y of lazy) { log += y; }
  return log;
}
