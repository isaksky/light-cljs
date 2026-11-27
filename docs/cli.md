# Command-line reference

Run `light-cljs --help` (or `cargo run -- --help`) for the built-in option summary.

If you have not installed the binary (`cargo install --path .`), replace `light-cljs` in the commands below with `cargo run --quiet --` or `target/release/light-cljs`.

## Project builds (`light-cljs.edn`)

`light-cljs.edn` defines a project's builds, modeled on `shadow-cljs.edn`: top-level `:source-paths`, cache settings, dev server/nREPL config, and `:builds` (each with `:target :esm`, `:output-dir`, one module's `:entries` namespace, and `:dev` / `:release` overrides). See [docs/build-configuration.md](build-configuration.md) for the full schema.

The subcommands `compile`, `release`, `watch`, and `cljs-repl` walk up from the current directory to locate `light-cljs.edn`. Every relative path in the file and in CLI flags resolves against the file's directory, so running from a subdirectory produces the same output in the same location.

```sh
cd examples/features
light-cljs release features   # chunked release build in out/release (:advanced)
light-cljs compile features   # dev build in out/dev, once
light-cljs watch features     # dev build in out/dev, watched and served
```

Build IDs can be written with or without a leading colon (`features` or `:features`). A command that begins with a flag never reads `light-cljs.edn`. To tokenize a file literally named `compile`, `watch`, `release`, `cljs-repl`, or `test`, pass `./compile`. `light-cljs test` (below) also finds `light-cljs.edn` this way when it is given no `--source-root`.

### `compile` and `release`

- **`compile ID [ID ...]`** writes each build's reloadable dev build (equivalent to `--compile --reloadable --out-dir`), in the order named, stopping at the first failure.
- **`release ID [ID ...]`** writes each build's chunked release build at its configured `:optimizations` (`:advanced` by default, with assertions elided unless `:compiler-options {:elide-asserts false}` and `deftest` forms omitted unless `:compiler-options {:load-tests true}`), equivalent to `--compile --optimize advanced --out-dir`.

### `watch`

`watch ID [ID ...]` builds, watches, and serves one or more builds in a single process (like shadow-cljs's `watch app worker`):

- Runs each startup build in order, then starts one dev HTTP server and one shared `:source-paths` poller; subsequent edits recompile affected builds concurrently on per-build threads.
- **Dev server:** binds `:http`'s `:host` and `:port` (`127.0.0.1:9630` by default; `0` for any free port). If the port is in use, it tries the next free port upward unless `:http {:strict true}`. The first build is served at `/` and every build at `/ID/`. The bound port is written to `<:cache-root>/http.port` (`.light-cljs/http.port` by default) and removed on exit.
- **nREPL:** served on the HTTP port unless `:nrepl false`. Setting `:nrepl {:port N}` starts a dedicated nREPL listener and writes `.nrepl-port` in the project directory.
- **Polling & reload strategy:** `:fs-watch {:loop-wait N}` maps to `--poll-ms N`, and `:devtools {:reload-strategy :full}` maps to `--reload-dependents`.
- **Logging & overlay:** stderr lines and browser overlay messages are prefixed with the build ID (e.g. `[:features] recompiled app.shared in 41.2 ms`). Compile errors in one build do not stop other watched builds.
- **Live config reload:** saving `light-cljs.edn` reloads it automatically. Invalid EDN is reported to stderr and the browser overlay without stopping the running watch. Changes to a build's settings (such as `:config`) restart that build from cache and reconnect its pages; top-level changes (such as `:source-paths`) restart all builds. Comments or edits to unwatched builds are ignored. Changes to `:http`, `:nrepl`, or `:fs-watch` print a notice that a watch restart is required.

### `cljs-repl`

`cljs-repl ID` connects a stdin REPL to a running `watch` for build `ID`:

```sh
light-cljs watch app worker    # terminal 1
light-cljs cljs-repl worker    # terminal 2, from the same project
# app.worker=> (jobs/run 2)
# 42
```

- Discovers the watch via `<:cache-root>/http.port` on `127.0.0.1` and binds the session to `ID` in its entry namespace (as if starting with `(light.dev/repl :ID)`).
- Accepts `--print-length N` and `--print-level N`.
- Never auto-starts a watch; if none is running or `ID` is not watched, it exits with an error listing the active builds.

### Switching builds inside a watch REPL

Any REPL connected to a `watch` (nREPL or `--repl --connect`) starts bound to the first watched build in its entry namespace:

- `(light.dev/repl :worker)` switches the session to another watched build (returns `[:selected :worker]`).
- `(light.dev/runtimes)` lists connected runtimes across all builds, each tagged with `:build`.
- `(light.dev/select! ID)` selects a specific runtime within the currently bound build.

### Overriding config with flags and `--print-config`

Flags placed *after* the build IDs override `light-cljs.edn` for every named build:

- `compile` accepts `--compile --reloadable` flags; `watch` accepts `--watch` flags (`--serve PORT` binds that exact port or fails; `--no-nrepl` acts like `:nrepl false`); `release` accepts `--compile` flags (plus `--warnings`, `--checked-arrays-warn`, `--checked-arrays-error`).
- `--source-root DIR` replaces `:source-paths`; `--config KEY=EDN` overrides one `:config` key; `--cache-dir DIR` enables and sets the cache store; `--no-cache` disables caching; `--cache-blocker NS` appends to blockers; `--cache-retention N` overrides top-level `:cache-retention`.
- `--config-merge DATA` (repeatable) deep-merges an EDN map (inline text starting with `{`, or a file path) over each build after its `:dev` / `:release` overrides.
- `--entry` and `--out-dir` require a single build ID; `--esm` and source-map flags are rejected on subcommands; watch-only flags are accepted only by `watch`.
- **`--print-config`** prints the fully merged configuration EDN for each build—with `;` comments showing each value's origin—without compiling or writing files:

```sh
light-cljs release features --optimize none --print-config
# ;; build :features, release mode, from /work/light-cljs/examples/features/light-cljs.edn
# {:source-paths ["."]                                 ; light-cljs.edn:7:16
#  :cache-root ".light-cljs"                           ; default
#  ...
#    :compiler-options {:optimizations :none           ; --optimize none, over the release default :advanced
#    :build-options {:cache-level :all}}}}             ; default
```

---

## Script evaluation and REPL (embedded V8)

```sh
light-cljs --eval --text '(/ 1 2)'
# 0.5
light-cljs --eval --source-root examples/native examples/native/demo.cljs
# Hello from compiled JavaScript
# {:squares [1 4 9 16 25], :sum 15}
printf '(+ 20\n 22)\n(* *1 2)\n' | light-cljs --repl
# 42
# 84
```

- **`--eval`** accepts a `.cljs`/`.cljc` file, `-` (stdin), or `--text SOURCE`. Standard output streams immediately; the printed final value is emitted only on success.
- **`--repl`** preserves definitions, namespaces, and `*1`/`*2`/`*3`/`*e`, supports multiline input, recovers from reader and language errors, and prints prompts to stderr.
- **Namespaces:** loaded only from explicit `--source-root DIR` flags (`.cljs` preferred over `.cljc`; `.clj` files are rejected).
- **Resource & print limits:** `--poll-budget`, `--memory-limit`, `--stack-limit`, `--timeout-ms`, `--source-limit`, `--output-limit`, `--print-length`, `--print-level`. The V8 heap requires at least 16 MiB (`16777216` bytes) at startup.
- **`--warnings`** prints undeclared Vars and arity conflicts to stderr instead of rejecting the form, allowing `(require 'app.view :reload)` when a file references a Var defined later.
- **`--elide-asserts`** strips `assert` and `fn` `:pre`/`:post` checks in `--eval` and `--repl` too.
- **`--engine`** accepts only `v8` (legacy `--engine quickjs` reports that QuickJS was removed).

See [docs/evaluation.md](evaluation.md#script-and-repl-commands) for details.

---

## Running `cljs.test` suites (`light-cljs test`)

`light-cljs test` discovers the test namespaces of the source roots, compiles a runner for them, and runs it in an embedded V8 isolate. No runner namespace, Node, or JVM is needed:

```sh
light-cljs test --source-root src --source-root test    # every namespace ending in -test
light-cljs test                                         # :source-paths of light-cljs.edn
light-cljs test app.math-test app.util-test             # just these namespaces
light-cljs test --ns-regexp '^app\.(math|util)-test$'   # by pattern
```

- **Selection:** a `NAMESPACE` argument runs that namespace and must name a `.cljs`/`.cljc` file under a root. `--ns-regexp REGEX`, named after shadow-cljs's `:ns-regexp`, also runs every namespace whose name REGEX finds a match in. REGEX is a JavaScript regular expression, the dialect of `#"..."`. With neither, the pattern is `-test$`. A namespace's name comes from its path, as `require` resolves it: `app/math_test.cljs` is `app.math-test`. Hidden entries, entries whose name has a hyphen (which no `require` resolves), and directories reached through a symbolic link are skipped. If nothing matches, the command reports the pattern and the roots, and exits nonzero.
- **Source roots:** `--source-root DIR` (repeatable, relative to the current directory). Without it, the command finds `light-cljs.edn` the way `compile` does and uses its top-level `:source-paths`, `:cache-root`, `:cache-retention` and `:cache-blockers`. It never reads a build.
- **Runner:** the command writes two namespaces under `.light-cljs/test-runner` in the current directory, or `<:cache-root>/test-runner` when the roots come from `light-cljs.edn`. It does so even with `--cache-dir` or `--no-cache`, and two runs in one project at once share these files. `light-cljs.test-console` turns on console printing before any test namespace loads. `light-cljs.test-runner` requires the selected namespaces, records the result through a `(defmethod cljs.test/report [:cljs.test/default :end-run-tests] ...)`, and calls `(cljs.test/run-tests ...)` on them. The entry is compiled like `--compile --entry` at the default `prepare` level, with the persistent compiler cache (`.light-cljs/cache`, or `<:cache-root>/cache` from the file). On a warm rerun, unchanged namespaces are installed from the cache.
- **Run and exit status:** `cljs.test`'s report goes to stdout as the tests run. `console.error` and `console.warn` go to stderr. The command exits 0 only when `(cljs.test/successful? summary)` holds. Any of the following makes it exit nonzero: a failed assertion, an error in a test, an exception while a namespace loads, a promise rejection with no handler, or a compile error.
- **What runs:** synchronous tests, and promise-based tests (`^:async deftest` with `await`, or `async` with a `done` that a promise chain calls). The isolate's promise jobs are drained after the program runs. The isolate is ECMAScript only, with a `console` but no timers (`js/setTimeout`), DOM, or Node APIs. A test that calls one fails with that name's `ReferenceError`. If the run ends while a test still waits on something other than a promise, the command reports that the run did not finish. A promise rejection with no handler is reported in place of `tests failed`, with the same nonzero status. Tests that need timers, the DOM, Node, or npm packages belong in a Node or browser test build.
- **Flags:** `--cache-dir DIR` or `--no-cache`, `--cache-blocker NS`, `--cache-retention N`, the `--macro-*` limits, `--config KEY=EDN`, `--elide-asserts`, `--jobs N` and `--stats` work as they do for `--compile --entry`. `--timeout-ms N` (10000 by default) and `--memory-limit N` (67108864 bytes by default) bound the run, as they bound `--eval`. Output flags such as `--optimize` are refused.

---

## Direct compilation (`--compile`)

```sh
# Single file, stdin, or --text: emits one JS expression returning the last form
light-cljs --compile examples/closure.cljs \
  | node --print 'eval(require("fs").readFileSync(0, "utf8"))'
# 10

# Entry namespace and its dependencies from explicit source roots
light-cljs --compile --entry demo.main --source-root examples/namespaces | node
# 4
light-cljs --compile --entry demo.main --source-root examples/macros | node
# 12
```

### JavaScript packages and `--esm`

String libspecs such as `["react" :as r]` read host-supplied packages from `globalThis.lightCljsPackages`. Passing `--esm` with `--compile --entry` emits a single ES module for a bundler (Vite, esbuild, Rollup) with static package imports and the entry value as the default export. See [JavaScript packages](compiler.md#javascript-packages).

### Code splitting (`--out-dir DIR`)

Adding `--out-dir DIR` to `--compile --entry` splits a program at its `light.lazy/loadable` sites (`--out-dir` implies `--esm`):

- Writes the startup chunk `NS.js` (default export = entry value), lazy chunks loaded on demand via `import()` when `light.lazy/load` requests them, and `chunks.json` describing each file's imports, namespaces, moved definitions, roots, loadable sites, and size.
- Chunks follow namespaces. At `--optimize advanced`, side-effect-free definitions, constant-table entries (keywords, symbols, constant collections), and runtime units move into the chunk of the roots that demand them; empty chunks are omitted.
- Works at every `--optimize` level, leaves unrelated files in `DIR` untouched, rejects source-map flags, and writes a single `--esm` file when there are no loadables.
- With `--size-report`, prints per-chunk byte counts, definitions, constants, and runtime units with what first demanded each.

```sh
light-cljs --compile --optimize advanced --entry app.main \
  --source-root examples/features --out-dir /tmp/light-features
node -e 'import("/tmp/light-features/app.main.js").then(m => m.default())'
```

See [Chunked builds](compiler.md#chunked-builds) and [docs/code-splitting.md](code-splitting.md).

### Reloadable dev builds (`--reloadable --out-dir DIR`)

Writes a reloadable build into `DIR`: `runtime.js`, `client.js`, one unit + source map per namespace in dependency order, `manifest.json`, and `index.js` (loads via `<script>` in browsers or `require` in Node). Definitions live in a registry read on each call, so re-evaluating a namespace unit updates definitions while preserving `defonce` state.

```sh
light-cljs --compile --entry app.main --source-root examples/dev \
  --reloadable --out-dir /tmp/light-dev
node -e 'require("/tmp/light-dev/index.js"); __lightDevLoaded.then(start => start())'
```

See [Dev builds](compiler.md#dev-builds) and [docs/dev-workflow.md](dev-workflow.md).

### Flag-only `--watch`, `--serve`, connected REPL, and nREPL

```sh
light-cljs --watch --entry app.main --source-root examples/dev --out-dir /tmp/light-dev --serve 9630 --nrepl 0
```

- **`--watch [--poll-ms N]`** writes a reloadable dev build and polls sources (default `100` ms). Recompiles changed namespaces incrementally against the live program, adds newly required units, recompiles dependents of modified macro namespaces or `light.build`-read files, recompiles `^:dev/always` namespaces and their dependents on every step, and preserves the previous unit on compile errors. Prints one status line per step to stderr.
  - Starts from the persistent cache (`.light-cljs/cache` or `--cache-dir DIR`) in a single V8 macro host; with `--no-cache`, writes the startup build before loading the watch session. Only startup reads or writes the cache; deleting the cache directory leaves a running watch intact.
  - Accepts `--cache-blocker NS` (repeatable) and the macro limit flags (`--macro-expansions`, `--macro-nodes`, `--macro-timeout-ms`, `--macro-memory-limit`, `--macro-poll-budget`). Expansion and node budgets reset per watch step; deadline and poll budget reset per namespace reload. Macros see stub JS packages.
- **`--serve PORT [--serve-host HOST] [--reload-dependents]`** serves the build at `http://127.0.0.1:PORT/` (default fallback console page at `/` if no `index.html` exists), serves source-mapped files, and pushes reload events over SSE. `client.js` runs `^:dev/before-load` and `^:dev/after-load` hooks, preserves `defonce` state, skips `^:dev/once` namespaces on reload, displays an error/warning overlay with locations, catches up late connections, and reconnects across restarts. `--reload-dependents` reloads all downstream namespaces (shadow-cljs `:full` strategy). Note: connected pages execute pushed code, so bind non-loopback hosts only on trusted networks.
- **`--repl --connect HOST:PORT`** connects a terminal REPL to a running dev server: compiles forms in the watch's program, evaluates them in the most recently connected browser or Node runtime, maps runtime stack traces to source locations, preserves `*1`/`*2`/`*3`/`*e`, and returns non-finite values as numbered references (`;; ref 1` on stderr) inspectable with `(light.dev/ref 1)` and `(light.dev/inspect 1)`.
- **nREPL (`--nrepl PORT`, `--no-nrepl`):** editors can connect over nREPL on the HTTP port (unless `--no-nrepl`) or on `--nrepl PORT` (`0` picks a free port and writes `.nrepl-port`). Supports `clone`, `close`, `describe`, `eval` (with `ns`), `load-file`, `interrupt`, `completions`, `lookup`, and `ls-sessions`; completions and lookups are answered directly from the watch's compiler metadata.

See [docs/evaluation.md](evaluation.md#watch) for full watch, serve, connected REPL, and nREPL details.

---

## Optimization, defines, assertions, and source maps

### `--optimize none|prepare|advanced`

Applies to `--compile` (single files, `--entry`, `--esm`, and `--out-dir`):

- **`none`** — emits every definition as analyzed.
- **`prepare`** *(default)* — removes unused inert definitions and specializes top-level calls.
- **`advanced`** — release mode (tracked in [docs/advanced-optimization.md](advanced-optimization.md)):
  - Removes unused definitions whose initializers only call pure core ops (without letting retained `js*` block optimization).
  - Embeds only reachable runtime units, drops namespace reflection metadata unless used (`*ns*`, `find-ns`, `read-string`), and drops the printer unless the program prints.
  - Deduplicates keywords, symbols, and constant collections with compile-time hashes and precomputed map key shapes.
  - Infers types across closed functions, core HOFs (`map`, `reduce`, `swap!`), closed atoms, and closed protocol methods to emit plain JS truth tests, arithmetic, comparisons, and typed ops.
  - Prunes unused protocol methods, types, and collection/seq runtime variants.
  - Calls closed functions directly without arity dispatchers and hoists capture-free local closures.
  - Folds constants, dead branches, `goog-define` values, and constant lookups; inlines single-use and small pure functions.
  - Minifies names, strips comments and whitespace.

See [Application preparation](compiler.md#application-preparation).

### Additional emission and analysis flags

| Flag | Behavior |
| --- | --- |
| `--pseudo-names` | Keeps `advanced` output human-readable (compiler names, intact runtime comments, indented layout). |
| `--size-report` | Prints embedded runtime units (with sizes and first-demanding expressions) and constant-table size to stderr. |
| `--define ns/NAME=VALUE` | Repeatable. Overrides a `(goog-define NAME default)` Var at every optimization level; dead branches are eliminated at `advanced`. |
| `--elide-asserts` | Omits all `assert` and `fn` `:pre`/`:post` checks at every level (and binds `*assert*` to `false` during macro expansion). `--define cljs.core/*assert*=false` is rejected. |
| `--load-tests`, `--no-load-tests` | Whether `cljs.test/deftest` defines its test: `--no-load-tests` makes every `deftest` expand to `nil`, as upstream's `:load-tests false` does. Tests load by default; `release` omits them unless `--load-tests` or `:compiler-options {:load-tests true}`. With `--compile`, `--watch` and the `compile`, `watch` and `release` commands; the last of the two flags wins. |
| `--config KEY=EDN` | Repeatable (with `--compile --entry`, `--reloadable`, `--watch`). Exposes build config values to macros via `(light.build/config :KEY)`. Keys are written without a leading colon; values are finite EDN literals (no symbols or tagged literals; duplicate keys are an error). See [Compile-time services](compiler.md#compile-time-services-lightbuild). |
| `--runtime-key KEY` | Derives the build's runtime isolation key from `KEY` so two builds of the same program can coexist on one page. Rejected on dev builds. |
| `--source-map PATH` | Writes a Source Map v3 to `PATH` (at any `--optimize` level, including `--esm`) and appends `//# sourceMappingURL=`. Maps expressions, macro expansions (to the call site), renamed `advanced` symbols, and runtime frames (`light-cljs/js_runtime.js`). Paths are relative to `PATH`'s directory. |
| `--source-map-inline` | Embeds the source map as a data URL comment (paths relative to cwd). |
| `--source-map-include-sources` | Embeds source text inside the source map. |
| `--warnings` | Place before the mode flag (`--warnings --compile ...`) to report undeclared Vars and arity conflicts as warnings rather than errors. |
| `--checked-arrays-warn` / `--checked-arrays-error` | Warn or error on unchecked array operands. |

Source map example:

```sh
light-cljs --compile --optimize advanced app.cljs --source-map app.js.map > app.js
node --enable-source-maps app.js
```

### Macro limits

Default limits per compilation: `--macro-expansions 1024`, `--macro-nodes 100000`, `--macro-timeout-ms 10000`, `--macro-memory-limit 67108864` (64 MiB), `--macro-poll-budget 1000000`. Raise these flags on `--compile` or `--watch` for macro-heavy programs.

---

## Persistent compiler cache

`--compile --entry` (including `--reloadable`, `--esm`, and `--out-dir` at every `--optimize` level) and `--watch` use a persistent cache enabled by default in `.light-cljs/cache` (override with `--cache-dir DIR`; disable with `--no-cache`). Combining `--no-cache` with `--cache-dir` is an error.

- **Namespace caching:** a namespace whose source, resolution, dependencies, `light.build` inputs, compiler options, `--config` map, and compiler version are unchanged is installed from its cached artifact without re-running its macros.
- **Output caching:** when every namespace has a cached artifact and emission options match (chunked builds, or single-file builds without source maps or `--size-report`), the cached JS output is written directly without re-running preparation or emission.
- **Determinism & opt-outs:** for macros obeying the [cache contract](compiler-cache.md#cache-contract), cached output is byte-for-byte identical to an uncached build. Macros with cross-namespace compile-time side effects do not re-run on a hit; declare them with `--cache-blocker NS` (repeatable) to bypass caching for `NS` and its compilation dependents. Namespace metadata `^:dev/always` bypasses caching for the namespace and its consumers; `^:light-cljs/no-cache` bypasses only that namespace.
- **Retention:** the cache directory is disposable derived data (add `.light-cljs/` to `.gitignore`), bounded by default to 3 artifacts per namespace, 3 cached outputs, and 1 GiB total. Pass `--cache-retention N` (positive integer) when sharing one cache across more than 3 build configurations.
- **Interactive vs. clean isolation:** cache keys distinguish interactive (`--watch`) from clean (`--compile`) builds so both can safely share one cache directory.

On Penpot, a warm no-op build takes ~7% of an uncached build's time, and cold cache population adds ~10% ([benchmarks/compiler-cache.md](../benchmarks/compiler-cache.md)). Embedders can use `light_cljs::compiler::analyze_namespace_entry_cached` or `light_cljs::dev_session::DevSession::compile_entry`.

---

## Build statistics and tracing (`--stats`, `--stats-file`)

Passing `--stats` to `--compile --entry`, `--reloadable`, or `--watch` prints structured diagnostics to stderr without changing build output (`light-cljs-stats 1`, followed by `key value` lines and tables). `light_cljs::compiler::analyze_namespace_entry_with_stats` exposes the same data as `CompileStats`.

- **Phase & macro counters:** wall times (source resolution/parsing, analysis + macro expansion, deferred units, application preparation, emission, writing, plus session loading in `--watch`), namespace/macro/form-node/deferred-unit counts, and a `[macro-calls-by-namespace]` table of the top 20 namespaces.
- **`trace.*` counters:** exclusive stage times (nested work counted once) covering loading, top-level passes, deferred units, drain bookkeeping, V8 time, macro-host units, and per-namespace drain ready width.
- **`memory.*` counters:**
  - `memory.peak-rss-bytes` — process peak RSS (`getrusage`, macOS/Linux; whole session for `--watch`).
  - `memory.parsed-trees.peak-bytes` / `-files` — estimated peak parsed trees held simultaneously before release after analysis.
  - `memory.queued-ir.peak-bytes` / `-units` — estimated peak analyzed deferred-unit IR queued waiting to merge.
  - `memory.live.artifacts`, `memory.live.engines`, `memory.live.engine-handles`, `memory.live.bridge-payloads` — live counts at report time (all `0` after `--compile` drops its macro host; also readable via `light_cljs::compile_stats::live_counts`). In `--watch --stats`, each step prints a `live ... value-roots=N` line after a full GC of session engines.
- **`cache.*` counters:** `cache.hits`, `cache.misses` (broken down by `cache.miss.KIND`), `cache.bypasses` (by `cache.bypass.REASON`), bytes read/written, validation/decode/write timings, `cache.output.reused` / `cache.output.stored`, and a `[cache-misses-and-bypasses]` table (up to 20 rows).
- **`pool.*` counters (with `--jobs`):** worker count, workers with V8 engines, engine-less units analyzed, pool time/utilization, jobs, claims, steals, parks, peak queue depth, and lock contention.
- **`--stats-file PATH`** (implies `--stats`) writes the exclusive trace as TSV (`stage`, `namespace`, `unit`, `execute`, `read` rows) for `benchmarks/concurrency/dag.bb` ([docs/compiler-concurrency.md](compiler-concurrency.md)).

---

## Parallel compilation

`--jobs N` (experimental) with `--compile` grants `N` CPU work slots including the calling thread (at most `N - 1` worker threads). The default is `1`: on Penpot, 8 jobs take ~0.79× wall time at ~1.8× peak RSS (missing the 0.5× / 1.5× gate in [benchmarks/compiler-concurrency.md](../benchmarks/compiler-concurrency.md#stage-a-gate-light-cljs-343-october-19-2026)), though programs with hundreds of definitions per namespace reach ~0.45×.

- **What runs in parallel:** source reading/parsing and analysis of each namespace's deferred definition bodies. All other phases run on the calling thread, which drains results in deterministic source order.
- **Determinism & stateful macros:** warnings, errors (first failing unit in source order; remaining work cancelled), `--macro-expansions`, and `--macro-nodes` match `--jobs 1`. Because each worker isolate has its own compile-time state, cross-namespace macro side effects are invisible across isolates unless the provider is declared with `--cache-blocker NS` (which pins `NS` and all namespaces referencing it to the coordinator isolate in source order; `^:light-cljs/no-cache` does not provide this affinity).
- **Memory budgets (`--macro-memory-limit`):** sets the combined budget for macro isolate heaps and fixed-length buffers. The coordinator reserves at least 64 MiB (or the full limit if smaller), and each macro-capable worker isolate requires at least 64 MiB—so the default 64 MiB limit leaves workers engine-less (handling only macro-free units). Isolates can grow past base shares into unused headroom; fully cached builds release unused worker shares to the coordinator.
- **Data-reader isolates:** started lazily on first custom tag, each with an independent 16 MiB starting allowance growing up to `min(64 MiB, --macro-memory-limit)` outside the macro budget.
- **Timeouts & poll budgets:** `--macro-timeout-ms` is a single build-wide deadline; `--macro-poll-budget` is per isolate. Timeout and memory-limit messages may depend on scheduling order.

See [docs/compiler-concurrency.md](compiler-concurrency.md#the-scheduler).

---

## Inspecting intermediate stages

```sh
light-cljs --text '(+ 1 2)'                  # raw tokens with byte spans
light-cljs --parse --text '(+ 1 2)'          # syntax arena
light-cljs --analyze --text '(let* [x 10] x)' # resolved Program IR
```
