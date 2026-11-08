// Cancellation inside code an indirect eval compiled (light-cljs-856;
// decision 4): the endless loop is the eval code's, and the cleanup runs its
// finally block, never the caller's catch block or anything after it.
// serial_tests.goose reads the writes. Never completes on its own.
function cancelEval() {
  const o = {};
  globalThis.target = o;
  try { (0, eval)("try { for (;;) { target.looped = 1; } } finally { target.inner = 1; }"); } catch (e) { o.caught = 1; }
  o.after = 1;
  return o;
}
