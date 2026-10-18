// Runs the WebAssembly compiler off the page's thread, so the editor stays
// responsive while a compile runs and the page can stop one by terminating
// this worker (compiler.js then starts a fresh one). The page compiles the
// wasm module once and sends it in the "init" message; each "compile" message
// is answered by one "compiled" message with the same id, in order.
import init, { compile, version } from "./pkg/light_cljs_playground.js";

let ready = null;

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
  const { id, paths, sources, entry, optimize, warnings, pseudo, sizes } = message;
  try {
    await ready;
  } catch (error) {
    return; // "failed" was already posted.
  }
  let result;
  try {
    result = compile(paths, sources, entry, optimize, warnings, pseudo, sizes, false);
  } catch (error) {
    // A panic leaves the module unusable: report it, and the page replaces
    // this worker.
    postMessage({ type: "failed", id, error: `the compiler crashed: ${error}` });
    return;
  }
  const answer = plain(result);
  result.free();
  postMessage({ type: "compiled", id, result: answer });
}

self.onmessage = (event) => receive(event.data);
