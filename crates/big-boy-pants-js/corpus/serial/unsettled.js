// A promise that never settles (light-cljs-790): two async functions
// await each other's promise, and the entry returns one of them. Once the
// promise jobs drain nothing can settle it, so the job fails
// Fault.Unsettled (serial_tests.goose; the reference engines return the
// pending promise).
function unsettled() {
  var pa, pb;
  async function A() { await null; await pb; }
  async function B() { await null; await pa; }
  pa = A(); pb = B();
  return pa;
}
