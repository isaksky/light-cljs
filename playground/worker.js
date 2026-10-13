// Runs one compiled program in a fresh worker. Printing goes through console,
// which the runtime's println uses, so every console method is relayed to the
// page. The page terminates the worker when the run ends or times out.

function text(args) {
  return args
    .map((value) => {
      if (typeof value === "string") return value;
      try {
        return String(value);
      } catch (error) {
        return Object.prototype.toString.call(value);
      }
    })
    .join(" ");
}

for (const level of ["log", "info", "debug", "warn", "error"]) {
  console[level] = (...args) => postMessage({ type: "console", level, text: text(args) });
}

self.onmessage = (event) => {
  const { js } = event.data;
  const started = performance.now();
  let value;
  try {
    // The emitted program is one expression; indirect eval yields its value.
    value = (0, eval)(js);
  } catch (error) {
    postMessage({
      type: "error",
      text: error && error.stack ? String(error.stack) : String(error),
      millis: performance.now() - started,
    });
    return;
  }
  postMessage({ type: "result", text: text([value]), millis: performance.now() - started });
};
