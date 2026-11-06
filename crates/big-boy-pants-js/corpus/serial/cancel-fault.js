// Cancellation (light-cljs-786): a TypeError in the finally block a
// cancellation runs would discard it, so it ends the cleanup: the outer
// finally block does not run and the job still ends cancelled. Never
// completes on its own.
function cancelFault() {
  const o = {};
  const n = null;
  try {
    try { for (;;) { o.looped = 1; } } finally { o.inner = 1; o.x = n.x; }
  } finally { o.outer = 1; }
  return o;
}
