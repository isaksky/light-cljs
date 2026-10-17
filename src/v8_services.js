            const __lightEvaluator = (() => {
              const rt = __LIGHT_RUNTIME_SOURCE__;
              const compilerEval = globalThis.eval;
              const values = new Map(); let next = 1; let lastError = null;
              let hostDiagnostic = null;
              let heldError = null; let jobFailure = null;
              const rejections = new Map();
              const exceptionMessage = error => {
                if (hostDiagnostic !== null && Object.is(error, hostDiagnostic.value)) { const message = hostDiagnostic.message; hostDiagnostic = null; return message; }
                return rt.compilerExceptionMessage(error);
              };
              const bridge = rt.compilerForms;
              if (bridge.version !== 5) throw new Error("Compiler form bridge version mismatch");
              const clearValues = Map.prototype.clear.bind(values);
              const save = value => { const id = next++; values.set(id, value); return id; };
              const get = id => { if (!values.has(id)) throw new Error('Released or unknown value ID'); return values.get(id); };
              return Object.freeze({
                save,
                registerNamespaces: functions => { for (const fn of functions) fn(rt); },
                canCacheRegistration: () => globalThis.eval === compilerEval,
                registerRoot: (name, unit, once, location, test) => globalThis.__lightRegisterUnit(name, unit, once, location, test),
                bind: (name, id) => { (globalThis.__lightApplicationRoots ??= Object.create(null))[name] = get(id); },
                coreBinding: (name, id) => { rt.core[name] = get(id); },
                coreLookup: name => save(rt.core[name]),
                lookup: name => save((globalThis.__lightApplicationRoots ??= Object.create(null))[name]),
                hostFunction: callback => (...args) => {
                  const ids = args.map(save);
                  try { const [result, ...owned] = callback(ids); try { return get(result); } finally { owned.forEach(id => values.delete(id)); } }
                  finally { ids.forEach(id => values.delete(id)); }
                },
                inputProvider: (name, provider) => save(rt.core.readerInputProvider(name, provider)),
                importForm: descriptor => save(bridge.decode(descriptor)),
                importMacroForm: descriptor => save(bridge.decode(descriptor, null, true)),
                macroArguments: (form, nodes, depth) => {
                  const [inputs, snapshots] = bridge.macroArguments(get(form), nodes, depth), ids = [];
                  try { for (const value of inputs) ids.push(save(value)); return [ids, snapshots]; }
                  catch (error) { for (const id of ids) values.delete(id); throw error; }
                },
                importEnvironment: (descriptor, nodes, depth) => { const [value, snapshots, published] = bridge.decode(descriptor, [nodes, depth]); return [save(value), snapshots, published]; },
                exportForm: (id, nodes, depth) => bridge.encode(get(id), nodes, depth),
                formServices: (one, all, nodes, depth) => rt.setMacroexpandServices({macroexpand1: form => bridge.decode(one(bridge.encode(form, nodes, depth))), macroexpand: form => bridge.decode(all(bridge.encode(form, nodes, depth)))}),
                bridgeVersion: bridge.version,
                checkBridge: bridge.check,
                errorData: () => { const error = lastError; return save(rt.core.exData(error)); },
                errorValue: () => lastError,
                rethrowError: (error, message) => {
                  hostDiagnostic = {value: error, message};
                  throw error;
                },
                evaluate: source => {
                  lastError = null;
                  try { const result = save((0, eval)(source)(rt)); hostDiagnostic = null; return result; }
                  catch (error) { lastError = error; const message = exceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; }
                },
                compilerFrontier: next => {
                  lastError = null;
                  try {
                    if (next == null) globalThis.__lightLeaveCompilerFrontier();
                    else globalThis.__lightEnterCompilerFrontier(next);
                    hostDiagnostic = null;
                  } catch (error) { lastError = error; const message = exceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; }
                },
                call: (id, ids) => {
                  try { const result = save(Reflect.apply(get(id), null, ids.map(get))); hostDiagnostic = null; return result; }
                  catch (error) {
                    lastError = error;
                    const message = exceptionMessage(error);
                    if (message !== undefined) throw new Error(message);
                    throw error;
                  }
                },
                number: id => { const v = get(id); if (typeof v !== 'number') throw new TypeError('Expected Number'); return v; },
                string: id => { const v = get(id); if (typeof v !== 'string') throw new TypeError('Expected string'); return v; },
                lastError: () => save(lastError),
                clearError: () => { lastError = null; },
                // Promise jobs may evaluate, which clears lastError: hold it while
                // they run, then restore it, or report the first job exception.
                holdError: () => { heldError = {value: lastError}; jobFailure = null; },
                jobError: error => { jobFailure ??= {value: error}; },
                releaseError: keep => {
                  const failure = keep ? null : jobFailure;
                  lastError = (failure ?? heldError).value; heldError = jobFailure = null;
                  if (failure !== null) { const message = exceptionMessage(failure.value); if (message !== undefined) throw new Error(message); throw failure.value; }
                },
                // The host's rejection tracker records a promise rejected with no
                // handler until one is attached; takeRejections forgets them all
                // and returns the reasons of up to `limit`, oldest first and
                // printed. With `raise` the first becomes the last exception;
                // otherwise the last exception stays, even when a printer evaluates.
                trackRejection: (promise, reason, handled) => {
                  if (handled) rejections.delete(promise); else rejections.set(promise, reason);
                },
                takeRejections: (limit, raise) => {
                  const reasons = limit > 0 ? [...rejections.values()].slice(0, limit) : []; rejections.clear();
                  const last = raise && reasons.length > 0 ? reasons[0] : lastError;
                  const describe = reason => {
                    try { return rt.core.printForHost(reason); } catch {}
                    try { return Object.prototype.toString.call(reason); } catch { return '<unprintable reason>'; }
                  };
                  try { return reasons.map(describe); } finally { lastError = last; }
                },
                inputWindow: more => rt.core.compilerInputWindow(more),
                inputConsume: units => rt.core.compilerInputConsume(units),
                inputRecover: () => rt.core.compilerInputRecover(),
                inputDiscard: () => rt.core.compilerInputDiscard(),
                inputCompact: () => rt.core.compilerInputCompact(),
                print: id => { try { return rt.core.printForHost(get(id)); } catch (error) { lastError = error; const message = rt.compilerExceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; } },
                validate: id => { get(id); },
                release: id => { bridge.releaseMacroInput(get(id)); values.delete(id); },
                clear: clearValues
              });
            })();
