// Adoption follows the visible `then` (light-cljs-790): with
// Promise.prototype.then deleted, an async function returning a promise
// fulfills with the promise itself instead of adopting it (ECMAScript
// 27.2.1.3.2, steps 9 to 13), and awaiting it still waits for it. The
// entry is async, so the job completes with "same7" (serial_tests.goose;
// the Pants engine refuses the delete, and Node returns the entry's
// promise).
async function promiseThen() {
  async function seven() { return "7"; }
  var p = seven();
  delete Object.getPrototypeOf(p).then;
  async function returns() { return p; }
  var x = await returns();
  return (x === p ? "same" : "adopted") + (await x);
}
