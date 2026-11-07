// Cancellation inside a for-of loop (light-cljs-843; the Pants engine's
// for_of_cancellation_closes_body_and_pending_next): mode 0 cancels an
// endless loop in the body, mode 1 one inside `next`, mode 2 one in the body
// with a `return` that throws. The cleanup runs the body's finally block
// (modes 0 and 2), then IteratorClose's `return` once; the catch block
// around the loop never runs and nothing after it; an exception from
// `return` ends the cleanup, still cancelled. serial_tests.goose reads the
// writes. Never completes on its own.
function cancelForOf(mode) {
  const o = {};
  const it = {
    next() { if (mode === 1) { for (;;) { o.looped = 1; } } return { value: 1, done: false }; },
    return() { o.closed = 1; if (mode === 2) { throw 42; } return {}; }
  };
  it[Symbol.iterator] = function () { return this; };
  try {
    for (const x of it) { try { for (;;) { o.looped = 1; } } finally { o.inner = 1; } }
  } catch (e) { o.caught = 1; }
  o.after = 1;
  return o;
}
