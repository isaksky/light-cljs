// Cancellation inside direct eval code (light-cljs-858; decision 4): the
// endless loop is the eval code's, and the cleanup runs its finally block,
// then the caller's, innermost first; never a catch block or anything after
// them. serial_tests.goose reads the writes. Never completes on its own.
function directEvalCancel() {
  const o = {};
  globalThis.target = o;
  try { eval("try { for (;;) { o.looped = 1; } } finally { o.inner = 1; }"); } catch (e) { o.caught = 1; } finally { o.outer = 1; }
  o.after = 1;
  return o;
}
