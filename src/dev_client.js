// The dev client of a light-cljs dev build (docs/dev-workflow.md, "The server
// and the client"), `client.js` of the build directory, which index.js loads
// after the runtime and before the units.
//
// `define(namespace, requires, unit)` records a namespace's unit and runs it
// against the runtime at once, keeping its value; a unit evaluated again (a
// reload) replaces the record and runs again.
//
// `connect(url, options)` reads the event stream of a dev server
// (`--watch --serve`) at `url` with `fetch`, so it works in a page and in
// Node, and interprets its messages, JSON objects with an `op` field:
//
// - `hello`: the server's ID for this runtime.
// - `load`: define the units in order, between the build's
//   `^:dev/before-load` functions (in reverse definition order) and its
//   `^:dev/after-load` functions (in order); an `-async` hook gets a
//   continuation, which the load waits for. A hook that throws is reported
//   and skipped; a unit that throws stops the load, is reported, and leaves
//   later units undefined. The server chose the units (its reload set). A
//   `replay` load, the build as the server has it when the runtime connects,
//   defines only the units whose text the runtime does not already run to
//   completion, so a page that loaded the current files changes nothing and
//   a runtime that missed steps catches up; of a namespace in its `once`
//   list (`^:dev/once`), only a unit the runtime never ran to completion,
//   and when it defines any, also the units of the namespaces in its
//   `always` list (`^:dev/always`) and, with `dependents`
//   (`--reload-dependents`), those downstream of them along each unit's
//   `requires`. One console line per load names the namespaces.
// - `warnings` and `error`: compile diagnostics with file, line and column,
//   drawn in an overlay in a page (and logged); the first message of the
//   next step clears them.
// - `invoke`: evaluate a REPL form's unit in its namespace and post its value
//   printed by `pr-str` under the print options sent, or the error it threw,
//   to `reply` with the request ID.
//
// When the stream ends or fails, the client connects again after a delay
// that doubles from 250 ms to 30 s; a server that has no event stream (a
// status 404) ends the attempts. In a page served over HTTP, the client
// connects to the server it was loaded from once the build has loaded.
// `options.log(level, text)` replaces the console for the client's own
// lines, and `options.onHandled(message, error)` is called after each
// message is handled. `connect` returns `{close()}`.
globalThis.__lightDev = (() => {
  const units = Object.create(null);
  const values = Object.create(null);

  // The overlay, in a page: a fixed panel listing the diagnostics of the
  // last step.
  const page = () => typeof document === "object" && document !== null &&
    typeof document.createElement === "function" && document.body != null;
  const overlay = {
    element: null,
    list: null,
    clear() {
      this.element?.remove();
      this.element = null;
      this.list = null;
    },
    add(kind, title, detail) {
      if (!page()) return;
      if (this.element === null) {
        const element = document.createElement("div");
        element.id = "light-cljs-overlay";
        element.setAttribute("role", "alert");
        element.style.cssText = "position:fixed;top:0;left:0;right:0;max-height:60vh;overflow:auto;" +
          "z-index:2147483647;margin:0;padding:12px 16px;background:rgba(24,24,27,.96);color:#f4f4f5;" +
          "font:13px/1.45 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;" +
          "border-bottom:3px solid #ef4444;box-shadow:0 4px 16px rgba(0,0,0,.4);";
        const close = document.createElement("button");
        close.textContent = "×";
        close.title = "Dismiss";
        close.style.cssText = "float:right;background:none;border:0;color:inherit;font-size:20px;cursor:pointer;";
        close.onclick = () => this.clear();
        element.append(close);
        this.list = document.createElement("div");
        element.append(this.list);
        document.body.append(element);
        this.element = element;
      }
      const entry = document.createElement("div");
      entry.className = "light-cljs-" + kind;
      entry.style.cssText = "margin:4px 0 10px;";
      const head = document.createElement("div");
      head.textContent = title;
      head.style.cssText = "font-weight:bold;color:" + (kind === "error" ? "#fca5a5" : "#fcd34d") + ";";
      const body = document.createElement("pre");
      body.textContent = detail;
      body.style.cssText = "margin:4px 0 0;white-space:pre-wrap;";
      entry.append(head, body);
      this.list.append(entry);
    },
  };

  const location = diagnostic =>
    [diagnostic.file, diagnostic.line, diagnostic.column].filter(part => part != null).join(":");

  function connect(url, options = {}) {
    const log = options.log ?? ((level, text) => console[level](text));
    const reply = new URL("reply", url).href;
    let runtime = null;
    let step = null;
    let closed = false;
    let controller = null;
    let timer = null;
    let queue = Promise.resolve();
    let attempts = 0;

    const runHook = async ({ name, async }) => {
      const hook = globalThis.__lightApplicationRoots?.[name];
      if (typeof hook !== "function") return;
      try {
        await (async ? new Promise(done => hook(done)) : hook());
      } catch (error) {
        log("error", `light-cljs: hook ${name} threw: ${error?.stack ?? error}`);
      }
    };
    // Whether the unit `unit` of a replay is one the runtime runs: the text
    // of the function `define` recorded is in it, and it ran to its end. Of a
    // `^:dev/once` namespace, any unit that ran to its end is.
    const current = (unit, once) => {
      const known = units[unit.namespace];
      return known !== undefined && known.loaded &&
        (once.has(unit.namespace) || unit.js.includes(String(known.unit)));
    };
    // The units of a replay to load, as the steps the runtime missed would
    // have loaded them: those the runtime does not run, and with them those
    // of the `^:dev/always` namespaces and, under `dependents`
    // (`--reload-dependents`), of every namespace that requires one loaded,
    // directly or not; never a `^:dev/once` unit the runtime ran. The
    // message's units are in load order, each after those it requires.
    const replayed = message => {
      const once = new Set(message.once ?? []);
      const always = new Set(message.always ?? []);
      const held = unit => once.has(unit.namespace) && current(unit, once);
      const stale = message.units.filter(unit => !current(unit, once));
      if (stale.length === 0) return [];
      const loading = new Set(stale.map(unit => unit.namespace));
      for (const unit of message.units) {
        if (held(unit)) continue;
        if (always.has(unit.namespace) ||
            message.dependents && (unit.requires ?? []).some(required => loading.has(required))) {
          loading.add(unit.namespace);
        }
      }
      return message.units.filter(unit => loading.has(unit.namespace));
    };
    const load = async message => {
      const begun = Date.now();
      const pending = message.replay ? replayed(message) : message.units;
      if (pending.length === 0) return;
      for (const hook of [...message["before-load"]].reverse()) await runHook(hook);
      for (const unit of pending) {
        try {
          (0, eval)(unit.js + "\n//# sourceURL=" + new URL(unit.file, url).href);
        } catch (error) {
          const detail = String(error?.stack ?? error);
          log("error", `light-cljs: ${unit.namespace} threw while it loaded; later units were not loaded\n${detail}`);
          overlay.add("error", `${unit.namespace} (${unit.file}) threw while it loaded`, detail);
          throw error;
        }
      }
      for (const hook of message["after-load"]) await runHook(hook);
      const names = pending.map(unit => unit.namespace).join(", ");
      log("info", `light-cljs: reloaded ${names} in ${Date.now() - begun} ms`);
    };
    const printed = (print, value) => {
      const core = globalThis.__lightRuntime.core;
      const cells = {
        printReadably: print?.readably ?? true, printLength: print?.length ?? null,
        printLevel: print?.level ?? null, printMeta: print?.meta ?? false,
        printNamespaceMaps: print?.["namespace-maps"] ?? false, printDup: print?.dup ?? false,
      };
      const saved = {};
      for (const key of Object.keys(cells)) { saved[key] = core[key]; core[key] = cells[key]; }
      try { return core.prStr(value); }
      finally { Object.assign(core, saved); }
    };
    const invoke = async message => {
      const answer = { runtime, id: message.id };
      try {
        const rt = globalThis.__lightRuntime;
        rt.core.currentNs = rt.core.createNs(rt.symbol(null, message.namespace));
        const value = (0, eval)("(" + message.js + "\n)")(rt);
        answer.value = printed(message.print, value);
      } catch (error) {
        answer.error = { message: String(error?.message ?? error), stack: error?.stack ?? null };
      }
      const response = await fetch(reply, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(answer),
      });
      await response.arrayBuffer();
    };
    const handle = async message => {
      if (message.op === "hello") {
        runtime = message.runtime;
        step = null;
        overlay.clear();
        log("info", `light-cljs: connected to ${new URL("/", url).href} as runtime ${runtime}`);
        return;
      }
      if (message.step != null && message.step !== step) {
        step = message.step;
        overlay.clear();
      }
      switch (message.op) {
        case "load": return load(message);
        case "warnings":
          for (const warning of message.warnings) {
            log("warn", `light-cljs: WARNING: ${location(warning)}: ${warning.message}`);
            overlay.add("warning", "Warning: " + location(warning), warning.message);
          }
          return;
        case "error":
          log("error", `light-cljs: ${location(message.error)}: ${message.error.message}`);
          overlay.add("error", "Compile error: " + location(message.error), message.error.message);
          return;
        case "invoke": return invoke(message);
        default: return;
      }
    };
    const enqueue = message => {
      queue = queue.then(async () => {
        let failure = null;
        try { await handle(message); } catch (error) { failure = error; }
        options.onHandled?.(message, failure);
      });
    };
    const read = async () => {
      controller = new AbortController();
      const response = await fetch(url, {
        signal: controller.signal,
        headers: { accept: "text/event-stream" },
        cache: "no-store",
      });
      if (response.status === 404) return "absent";
      if (!response.ok || response.body == null) {
        await response.body?.cancel();
        throw new Error(`the dev server answered ${response.status}`);
      }
      const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
      let buffer = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return "ended";
        buffer += value.replaceAll("\r\n", "\n");
        let end;
        while ((end = buffer.indexOf("\n\n")) >= 0) {
          const block = buffer.slice(0, end);
          buffer = buffer.slice(end + 2);
          const data = block.split("\n")
            .filter(line => line.startsWith("data:"))
            .map(line => line.slice(line.startsWith("data: ") ? 6 : 5))
            .join("\n");
          if (data === "") continue;
          attempts = 0;
          enqueue(JSON.parse(data));
        }
      }
    };
    (async () => {
      while (!closed) {
        let ended;
        try {
          ended = await read();
        } catch (error) {
          if (closed) break;
          ended = String(error?.message ?? error);
        }
        if (closed) break;
        if (ended === "absent") {
          log("info", `light-cljs: no dev server at ${url}`);
          break;
        }
        const delay = Math.min(30000, 250 * 2 ** attempts++);
        if (runtime !== null) {
          log("warn", `light-cljs: disconnected from the dev server (${ended}); reconnecting`);
          runtime = null;
        }
        await new Promise(done => { timer = setTimeout(done, delay); });
        timer = null;
      }
    })();
    return {
      close() {
        closed = true;
        controller?.abort();
        if (timer !== null) clearTimeout(timer);
      },
    };
  }

  const dev = {
    units,
    values,
    overlay,
    define(namespace, requires, unit) {
      // A unit counts as loaded once it has run to its end, so a replay
      // runs again one that threw.
      const record = { requires, unit, loaded: false };
      units[namespace] = record;
      values[namespace] = unit(globalThis.__lightRuntime);
      record.loaded = true;
      return values[namespace];
    },
    connect,
  };

  // In a page served over HTTP, connect to the server this script came from
  // once the build has loaded (or failed to).
  const script = typeof document === "object" && document !== null ? document.currentScript : null;
  if (typeof script?.src === "string" && /^https?:/.test(script.src) && typeof fetch === "function") {
    const events = new URL("events", script.src).href;
    Promise.resolve(globalThis.__lightDevLoaded).catch(() => {}).then(() => connect(events));
  }
  return dev;
})();
