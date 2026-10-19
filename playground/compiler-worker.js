// Runs the WebAssembly compiler off the page's thread, so the editor stays
// responsive while a compile runs and the page can stop one by terminating
// this worker (compiler.js then starts a fresh one). The page compiles the
// wasm module once and sends it in the "init" message; each "compile" message
// is answered by one "started" and then one "compiled" message with the same
// id, in order.
//
// A request with `macros: "browser"` runs its macros in this worker's own
// JavaScript engine through macro-host.js instead of in the module's QuickJS
// (light-cljs-307, docs/browser-macros.md). The 10 s session deadline is not
// checked while JavaScript runs there, so compiler.js keeps a watchdog
// (light-cljs-356), and this worker tells it which macro host call is open.
// It numbers a compile's host calls from 0 and, once the compile has run
// `quiet` milliseconds (the request's, QUIET_MS by default), posts
// `[N, trace]` when call N starts, `trace` being a hash of every call so far,
// and `{ type: "open", call }` when control returns to the innermost open
// call, or to none (`call: null`), after a host call or a compiler
// service. A post costs 7 to 27 µs in a browser, and
// a compile makes thousands of host calls, so one that ends sooner posts
// nothing. A request with `stopAt: N` and `fingerprint` replays a stopped
// compile: host call N fails with the deadline's own diagnostic instead of
// running, so the compiler reports it where the embedded engine would, and
// "compiled" says `stopped: true` when the trace there was `fingerprint`.
import init, { compile, serve_macro_host, set_browser_macros, version } from "./pkg/light_cljs_playground.js";
import { createMacroHost } from "./macro-host.js";

// The evaluator's text for an expired session (src/evaluator.rs).
const DEADLINE = "JavaScript session deadline exceeded";
const QUIET_MS = 500;

// FNV-1a offset basis: the trace of a compile before its first host call.
const TRACE = 0x811c9dc5;

let ready = null;
// The current compile's host calls: how many started, the open ones
// innermost last, when it starts posting, its trace, and where its replay
// stops.
let calls = 0;
const open = [];
let posting = 0;
let trace = TRACE;
let stopAt = null;
let fingerprint = null;
let stopped = false;
// The trace folds in every host call's service and its arguments (only
// their length when long; a call's are value IDs), so a replay can tell
// that it took the same path to call N as the run it replays.
function mix(text) {
  for (let i = 0; i < text.length; i++) trace = Math.imul(trace ^ text.charCodeAt(i), 16777619);
  trace >>>= 0;
}
function report() {
  if (performance.now() >= posting) postMessage({ type: "open", call: open.length ? open[open.length - 1] : null });
}
const host = createMacroHost((...request) => {
  try {
    return serve_macro_host(...request);
  } finally {
    report();
  }
});
globalThis.lightCljsMacroHost = {
  ...host,
  call(session, name, args) {
    const index = calls++;
    mix(name);
    mix(args.length > 64 ? String(args.length) : args);
    if (index === stopAt) {
      // A replay that took another path stops too, without claiming it.
      stopped = trace === fingerprint;
      throw DEADLINE;
    }
    if (performance.now() >= posting) postMessage([index, trace]);
    open.push(index);
    try {
      return host.call(session, name, args);
    } finally {
      open.pop();
      report();
    }
  },
};

// The fields of a CompileResult the page reads, as a plain object: the
// wasm-bindgen class does not survive postMessage.
function plain(result) {
  return {
    js: result.js ?? null,
    error: result.error ?? null,
    error_file: result.error_file ?? null,
    error_line: result.error_line,
    error_column: result.error_column,
    warnings: result.warnings,
    size_report: result.size_report,
    millis: result.millis,
  };
}

async function receive(message) {
  if (message.type === "init") {
    ready = init({ module_or_path: message.module });
    try {
      await ready;
      postMessage({ type: "ready", version: version() });
    } catch (error) {
      postMessage({ type: "failed", error: `failed to load the compiler: ${error}` });
    }
    return;
  }
  if (message.type !== "compile") return;
  const { id, paths, sources, entry, optimize, warnings, pseudo, sizes, macros } = message;
  try {
    await ready;
  } catch (error) {
    return; // "failed" was already posted.
  }
  calls = 0;
  open.length = 0;
  trace = TRACE;
  stopAt = message.stopAt ?? null;
  fingerprint = message.fingerprint ?? null;
  stopped = false;
  postMessage({ type: "started", id });
  posting = performance.now() + (message.quiet ?? QUIET_MS);
  let result;
  try {
    set_browser_macros(macros === "browser");
    result = compile(paths, sources, entry, optimize, warnings, pseudo, sizes, false);
  } catch (error) {
    // A panic leaves the module unusable: report it, and the page replaces
    // this worker.
    postMessage({ type: "failed", id, error: `the compiler crashed: ${error}` });
    return;
  }
  const answer = plain(result);
  result.free();
  postMessage({ type: "compiled", id, result: answer, stopped });
}

self.onmessage = (event) => receive(event.data);
