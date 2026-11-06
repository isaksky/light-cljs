// Cancellation with continuations (light-cljs-790): mode 0 cancels an
// endless loop inside a running generator while another generator is
// suspended; mode 1 one inside an async function a promise job resumed.
// The running activation's finally block runs (a yield in it abandons it);
// the suspended generator's never does: it is dropped with the job.
// serial_tests.goose reads the writes. Never completes on its own.
function cancelSuspended(mode) {
  const o = {};
  function* suspended() { try { yield 1; } finally { o.dropped = 1; } }
  function* running() { try { yield 1; for (;;) { o.looped = 1; } } finally { o.cleaned = 1; yield 2; o.after = 1; } }
  async function awaiting() { try { await 0; for (;;) { o.looped = 1; } } finally { o.cleaned = 1; await 1; o.after = 1; } }
  const s = suspended(); s.next();
  if (mode === 0) { const r = running(); r.next(); r.next(); } else { awaiting(); }
  return o;
}
