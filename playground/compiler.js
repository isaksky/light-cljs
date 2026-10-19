// The page's handle on compiler-worker.js. `compile(request)` resolves to
//   { status: "stale" }            a later compile was requested meanwhile
// or, for the newest request, to
//   { status: "done", result }     its CompileResult fields
//   { status: "cancelled" }        `cancel` stopped it
//   { status: "failed", error }    the worker failed to load or crashed
// so a caller renders only the newest outcome.`cancel` terminates a busy worker and
// starts a fresh one from the same compiled module; that costs one
// instantiation, and the next compile redoes the macro bootstrap.
//
// Every compile also has a watchdog (light-cljs-356, docs/browser-macros.md):
// macros run in the browser's engine, which does not check the macro
// session's 10 s deadline while JavaScript runs. A compile that has run
// DEADLINE_MS with a macro host call still open can only end with the
// deadline, which the compiler checks when the call returns, so the watchdog
// terminates its worker. To give the located diagnostic that an engine
// checking the deadline itself gives (the native compiler's QuickJS), it
// replays the request in a fresh worker with `stopAt` naming that call,
// which then fails at once; the worker checks that the replay reached it by
// the same path (`fingerprint`).
// The worker reports calls only after a compile's first 500 ms, so for a call
// open since then the watchdog first replays the request as a probe that
// reports every call (`quiet: 0`) and takes the call it keeps open for
// PROBE_QUIET_MS. A replay redoes the macros before that call, so a call that
// started more than REPLAY_LIMIT_MS into its run is not replayed, and every
// outcome comes within ALLOWANCE_MS of the deadline: a probe or replay that
// would take longer, takes another path or does not reach such a call (a
// macro that reads the clock, say) gives the deadline diagnostic without a
// location. `millis` counts from the first start. With no host call open, the
// compiler's own code is running, and it reports the deadline at its next
// call, so the watchdog waits. A worker that does not start a compile within
// DEADLINE_MS, because something a macro left behind keeps it busy, is
// replaced and the newest request sent again.
//
// `spawn` makes a worker-like object (postMessage, terminate, onmessage,
// onerror) running compiler-worker.js; `module` is the compiled
// WebAssembly.Module. check.mjs passes a Node worker_threads adapter.
export const DEADLINE_MS = 10_000;
// The compiler starts its clock after the worker posts "started".
const GRACE_MS = 250;
export const ALLOWANCE_MS = 3000;
// Twice the worker's QUIET_MS: every call the probe makes before the stuck
// one returned within the original compile's first 500 ms.
const PROBE_QUIET_MS = 1000;
const REPLAY_LIMIT_MS = 1500;
// The evaluator's text for an expired session (src/evaluator.rs).
const DEADLINE = "JavaScript session deadline exceeded";

export function createCompiler({ spawn, module }) {
  let worker = null;
  let latest = 0;
  // Request id -> { resolve, request, at, phase, call, fingerprint }: `at` is
  // when it first started; `phase` is null, "probe" or "replay" (of host
  // call `call`).
  const pending = new Map();
  // The compile the worker runs: its id, when this run started, the
  // innermost open host call, whether the worker has said which that is, the
  // calls it reported starting (call -> { trace, time }), and whether it is
  // past the deadline.
  let running = null;
  let timer = null;
  let bound = null;
  let announce;
  const ready = new Promise((resolve, reject) => (announce = { resolve, reject }));
  // A caller that never awaits it must not see an unhandled rejection.
  ready.catch(() => {});

  function settle(id, outcome) {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    entry.resolve(id === latest ? outcome : { status: "stale" });
  }

  function settleAll(outcome) {
    for (const id of [...pending.keys()]) settle(id, outcome);
  }

  function arm(delay, fire) {
    clearTimeout(timer);
    timer = setTimeout(fire, delay);
  }

  // Until a compile starts, the worker may be busy with something else.
  function awaitStart() {
    clearTimeout(timer);
    if (pending.size) arm(DEADLINE_MS, restart);
  }

  function stop() {
    clearTimeout(timer);
    clearTimeout(bound);
    running = null;
    if (!worker) return;
    worker.onmessage = worker.onerror = null;
    worker.terminate();
    worker = null;
  }

  // A worker that failed is dropped; the next compile starts another.
  function fail(error) {
    stop();
    announce.reject(new Error(error));
    settleAll({ status: "failed", error });
  }

  function post(id) {
    const { request, phase, call, fingerprint } = pending.get(id);
    if (!running) awaitStart();
    const mode = phase === "probe" ? { quiet: 0 } : phase === "replay" ? { stopAt: call, fingerprint } : {};
    worker.postMessage({ type: "compile", id, ...request, ...mode });
  }

  // A fresh worker for the newest request; the older ones are stale.
  function restart() {
    stop();
    for (const id of [...pending.keys()]) if (id !== latest) settle(id, { status: "stale" });
    start();
    if (pending.has(latest)) post(latest);
  }

  function deadline(id) {
    const result = { js: null, error: DEADLINE, error_file: null, error_line: 0, error_column: 0, warnings: "", size_report: "" };
    return { status: "done", result: { ...result, millis: performance.now() - pending.get(id).at } };
  }

  // The running compile ends with the deadline, without a location.
  function giveUp() {
    if (running.id !== latest) return restart();
    const outcome = deadline(latest);
    stop();
    settle(latest, outcome);
    start();
  }

  // Runs the running compile again in a fresh worker as `phase`.
  function escalate(phase, call = null, fingerprint = null) {
    const entry = pending.get(running.id);
    if (running.id === latest) Object.assign(entry, { phase, call, fingerprint });
    restart();
  }

  // Past the deadline with host call `call` open.
  function stuck(call) {
    const start = running.entered.get(call);
    if (!start) {
      if (pending.get(running.id).phase) giveUp();
      else escalate("probe");
    } else if (start.time - running.since > REPLAY_LIMIT_MS) giveUp();
    else escalate("replay", call, start.trace);
  }

  function expired() {
    running.expired = true;
    if (!running.known) escalate("probe");
    else if (running.open != null) stuck(running.open);
  }

  function quiet() {
    if (running.open != null) stuck(running.open);
    else giveUp();
  }

  function opened(call) {
    if (!running) return;
    running.open = call;
    running.known = true;
    if (pending.get(running.id)?.phase === "probe") arm(PROBE_QUIET_MS, quiet);
    else if (running.expired && call != null) stuck(call);
  }

  function started(id) {
    const entry = pending.get(id);
    clearTimeout(timer);
    clearTimeout(bound);
    if (!entry) return;
    const now = performance.now();
    entry.at ??= now;
    running = { id, since: now, open: null, known: false, entered: new Map(), expired: false };
    if (!entry.phase) return arm(DEADLINE_MS + GRACE_MS, expired);
    bound = setTimeout(giveUp, Math.max(0, entry.at + DEADLINE_MS + GRACE_MS + ALLOWANCE_MS - now));
    if (entry.phase === "probe") arm(PROBE_QUIET_MS, quiet);
  }

  function compiled(id, result, stopped) {
    clearTimeout(bound);
    running = null;
    const entry = pending.get(id);
    if (!entry?.phase) settle(id, { status: "done", result });
    else if (entry.phase === "replay" && stopped) settle(id, { status: "done", result: { ...result, millis: performance.now() - entry.at } });
    else settle(id, deadline(id));
    awaitStart();
  }

  function start() {
    worker = spawn();
    worker.onmessage = (event) => {
      const message = event.data;
      if (Array.isArray(message)) {
        const [call, trace] = message;
        running?.entered.set(call, { trace, time: performance.now() });
        opened(call);
      } else if (message.type === "open") opened(message.call);
      else if (message.type === "started") started(message.id);
      else if (message.type === "compiled") compiled(message.id, message.result, message.stopped);
      else if (message.type === "ready") announce.resolve(message.version);
      else if (message.type === "failed") fail(message.error);
    };
    worker.onerror = (event) => fail(event.message || String(event));
    worker.postMessage({ type: "init", module });
  }

  start();
  return {
    // Resolves to the compiler's version once the first worker has loaded.
    ready,
    get busy() {
      return pending.size > 0;
    },
    compile(request) {
      if (!worker) start();
      const id = ++latest;
      return new Promise((resolve) => {
        pending.set(id, { resolve, request, at: null, phase: null, call: null, fingerprint: null });
        post(id);
      });
    },
    // Stops every pending compile; true when there was one.
    cancel() {
      if (!pending.size) return false;
      stop();
      settleAll({ status: "cancelled" });
      start();
      return true;
    },
  };
}
