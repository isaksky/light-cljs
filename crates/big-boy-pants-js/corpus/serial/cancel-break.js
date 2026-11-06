// Cancellation (light-cljs-786): a break out of the finally block a
// cancellation runs would discard it, so it ends the cleanup instead; the
// loop after it never resumes. Never completes on its own.
function cancelBreak() {
  const o = {};
  for (;;) {
    try { for (;;) { o.looped = 1; } } finally { o.inner = 1; break; }
  }
  o.after = 1;
  return o;
}
