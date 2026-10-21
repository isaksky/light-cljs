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
//   to `reply` with the request ID. One from a REPL session first runs the
//   `setup` script of what the form loaded, keeps the session's `*1`, `*2`,
//   `*3` and `*e`, and gives a value that is not finite data a reference
//   number, `ref`, by which `light.dev/ref` returns it and `light.dev/inspect`
//   describes it.
// - `run`: run the script of what a REPL form loaded, sent to the runtimes
//   that do not run the form, or to all when a later part of it failed.
// - `forget`: drop the history of a REPL session that closed.
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

  // The connected REPL's side of the runtime: each REPL session's `*1`, `*2`,
  // `*3` and `*e`, and the values a reply could not give as data, kept by
  // reference number for `light.dev/ref` and `light.dev/inspect`, the
  // newest `REFERENCES`.
  const REFERENCES = 1000;
  const repl = { sessions: new Map(), references: new Map(), next: 1 };
  const reference = id => {
    const number = Number(id);
    if (!repl.references.has(number)) {
      throw new Error(`light.dev: no reference ${id} in this runtime`);
    }
    return repl.references.get(number);
  };
  const remember = value => {
    const number = repl.next++;
    repl.references.set(number, value);
    if (repl.references.size > REFERENCES) {
      repl.references.delete(repl.references.keys().next().value);
    }
    return number;
  };
  // Whether `value`, printed under `print`, is finite data that the printed
  // text gives whole: nil, a boolean, number, string, keyword, symbol, UUID,
  // instant, regex or tagged literal, or a collection of such within the
  // print length and level. Anything else, a function, an atom, a host
  // object, a sequence cut short, comes back as a reference too.
  const finite = (value, print) => {
    const core = globalThis.__lightRuntime.core;
    const length = print?.length ?? null;
    const level = print?.level ?? null;
    let budget = 100000;
    const walk = (value, depth) => {
      if (--budget < 0) return false;
      if (value == null || typeof value === "boolean" || typeof value === "number" ||
          typeof value === "string") return true;
      if (core.isKeyword(value) || core.isSymbol(value) || core.isUuid(value) ||
          core.isInst(value) || core.isRegex(value)) return true;
      if (core.isTaggedLiteral(value)) return walk(value.form, depth);
      if (!core.isColl(value) || level !== null && depth >= level) return false;
      let count = 0;
      for (let items = core.seq(value); items != null; items = core.next(items)) {
        if (length !== null && count++ >= length) return false;
        const item = core.first(items);
        // A map's entries print at its level, their keys and values below.
        const parts = core.isMap(value) ? [core.nth(item, 0), core.nth(item, 1)] : [item];
        if (!parts.every(part => walk(part, depth + 1))) return false;
      }
      return true;
    };
    return walk(value, 0);
  };
  // What `light.dev/inspect` tells of a reference: its type, and as fits it
  // the function's name and arity, the count, the dereferenced value, or a
  // host object's keys.
  const inspect = id => {
    const value = reference(id);
    const core = globalThis.__lightRuntime.core;
    const k = name => core.keyword(name);
    const entries = [k("ref"), Number(id), k("type"),
      typeof value === "function" ? "function" : value?.constructor?.name ?? typeof value];
    if (typeof value === "function") {
      entries.push(k("name"), core.demunge(value.name), k("arity"), value.length);
    }
    if (value != null && core.isCounted(value)) entries.push(k("count"), core.count(value));
    if (value != null && core.satisfies(core.IDeref, value) &&
        !(core.IPending && core.satisfies(core.IPending, value) && !core.realized(value))) {
      entries.push(k("deref"), core.deref(value));
    }
    const prototype = value !== null && typeof value === "object" ? Object.getPrototypeOf(value) : undefined;
    if (Array.isArray(value) || prototype === Object.prototype || prototype === null) {
      entries.push(k("keys"), core.vec(Object.keys(value).slice(0, 100)));
    }
    return core.arrayMap(...entries);
  };
  const command = name => () => {
    throw new Error(`light.dev/${name} is a command of the connected REPL: evaluate it as a form of its own`);
  };
  const roots = globalThis.__lightApplicationRoots ??= Object.create(null);
  roots["light.dev/ref"] = reference;
  roots["light.dev/inspect"] = inspect;
  roots["light.dev/runtimes"] = command("runtimes");
  roots["light.dev/select!"] = command("select!");
  roots["light.dev/repl"] = command("repl");

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
    // An `invoke`: run its `setup` script (what the form loaded), then its
    // unit, `js`, if any, in its namespace, with the `*1`, `*2`, `*3` and
    // `*e` of its REPL `session`, which a value rotates and an error sets.
    const invoke = async message => {
      const answer = { runtime, id: message.id };
      const rt = globalThis.__lightRuntime;
      const core = rt.core;
      let history = null;
      if (message.session != null) {
        history = repl.sessions.get(message.session) ?? [null, null, null, null];
        repl.sessions.set(message.session, history);
        [core.replOne, core.replTwo, core.replThree, core.replError] = history;
      }
      try {
        if (message.setup) (0, eval)(message.setup);
        core.currentNs = core.createNs(rt.symbol(null, message.namespace));
        const value = message.js == null ? null : (0, eval)(message.js)(rt);
        answer.value = printed(message.print, value);
        if (message.session != null && !finite(value, message.print)) answer.ref = remember(value);
        if (history !== null) history.splice(0, 3, value, history[0], history[1]);
      } catch (error) {
        if (history !== null) history[3] = error;
        answer.error = { message: String(error?.message ?? error), stack: error?.stack ?? null };
        try {
          const data = core.exData(error);
          if (data != null) answer.error.data = printed(message.print, data);
        } catch {}
      }
      if (history !== null) [core.replOne, core.replTwo, core.replThree, core.replError] = history;
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
        case "forget":
          // A REPL session closed: its history goes.
          repl.sessions.delete(message.session);
          return;
        case "run":
          // What a REPL form loaded, for the runtimes it was not sent to.
          try {
            (0, eval)(message.js);
          } catch (error) {
            log("error", `light-cljs: a REPL load threw: ${error?.stack ?? error}`);
            throw error;
          }
          return;
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
    repl,
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
