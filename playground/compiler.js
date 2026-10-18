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
// `spawn` makes a worker-like object (postMessage, terminate, onmessage,
// onerror) running compiler-worker.js; `module` is the compiled
// WebAssembly.Module. check.mjs passes a Node worker_threads adapter.
export function createCompiler({ spawn, module }) {
  let worker = null;
  let latest = 0;
  // Request id -> resolve.
  const pending = new Map();
  let announce;
  const ready = new Promise((resolve, reject) => (announce = { resolve, reject }));
  // A caller that never awaits it must not see an unhandled rejection.
  ready.catch(() => {});

  function settle(id, outcome) {
    const resolve = pending.get(id);
    if (!resolve) return;
    pending.delete(id);
    resolve(id === latest ? outcome : { status: "stale" });
  }

  function settleAll(outcome) {
    for (const id of [...pending.keys()]) settle(id, outcome);
  }

  function stop() {
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

  function start() {
    worker = spawn();
    worker.onmessage = (event) => {
      const message = event.data;
      if (message.type === "ready") announce.resolve(message.version);
      else if (message.type === "compiled") settle(message.id, { status: "done", result: message.result });
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
        pending.set(id, resolve);
        worker.postMessage({ type: "compile", id, ...request });
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
