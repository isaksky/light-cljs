// The engine side of light-cljs's host evaluator (src/host_evaluator.rs,
// light-cljs-307): with it the compiler runs macros in this JavaScript engine,
// the browser's own, instead of in QuickJS inside the WebAssembly module.
// `createMacroHost(serve)` answers the crate's `ScriptHost` calls; `serve` is
// the module's `serve_macro_host` export, which compiler services the runtime
// calls (`__lightCompileUnit`, `__lightBuildService`) reenter synchronously.
//
// Each compiler session loads src/v8_services.js over the canonical runtime
// into this realm with an indirect eval. A realm has one global object, so
// the sessions share it: opening one first deletes the globals that the
// previous sessions added, but changes macros made to builtins remain.
export function createMacroHost(serve) {
  const sessions = new Map();
  let baseline = null;
  // JSON has no NaN, infinities or -0; they travel as {"$n": text}.
  const special = (value) => value !== value || value === Infinity || value === -Infinity || Object.is(value, -0);
  const replace = (key, value) => (typeof value === "number" && special(value) ? { $n: Object.is(value, -0) ? "-0" : String(value) } : value);
  const revive = (key, value) => (value !== null && typeof value === "object" && !Array.isArray(value) && "$n" in value ? Number(value.$n) : value);
  const parse = (text) => (text.includes('"$n"') ? JSON.parse(text, revive) : JSON.parse(text));
  // A thrown value crosses to Rust as its String value; never call a
  // throwing toString to describe it.
  const describe = (error) => {
    try {
      return String(error);
    } catch {
      return Object.prototype.toString.call(error);
    }
  };
  const guard = (f) => {
    try {
      return f();
    } catch (error) {
      throw describe(error);
    }
  };
  function clean() {
    for (const name of Reflect.ownKeys(globalThis)) {
      // A macro may have made one non-configurable; it then stays.
      if (!baseline.has(name)) Reflect.deleteProperty(globalThis, name);
    }
  }
  return {
    engine: () => (typeof navigator === "object" && navigator.userAgent) || `${typeof process === "object" ? `node ${process.version}` : "unknown"}`,
    open: (session, services) =>
      guard(() => {
        baseline ??= new Set(Reflect.ownKeys(globalThis));
        if (sessions.size === 0) clean();
        sessions.set(session, (0, eval)(services));
      }),
    call: (session, name, args) =>
      guard(() => {
        const services = sessions.get(session);
        const result = services[name](...parse(args));
        services.checkBridge();
        return JSON.stringify(result ?? null, replace);
      }),
    install: (session, name, index, strings) =>
      guard(() => {
        globalThis[name] = strings
          ? (request) => {
              if (typeof request !== "string") throw new TypeError("compiler service expects a string");
              return serve(session, index, true, request);
            }
          : (id) => {
              if (typeof id !== "number" || !Number.isSafeInteger(id) || id < 0) throw new Error("invalid compiler unit service ID");
              return serve(session, index, false, String(id));
            };
      }),
    close: (session) => {
      const services = sessions.get(session);
      sessions.delete(session);
      try {
        services?.clear();
      } catch {}
    },
  };
}
