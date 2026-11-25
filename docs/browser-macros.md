# Browser macros for the playground

The playground's WebAssembly compiler runs emitted macro JavaScript in the
browser's engine. Macros and the compiler run in the same worker. A separate
worker executes the finished program.

## How it works

`src/host_evaluator.rs` is a compiler evaluator (`MacroEvaluator` and
`CompilerEvaluator`, the traits the V8 evaluator implements) over
an engine the embedder supplies through the public `ScriptHost` trait:

- `open(session, services)` runs one script whose completion value is the
  session's service object. It is the V8 evaluator's `src/v8_services.js`
  over the canonical runtime (`RUNTIME_SOURCE`), unchanged, so both engines
  load the same runtime and the same bridge services.
- `call(session, name, arguments)` calls one service with a JSON array of
  arguments and answers JSON. The arguments carry bridge ABI 5 descriptors,
  the nested arrays the V8 bridge builds natively (`[tag, provenance,
  metadata, payload...]`). Strings cross as UTF-16 code units: Rust escapes
  every non-ASCII unit, and `JSON.stringify` escapes unpaired surrogates, so
  they survive both ways. NaN, the infinities and -0 cross as `{"$n": text}`.
- `install(session, name, index, strings)` defines a global compiler service
  (`__lightCompileUnit`, `__lightBuildService`). When the runtime calls one,
  the host calls back into the module synchronously through
  `host_evaluator::serve`, which runs the compiler's callback with this same
  session as its evaluator, so nested analysis and macro expansion reenter
  the engine again.
- `close(session)` drops the service object.

A provenance is a positive integer naming an entry in the session's table of
borrowed compiler data (the V8 bridge wraps the same payload in a cppgc
object). The table lives until the session closes: a synchronous bridge has
no finalization it could observe.

`MacroHost::set_script_host` selects the host for clean compilations.
Interactive sessions and compiler data readers require a V8 build.
`cache_key` keys hosted compilations with `host` and the engine's own
description.

In the playground, `playground/macro-host.js` implements `ScriptHost` in the
compiler worker (`globalThis.lightCljsMacroHost`), and the module exports
`serve_macro_host`. Every compile runs its macros there: the module embeds
no engine of its own.

## What it preserves

- **Form identity.** A form a macro returns unchanged is the compiler's own
  form again, with its location: `(defmacro ident [x] x)` around an
  undeclared Var warns at the original bytes, and `(identical? a (second
  &form))` holds, because the call is imported once and its arguments are
  the elements of `&form` (`importMacroForm` and `macroArguments`, as in V8).
- **Namespace snapshots.** Every import is followed by the canonical
  snapshot (`exportForm` with `snapshot`), and an export recovers original
  data only when the canonical value is exact, as in the V8 bridge. A
  `#js` object a macro mutates and returns keeps the new value, and a helper
  keeps the macro generation and values it was defined against.
- **Compiler callback reentry.** A macro that calls a function defined
  earlier in its namespace makes the runtime call `__lightCompileUnit`,
  which analyzes and registers that unit from inside the macro call. A
  failing service is terminal: the outer operation reports its diagnostic
  even when JavaScript catches the thrown error. A caught compile dependency
  cycle still fails the compile with the cycle as its message.
- **Macro semantics.** `playground/check.mjs` compiles every example at
  every level, and programs built to stress macros (a helper emitted on
  demand, recursion, gensyms, `&form` metadata, `&env` locals, special
  numbers, unpaired surrogates, a thrown `ex-info`, a non-form result).
  The check compares emitted programs and located errors against the
  recorded expectations.

## Isolation and resource limits

Opening a macro session clears globals added by previous sessions in the
compiler worker. Changes to builtins persist until the worker is replaced.
Macros can use worker APIs such as `fetch`, `WebSocket` and `indexedDB`.
A macro can forge a `compiled` reply to the page via `postMessage`. The
playground's browser macro backend is unsuitable for expanding untrusted code.

The browser sets the worker's heap and stack limits. JavaScript stack
overflow raises a catchable exception such as `RangeError`. If it escapes
the macro, the compiler reports it as a macro error. The command line and
development server use embedded V8, which enforces a configurable memory
budget and stack limit and checks execution limits while JavaScript runs.
[V8 evaluator](evaluator.md)

The October 2026 comparison produced identical output in Chromium,
Firefox and WebKit. Macros that use `Intl`, read the time zone or rely on
engine-specific behavior can expand differently in different browsers.

### Deadline watchdog

In the browser, the compiler checks its deadline and poll budget at bridge
boundaries. When a macro host call is still open after the 10-second deadline
and a 250 ms grace period, the watchdog in `playground/compiler.js` terminates
the worker. If the worker reports that the compiler is between host calls,
the watchdog waits for the compiler to check the deadline at its next call.
Stop also terminates the worker so the next compile starts in a fresh one.

The worker numbers host calls and fingerprints the sequence of services and
arguments. It starts reporting calls after 500 ms to avoid slowing down
short compiles. If the stuck call began before then, the watchdog probes in
a fresh worker that reports every call and selects the one that stays open
for one second.

To recover a source location, the watchdog replays the compile in a fresh
worker. The replay reruns earlier macros and raises `JavaScript session
deadline exceeded` just before the stuck call. The watchdog uses the location
only if the fingerprint matches, and counts elapsed time from the original
start. It reports the deadline without a source location if the stuck call
started more than 1.5 seconds into its run, the probe or replay runs out of
time, or the replay fails to reach the same call. Probes and replays share a
three-second allowance after the watchdog fires.

If a worker fails to start a queued compile within 10 seconds, for example
because a macro left a timer running, the page replaces it and resends the
newest request. `playground/check.mjs` covers the watchdog and its fallbacks.

## Historical measurements (October 18, 2026)

QuickJS was removed from light-cljs. These measurements describe the
playground prototype from October 2026.

A macro-heavy program: 30 copies of a block of user macros (one emitting
`defn`s through a helper from its own namespace, a recursive `my-and`, a
gensym `with-trace`, an unrolling `for`) and core JavaScript macros (`cond->`,
`->`, `->>`, `some->`, `case`, `cond`, `when-let`, `if-let`, `for`), 455 KB of
output at `--optimize prepare`, close to the default limit of 1,024
expansions. The time is the compile's own (`millis`), median of five warm
compiles in one worker, alternating engines; "cold" is the first compile in a
fresh worker, which also builds the macro bootstrap. Load average 2.5 to 8 on
the shared 24-vCPU Mac.

| Engine | Embedded QuickJS | Browser macros | Speedup | Cold, QuickJS / browser |
|---|---|---|---|---|
| Node 24.18 (V8), direct | 490 ms | 197 ms | 2.5x | 644 / 330 ms |
| Chromium 153 (headless), worker | 468 ms | 152 ms | 3.1x | 535 / 200 ms |
| Firefox 155, worker | 570 ms | 224 ms | 2.5x | 648 / 263 ms |
| WebKit 26.6, worker | 454 ms | 143 ms | 3.2x | 539 / 246 ms |

Each of these compiles makes 7,582 host calls carrying 3.76 MB of JSON, and
about 120 ms of Node's 210 ms is spent inside host calls (macro execution, the
bridge in JavaScript and 30 ms of `JSON.parse` and `JSON.stringify`). In the real page, the calculator example compiled in 225 ms the
first time with QuickJS and in 39 ms, then 20 ms, with browser macros.

Download size of the module (`light_cljs_playground_bg.wasm`, built on master 7931693):

| Build | Bytes | gzip -9 |
|---|---|---|
| Before this prototype | 6,385,991 | 2,018,361 |
| With the prototype (QuickJS still linked) | 6,444,001 (+0.9%) | 2,040,034 (+1.1%) |
| QuickJS alone, same profile (rquickjs, its C engine and wasm shims) | 1,093,751 | 403,086 |

The prototype adds 58 KB. The gain in download size would come only from
dropping QuickJS from the module, about 1.05 MB (17%) raw and 0.39 MB (19%)
compressed, projecting roughly 5.3 MB and 1.63 MB gzip. Of the stripped
module's 4.2 MB of code, QuickJS's C engine is 0.87 MB, its other C
helpers 0.14 MB and the Rust that only drives QuickJS (rquickjs,
`evaluator.rs`, `form_bridge.rs`) 0.14 MB.

After the playground switched to its browser host, the module measured
5,535,568 bytes (1,657,208 gzip -9), 17.9% (22.2%) smaller than the
comparison build. [Compiler benchmarks](../benchmarks/compiler.md)
