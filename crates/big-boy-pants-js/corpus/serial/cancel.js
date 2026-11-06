// Cancellation (light-cljs-786): an endless loop inside nested try blocks.
// Cancelled, it runs both finally blocks, innermost first, and not the catch
// block; serial_tests.goose reads the writes. Never completes on its own.
function cancelled() {
  const o = {};
  try {
    try { for (;;) { o.looped = 1; } } catch (e) { o.caught = 1; } finally { o.inner = 1; }
  } finally { o.outer = 1; }
  return o;
}
