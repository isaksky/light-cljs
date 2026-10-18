# Browser-native macros for the playground

Status: prototype (light-cljs-307), off by default. The playground's
WebAssembly module embeds QuickJS so that the compiler can run macros while it
compiles. This document records a second way: the compiler runs the same
compiler-emitted macro JavaScript in the browser's own engine, the one already
running the module. It covers how that works, what it preserves, where its
isolation and resource limits differ from QuickJS, what it measured, and
whether to adopt it.

## How it works

`src/host_evaluator.rs` is a compiler evaluator (`MacroEvaluator` and
`CompilerEvaluator`, the traits the QuickJS and V8 evaluators implement) over
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

`MacroHost::set_script_host` selects the host for clean compilations;
interactive sessions and data readers keep QuickJS. `cache_key` keys such a
compilation's artifacts with `host` and the engine's own description.

In the playground, `playground/macro-host.js` implements `ScriptHost` in the
compiler worker (`globalThis.lightCljsMacroHost`). The module exports
`set_browser_macros(bool)` and `serve_macro_host`; `compiler-worker.js` runs
a request's macros in the browser when it carries `macros: "browser"`, and
the page sends that when its "browser macros" checkbox is checked.

## What it preserves

- **Form identity.** A form a macro returns unchanged is the compiler's own
  form again, with its location: `(defmacro ident [x] x)` around an
  undeclared Var warns at the original bytes, and `(identical? a (second
  &form))` holds, because the call is imported once and its arguments are
  the elements of `&form` (`importMacroForm` and `macroArguments`, as in V8).
- **Namespace snapshots.** Every import is followed by the canonical
  snapshot (`exportForm` with `snapshot`), and an export recovers original
  data only when the canonical value is exact, as in both native bridges: a
  `#js` object a macro mutates and returns keeps the new value, and a helper
  keeps the macro generation and values it was defined against.
- **Compiler callback reentry.** A macro that calls a function defined
  earlier in its namespace makes the runtime call `__lightCompileUnit`,
  which analyzes and registers that unit from inside the macro call. A
  failing service is terminal: the outer operation reports its diagnostic
  even when JavaScript catches the thrown error: a caught compile dependency
  cycle still fails the compile with the cycle as its message, where the
  embedded QuickJS reports only that the session was invalidated.
- **Macro semantics.** `playground/check.mjs` compiles every example at
  every level, and programs built to stress macros (a helper emitted on
  demand, recursion, gensyms, `&form` metadata, `&env` locals, special
  numbers, unpaired surrogates, a thrown `ex-info`, a non-form result), with
  both engines, and requires identical results. Besides the caught failure
  above, the one difference is a
  macro's error message: QuickJS appends its stack trace, the browser
  engine does not (the native V8 evaluator also omits it).

## Isolation and resource limits

| | Embedded QuickJS | Browser engine |
|---|---|---|
| Heap | A fresh heap per compile | The compiler worker's one realm. Opening a session deletes the globals earlier sessions added; changes to builtins persist until the worker is replaced |
| Capabilities | ECMAScript only | Everything a module worker has: `fetch`, `WebSocket`, `postMessage` to the page (a macro could forge a "compiled" answer), `navigator`, `indexedDB`, the macro host itself |
| Time | The interrupt handler charges a fuel budget of one million polls and checks the 10 s session deadline while JavaScript runs | Checked only at bridge boundaries. A loop inside JavaScript runs until the page's Stop terminates the worker (`check.mjs` checks both engines) |
| Memory | 64 MiB allocation ceiling, terminal | The browser's per-worker heap limit (gigabytes); exhausting it kills the worker, which the page replaces |
| Stack | 1 MiB engine stack, terminal | The browser's stack; overflow is a catchable RangeError, reported as the macro's error |
| Determinism | One engine everywhere | Each browser's engine. Output was identical across Chromium, Firefox and WebKit here, but a macro that formats numbers with `Intl`, reads the time zone or relies on engine-specific behavior can expand differently per browser |

For the playground these are acceptable: the person writing the macros is the
person running the page, and the code is theirs. They rule out the backend
where untrusted code expands: the CLI, the dev server and any shared
compile service keep QuickJS or V8.

## Measurements (October 18, 2026)

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

## Verdict

Ready to adopt as an option; not yet as the playground's only engine.

- Macro output is identical to QuickJS in every check, across three engines,
  and compiles are 2.5 to 3.2 times faster. The checkbox can stay, and could
  default to on once the next point is done.
- The 10 s deadline does not hold inside JavaScript. Before defaulting to
  browser macros, `compiler.js` should stop a compile after the same 10 s by
  terminating its worker, so a runaway macro ends without the user pressing
  Stop.
- The download saving requires removing QuickJS from the wasm build. Data
  readers (`MacroHost::install_reader_build_services` and the reader heap),
  interactive sessions and `EvalSession` use the concrete QuickJS
  `Evaluator`; they need the host evaluator too, or a `cfg` that leaves them
  out of the playground, before `rquickjs` can leave the module.
- JSON costs 30 ms on the engine side alone, plus Rust's encoding and parsing. The V8 bridge's
  environment cache (`encode_environment`) and folding each import with its
  snapshot into one call would cut it, if the playground needs more speed.

### As built (light-cljs-307)

- `src/host_evaluator.rs`: `ScriptHost`, `serve`, `HostEvaluator`, the JSON
  descriptor transport and its unit test. It implements the trait defaults
  for environment import (no environment cache) and evaluates registrations
  and roots through `evaluate`, as an external adapter may.
- `MacroHost::set_script_host`; `cache_key::compiler_identity` keys `host`.
- `playground/macro-host.js`, `set_browser_macros` and `serve_macro_host` in
  the crate, `macros` in compile requests, the page's checkbox, and the
  checks in `playground/check.mjs`.
- The cross-browser numbers came from a Playwright script outside the repo
  that serves `playground/dist`, compiles through `compiler.js` and its
  module worker in each browser, compares both engines' results for every
  example at every level and times the program above. Safari itself was not
  driven; WebKit stands in for it.
