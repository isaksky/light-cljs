// An async entry (light-cljs-790): the job completes when its promise
// settles, with the promise's value; on the way it awaits a fulfilled
// value and a rejected promise (serial_tests.goose; the reference engines
// return the promise itself).
async function asyncEntry() {
  var a = await "x"; var b = "";
  try { await (async function () { throw "y"; })(); } catch (e) { b = e; }
  return a + b;
}
