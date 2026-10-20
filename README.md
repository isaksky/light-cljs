# light-cljs

light-cljs is a ClojureScript-to-JavaScript compiler written in Rust. It reads
`.cljs`/`.cljc` source, resolves and analyzes it in Rust, and emits a single
self-contained JavaScript expression that carries its own persistent-collection
runtime. The same compiler powers a script runner, a REPL and an embedding API
that execute the emitted JavaScript in an embedded QuickJS engine (via
[`rquickjs`](https://crates.io/crates/rquickjs) 0.14.0), so no Node, JVM or
installed `cljs.core` is needed to run code.

The project site, <https://isaksky.github.io/light-cljs/>, has an overview and
the [browser playground](https://isaksky.github.io/light-cljs/playground/).

**Status:** ClojureScript support is substantial but partial. Core special forms,
macros, namespaces, persistent and sorted collections, lazy sequences,
transducers, protocols, types, records, multimethods, atoms, exceptions and
JavaScript interop work, and behavior is regularly compared against standard
ClojureScript. It is not a drop-in replacement: there is no Google Closure
Library, no `cljs.pprint`/`core.async`/`test.check`, no package resolution (npm
packages come from the host or a bundler), and numbers are always JavaScript
Numbers. See [known boundaries](#known-boundaries).

## Architecture

```text
source ─▶ Tokenizer ─▶ Parser ─▶ compiler reader ─▶ Analyzer ─▶ application prep ─▶ Emitter ─▶ JavaScript
          (lib.rs)    (parser.rs) (compiler_reader.rs) (analyzer.rs) (application.rs)   (emitter.rs)  + js_runtime.js
                                         ▲                    │
                                         └── macro expansion ◀┘ compiled to JS, run in an embedded engine
```

- **Tokenizer** (`src/lib.rs`) borrows the source and yields raw tokens with byte
  spans; it never allocates text. See [docs/lexer.md](docs/lexer.md).
- **Parser** (`src/parser.rs`) builds a flat node arena with byte spans and
  validates literal spelling; it performs no reader expansion. See
  [docs/parser.md](docs/parser.md).
- **Compiler reader** (`src/compiler_reader.rs`, `compiler_data.rs`) turns syntax
  into immutable compiler forms: reader conditionals, syntax quote, `#()`,
  namespaced maps, tagged literals and namespace-aware symbol resolution.
- **Analyzer** (`src/analyzer.rs`) resolves names to stable binding IDs and
  lowers forms into an expression arena (`Program`). Binding/destructuring
  macros are expanded in Rust (`compiler_binding_macros.rs`,
  `compiler_destructuring.rs`). Every core operation (`CoreFnId`) carries
  parameter and return types, purity and its runtime units from
  `src/core_operations.tsv` (`src/core_operations.rs`).
- **Macros** run as JavaScript emitted by this same compiler, inside a separate
  heap: V8 for native clean compilations, QuickJS for interactive sessions and
  WebAssembly (`macro_host.rs`, `compiler_evaluator.rs`). Core macros
  are bootstrapped from `src/compiler_core_macros.cljs` and
  `src/compiler_sequence_macros.cljs`; user macros are ordinary `defmacro`s in
  `.cljs`/`.cljc` files. Ordinary helpers that macros call are compiled on demand
  (`compilation_session.rs`).
- **Application preparation** (`src/application.rs`) conservatively removes
  unused inert definitions and specializes some calls, preserving effect order.
- **Emitter** (`src/emitter.rs`) writes the JavaScript expression; the shared
  runtime is `src/js_runtime.js`, split into units with declared dependencies
  (`src/runtime_manifest.rs`). Bundled `clojure.string`, `clojure.set`,
  `clojure.walk`, `clojure.edn`, `cljs.reader` and `light.lazy` live in
  `src/compiler_libraries/`.
- **Evaluation** (`src/eval_session.rs`, `eval_compiler.rs`, `eval_cli.rs`)
  compiles forms incrementally and executes them in a persistent V8 isolate, or
  a QuickJS heap with `--engine quickjs` and in builds without V8
  (`src/v8_session.rs`, `src/evaluator.rs`).

## Building

The default native build requires Rust 1.91+, babashka (`bb`), Git, curl, tar,
Python 3, a C/C++ toolchain and libclang. The first build on a machine compiles a pinned V8
source patch into a per-user cache; later builds in any checkout, worktree or clean
target directory reuse it (`LIGHT_CLJS_V8_CACHE=require` fails instead of rebuilding). See [engine build details](vendor/v8/README.md),
including `LIBCLANG_PATH` setup on macOS. Node.js is needed for parts of the test
suite and for running emitted JavaScript outside the embedded evaluator.

```sh
cargo build --release
```

`cargo build --release --no-default-features` selects the QuickJS-only compiler
and needs Rust and a C compiler. WebAssembly builds embed no JavaScript engine:
their embedder runs the macros (`host_evaluator::ScriptHost`, as the browser
playground does). The native
V8 backend runs the macros of clean compilations and the forms of `--eval` and
`--repl` (whose macros stay in QuickJS); generated applications carry the same
JavaScript runtime.

## Command-line usage

Run `cargo run -- --help` for the full option list.

### Evaluate scripts and use the REPL (embedded V8 or QuickJS)

```sh
cargo run --quiet -- --eval --text '(/ 1 2)'
# 0.5
cargo run --quiet -- --eval --source-root examples/native examples/native/demo.cljs
# Hello from compiled JavaScript
# {:squares [1 4 9 16 25], :sum 15}
printf '(+ 20\n 22)\n(* *1 2)\n' | cargo run --quiet -- --repl
# 42
# 84
```

`--eval` takes a `.cljs`/`.cljc` file, `-`/stdin, or `--text SOURCE`. Program
output streams immediately; the printed final value appears only after success.
`--repl` keeps definitions, namespaces and `*1`/`*2`/`*3`/`*e`, accepts
multiline input and recovers from reader and language errors (prompts go to
stderr). Namespaces are loaded only from explicit `--source-root DIR` options
(`.cljs` is preferred over `.cljc`); `.clj` files are rejected. Resource limits:
`--poll-budget`, `--memory-limit`, `--stack-limit`, `--timeout-ms`,
`--source-limit`, `--output-limit`, plus `--print-length`/`--print-level` for the
printed result. `--engine quickjs` runs the forms in QuickJS instead of V8,
the default where the build has V8.
`--warnings` analyzes as `--compile --warnings` does, printing
undeclared Vars and arity conflicts to stderr instead of rejecting the form, so
`(require 'app.view :reload)` of a file that uses a Var another file defines
later still loads. Details: [docs/evaluation.md](docs/evaluation.md#script-and-repl-commands).

### Compile to JavaScript

```sh
# A single file, stdin or --text: emits one JS expression whose value is the last form
cargo run --quiet -- --compile examples/closure.cljs \
  | node --print 'eval(require("fs").readFileSync(0, "utf8"))'
# 10

# A namespace and its dependencies from explicit source roots
cargo run --quiet -- --compile --entry demo.main --source-root examples/namespaces | node
# 4
cargo run --quiet -- --compile --entry demo.main --source-root examples/macros | node
# 12
```

String libspecs such as `["react" :as r]` read host-supplied packages from
`globalThis.lightCljsPackages`. Add `--esm` to `--compile --entry` to emit one ES
module for a bundler instead, with static package imports and the entry value as
its default export (see [JavaScript packages](docs/compiler.md#javascript-packages)).

Add `--out-dir DIR` to `--compile --entry` to split a program at its
`light.lazy/loadable` sites: `DIR` gets one ES module per chunk (`--out-dir`
implies `--esm`), the startup chunk `NS.js` whose default export is the entry
value, the chunks loaded with `import()` the first time `light.lazy/load` asks
for a target in them, and `chunks.json`, which lists each file with its
imports, namespaces, moved definitions, roots, loadable sites and size.
Chunks follow namespaces, and at `--optimize advanced` definitions without
effects move to the chunk of the code that reads them, as do the keywords,
symbols and constant collections of the constant table and the runtime units
(each chunk embeds those only the roots that load it need, and
`--size-report` lists them per chunk), and a chunk left without code is not
written. It works at every
`--optimize` level, writes only the files the manifest names, takes no source
map flags, and leaves a program without loadables one file, its `--esm` output
([Chunked builds](docs/compiler.md#chunked-builds),
[Code-splitting contract](docs/compiler.md#code-splitting-contract)).

```sh
cargo run --quiet -- --compile --optimize advanced --entry app.main \
  --source-root examples/features --out-dir /tmp/light-features
node -e 'import("/tmp/light-features/app.main.js").then(m => m.default())'
# menu: Editor | Reports
# editor: 4 documents, first Introduction, ids #{:api :faq :intro :setup}
# reports: tags Draft, Guide, Reference; drafts FAQ, INTRO; 1050 words
```

Add `--reloadable --out-dir DIR` to `--compile --entry` to write a dev build
into `DIR` instead: `runtime.js` (the runtime, once), one unit per namespace in
dependency order with its source map, `manifest.json` and an `index.js` that
loads them in order (script elements in a page, `require` in Node). Definitions
live in a registry read at each use, as in the REPL, so evaluating a namespace's
unit again replaces its definitions while `defonce` state stays
([Dev builds](docs/compiler.md#dev-builds),
[docs/dev-workflow.md](docs/dev-workflow.md)).

```sh
cargo run --quiet -- --compile --entry app.main --source-root examples/dev \
  --reloadable --out-dir /tmp/light-dev
node -e 'require("/tmp/light-dev/index.js"); __lightDevLoaded.then(start => start())'
# start
# clicks: [1]
```

`--watch --entry NS --source-root DIR --out-dir DIR [--poll-ms N]` writes the
same dev build, then polls the sources (every 100 ms by default) and, for each
saved namespace, compiles its forms again against the program it keeps and
rewrites the namespace's unit, its source map and the manifest: a new require
adds the units of the namespaces it loads, a change to a macro namespace or
to a namespace its macros require recompiles the namespaces that use its
macros, a change to a file a macro read through `light.build` recompiles the
namespaces that read it, every step recompiles the namespaces marked
`^:dev/always` and those that depend on them, and a form that fails prints its
error and leaves the previous unit. The watch starts from the persistent
compiler cache (below), in `.light-cljs/cache` or in `--cache-dir DIR`: its
startup build runs once, in
the session's own engine, installing every namespace whose artifact is
current instead of analyzing it, and the session compiles later edits
against that build as if it had loaded the namespaces itself. Steps neither
read nor write the cache, so a restart analyzes what changed since the
cache was last filled; a fresh uncached start is a restart with
`--no-cache`, and deleting the directory never touches a running watch.
`--cache-blocker NS` (repeatable) keeps NS and its compilation consumers
out of the cache; a blocker never makes a step recompile. A large program
raises the macro limits with the flags `--compile` takes,
`--macro-expansions N`, `--macro-nodes N`, `--macro-timeout-ms N`,
`--macro-memory-limit N` and `--macro-poll-budget N` (by default 1,024
expansions and 100,000 generated form nodes per compilation, a 10 s deadline,
a 64 MiB heap and a poll budget of 1,000,000): they bound the startup build
and the watch's session, whose load of the program at startup is one
operation under the deadline and poll budget, as is each namespace a step
compiles again. As with `--compile`, macros see stub JavaScript
packages, which only the application has. One line per step goes to stderr
([Watch](docs/evaluation.md#watch)).

```sh
cargo run --quiet -- --watch --entry app.main --source-root examples/dev --out-dir /tmp/light-dev
# light-cljs: built app.main (4 namespaces) into /tmp/light-dev in 281 ms; watching every 100 ms
# light-cljs: recompiled app.view in 4.1 ms
```

Add `--serve PORT` to `--watch` to serve the build at `http://127.0.0.1:PORT/`
(`--serve-host HOST` for another address; a connected page runs what the server
sends it, so open it to a network only when you trust it): a page with the
console output at `/` unless the build directory has an `index.html`, the
build's files and the sources its source maps name, and an event stream that
pushes each step to the open pages. The build's `client.js` loads the pushed
units between the `^:dev/before-load` and `^:dev/after-load` hooks, keeping
`defonce` state, draws compile errors and warnings over the page with file,
line and column, catches up when it connects late, and reconnects when the
server restarts. A step loads the namespaces it recompiled (a changed macro
namespace recompiles those that use it, and every step those marked
`^:dev/always` with those that depend on them), never again one marked
`^:dev/once`; `--reload-dependents` also reloads every
namespace downstream of them, as shadow-cljs's `:full` strategy does.
JavaScript packages come from a page in the build directory
that sets `globalThis.lightCljsPackages` before it loads `index.js`
([Serve](docs/evaluation.md#serve), [JavaScript packages in
dev](docs/evaluation.md#javascript-packages-in-dev)).

```sh
cargo run --quiet -- --watch --entry app.main --source-root examples/dev --out-dir /tmp/light-dev --serve 9630
# light-cljs: serving /tmp/light-dev at http://127.0.0.1:9630/
# light-cljs: built app.main (4 namespaces) into /tmp/light-dev in 281 ms; watching every 100 ms
```

With the server running, `--repl --connect HOST:PORT` is a REPL in the page:
it reads forms as `--repl` does, the server compiles each with the watch's
program (so REPL definitions and saved files see each other) in the
connection's namespace, the entry namespace to begin with, and the most
recently connected page or Node runtime runs it, prints the value with
`--print-length` and `--print-level`, and keeps `*1`, `*2`, `*3` and `*e`.
Runtime errors come back with their stacks mapped to the sources. A value that
is not finite data also comes back as a reference (`;; ref 1` on stderr),
which `(light.dev/ref 1)` returns and `(light.dev/inspect 1)` describes;
`(light.dev/runtimes)` lists the connected runtimes and
`(light.dev/select! ID)` sends the forms to another
([Connected REPL](docs/evaluation.md#connected-repl)).

```sh
cargo run --quiet -- --repl --connect localhost:9630
# app.main=> (swap! app.state/state update :clicks inc)
# {:clicks 2, :card #app.view.Card{:label "clicks", :clicks 1}}
```

Editors connect over nREPL to the same port, or to the port of `--nrepl PORT`
(0 for any free one), which the watch writes into `.nrepl-port` in the working
directory while it runs. An nREPL session is such a REPL; the server answers
`clone`, `close`, `describe`, `eval` (with `ns`), `load-file`, `interrupt`
(which stops waiting for the runtime), `completions`, `lookup` and
`ls-sessions`, completions and lookups from the definitions and metadata of the
watch's program without a runtime round trip ([nREPL](docs/evaluation.md#nrepl)).

```sh
cargo run --quiet -- --watch --entry app.main --source-root examples/dev --out-dir /tmp/light-dev --serve 9630 --nrepl 0
# light-cljs: serving /tmp/light-dev at http://127.0.0.1:9630/
# light-cljs: nREPL at 127.0.0.1:52411, its port in /work/.nrepl-port
```

`--optimize none|prepare|advanced` after `--compile` (single files and `--entry`
builds, with `--esm` or `--out-dir` too) picks the optimization level. `prepare`, the
default, removes unused definitions with inert initializers and specializes
top-level calls; `none` emits everything as analyzed. `advanced` is the release
mode, still being built from
[docs/advanced-optimization.md](docs/advanced-optimization.md). So far it:

- removes unused definitions whose initializers only call pure core operations,
  and does not let retained `js*` disable these passes;
- embeds just the runtime units the program reaches and reads runtime functions
  directly; registers namespaces with the runtime only for programs that use
  reflection such as `*ns*`, `find-ns` or `read-string`; keeps the printer (and
  with it a printing `toString` on collections) only for programs that print;
- builds each distinct keyword, symbol and constant collection literal once,
  with its hash computed by the compiler, and builds map literals with constant
  keys from a precomputed key shape;
- infers types, of parameters too from the calls of functions only the program
  calls or passes to core operations such as `map`, `reduce` and `swap!` (with
  the values stored in atoms only the program uses, which `deref` and `swap!`
  then return), and of protocol methods
  only the program calls, to write truth tests,
  arithmetic, comparisons and typed core calls as plain JavaScript;
- drops the methods and types no instance can use, and keeps of the runtime's
  collection, keyword and seq types only those the program can build, with the
  parts of each operation that handle them;
- calls the program's own functions directly through the method a call selects,
  without an arity dispatcher when nothing else sees them, and creates once the
  methods of local functions that capture nothing;
- folds constants, dead branches, `goog-define` values and lookups in constant
  collections, and inlines single-use and small pure functions;
- writes the program and the runtime it embeds tersely, with short names and
  without comments or whitespace.

See [Application preparation](docs/compiler.md#application-preparation) for
the passes and [known boundaries](#known-boundaries) for typical output sizes.
Add `--pseudo-names` to keep `advanced` output readable for debugging: the
compiler's own names, the runtime as written and an indented layout. Add
`--size-report` to print to stderr which runtime units the output embeds, with
their sizes and the source expression that first demanded each, and the size of
the constant table. `--define ns/NAME=VALUE` (repeatable) gives a
`(goog-define NAME default)` Var its value for the build, at every level; at
`advanced` the code that only another value runs, such as a `(when DEBUG ...)`
logging path, is removed. `--elide-asserts` omits every `assert` and `fn`
`:pre`/`:post` check at every level, as upstream's `:elide-asserts` does:
`(assert (= 1 1) "x") (js/console.log 1)` is 151 bytes at `advanced`, the same
as without the assertion, instead of 309. Source macros read it as a false
`*assert*` while they expand. It is the only switch; `--define`
names only `goog-define` Vars, so `--define cljs.core/*assert*=false` is
rejected. `--eval` and `--repl` take `--elide-asserts` too.

`--stats` with `--compile --entry`, `--reloadable` or `--watch` prints to
stderr what the build did, without changing its output: the wall time of each
phase (source resolution and parsing, analysis with macro expansion, draining
deferred units, application preparation, emission and writing; for `--watch`
also loading its session), the namespaces compiled, the macro calls executed,
the form nodes they generated, the deferred units registered, analyzed,
prepared and executed, and the 20 namespaces with the most macro calls. Each
counter is one `key value` line, after a `light-cljs-stats 1` line, and the
table follows a `[macro-calls-by-namespace]` line, one `NAMESPACE COUNT` row
each; `light_cljs::compiler::analyze_namespace_entry_with_stats` returns the
same numbers as a `CompileStats`. The `trace.*` counters ahead of the table
come from a trace of *exclusive* stage times, in which nested work counts
once: loading, the top-level passes without the units they demand, each
deferred unit without the units it demands, the drains' bookkeeping, the
V8 time of each, the units that entered the macro host and the ready width of
each namespace's drain. The `memory.*` counters come last:
`memory.peak-rss-bytes`, the process's peak resident set so far (from
`getrusage`, on macOS and Linux only; for `--watch` it covers the whole
session); `memory.parsed-trees.peak-bytes` and `-files`, the most parsed
sources the loader held at once, each from when it is parsed until its tree
is released after its namespace's analysis (its text, nodes, roots, child
lists and decoded strings); and `memory.queued-ir.peak-bytes` and `-units`,
the most analyzed deferred-unit IR waiting to merge into the program at
once, from when a unit's analysis ends, on whichever thread, until its
namespace's units merge after the next namespace's top-level pass. Both
peaks are estimates, table lengths times element sizes, without an
expression's own vectors. The last four, `memory.live.artifacts`,
`memory.live.engines`, `memory.live.engine-handles` and
`memory.live.bridge-payloads`, count what the process still holds when the
report is printed: namespace artifacts (captured or decoded for the cache),
JavaScript engines, persistent handles Rust holds into an engine's heap (V8
`Global`s), and the Rust provenance data an engine's objects own through the
form bridge. A `--compile` build has dropped its macro host by then, so all
four are 0; `light_cljs::compile_stats::live_counts` reads them from the
library. With `--watch --stats`, each step's line is followed by
`light-cljs: live artifacts=N engines=N engine-handles=N bridge-payloads=N value-roots=N`,
counted after a full garbage collection of the session's engines;
`value-roots` is how many values the session's macro engine roots for
the compiler in its private registry.
`--stats-file PATH` (which implies `--stats`) also
writes that trace to `PATH` as tab-separated `stage`, `namespace` and `unit`
rows (and `execute` and `read` rows for the compile-time roots evaluated
and read), with each unit's position, demander and costs, for
`benchmarks/concurrency/dag.bb` ([docs/compiler-concurrency.md](docs/compiler-concurrency.md)).

`--jobs N` with `--compile` is experimental. It allows the compilation N CPU
work slots, the calling thread's included, so at most N-1 worker threads.
The default is 1: on Penpot, eight jobs take 0.79x the wall time of one
with 1.8x the peak memory, short of the design's gate of 0.5x and 1.5x
([benchmarks/compiler-concurrency.md](benchmarks/compiler-concurrency.md#stage-a-gate-light-cljs-343-october-19-2026)),
while programs whose namespaces have hundreds of definitions reach about
0.45x. Sources are read and parsed on that pool, the source
order coming from the require edges and not from which file finished first;
and each namespace's deferred units (its definitions' bodies) are analyzed on
it, each worker in a V8 isolate of its own, while the calling thread drains
them in order. Every other phase still runs on the calling thread. Output
does not depend on N, except where a macro's compile-time effect in one
namespace's definition bodies is observed from another: each isolate has its
own compile-time state, so such an effect is invisible in the others (the
`counter` example of the design prints `1 1` with several jobs and `1 2`
with one). Declare such a provider with `--cache-blocker NS` (repeatable,
with or without `--cache-dir`): NS and every namespace that requires it,
directly or transitively, or names it in a symbol (such as a fully
qualified macro), analyze their definition bodies on the calling
thread's isolate in source order, as one job does, so the example prints
`1 2` at every N, while the other namespaces' units still go to the pool.
`^:light-cljs/no-cache` keeps a namespace out of the cache only and gives no
such affinity; an undeclared provider is treated as pure and nothing
detects its effects. Warnings and
errors do not depend on N either: warnings come in source order, and a
failed build reports the error the first failing unit in source order
gives, whichever worker failed first, after which the workers' remaining
work is cancelled and nothing is written. `--macro-expansions` and
`--macro-nodes` count each unit's expansions where the units are charged in
source order, so a limit fails in the unit and with the message one job
gives. `--macro-memory-limit` is one budget for the calling thread's isolate
and every worker's together: as many isolates run as the limit gives at
least 32 MiB each (a limit of less than 64 MiB leaves the workers without
engines), each is allowed a base share and takes more from what the others
have not taken, and one that cannot get what it needs fails the build.
`--macro-timeout-ms` is one deadline for the build, from the pool's start;
`--macro-poll-budget` is per isolate. A timeout or memory failure depends on
scheduling, so its message is not promised to be the same at every N. With
`--stats` the report adds `pool.*` lines: the workers, the pool's time and
utilization, the jobs, claims, steals and parks, the peak queue depth and the
lock contention. The pool is described in
[docs/compiler-concurrency.md](docs/compiler-concurrency.md#the-scheduler).

`--compile --entry` (also with `--reloadable`, `--esm` and `--out-dir`, at
every `--optimize` level) and `--watch` keep a persistent compiler cache, on
by default since light-cljs-328, in `.light-cljs/cache` under the current
directory, or in DIR with `--cache-dir DIR`, created when missing. A
namespace whose source, resolution, dependencies, `light.build` inputs,
options, `--config` map and compiler are unchanged is installed from its stored artifact instead of being analyzed,
so its macro calls do not run again; every other namespace is analyzed and
stored. For macros that follow the cache contract
([docs/compiler-cache.md](docs/compiler-cache.md#cache-contract)), the output
is byte for byte the uncached build's; a macro with side effects across
namespaces does not repeat them on a hit. `--cache-blocker NS` (repeatable)
keeps NS and every namespace whose compilation depends on it from reading or
writing the cache, as `^:dev/always` on a namespace does for it and its
consumers and `^:light-cljs/no-cache` for that namespace alone. With
`--no-cache` nothing reads or writes a cache: the build is a fresh uncached
one, and `--no-cache` with `--cache-dir` is an error. The directory is
disposable derived data, bounded to three artifacts per namespace and 1 GiB;
never commit it (add `.light-cljs/` to `.gitignore`). On Penpot a warm build
with nothing edited takes 7% of an uncached one's time and filling the cache
costs about 10% more
([benchmarks/compiler-cache.md](benchmarks/compiler-cache.md)). With
`--stats` the report adds `cache.hits`, `cache.misses` with
`cache.miss.KIND` per first differing key component (`source`,
`dependencies`, ...) or failure (`no-artifact`, `unreadable`, ...),
`cache.bypasses` with `cache.bypass.REASON`, bytes read and written and the
validation, decode and write times, then a `[cache-misses-and-bypasses]`
table of up to 20 rows each, such as `app.main miss source: source changed`
or `app.view bypass blocked app.macros > app.view`. Embedders call
`light_cljs::compiler::analyze_namespace_entry_cached`, or
`light_cljs::dev_session::DevSession::compile_entry` for a session. The
cache key names the macro engine, so `--watch`, whose session compiles with
QuickJS, and a V8 `--compile` keep separate artifacts in one directory.

`--config KEY=EDN` (repeatable) with `--compile --entry`, `--reloadable` or
`--watch` gives macros a build configuration value, which they read with
`(light.build/config :KEY)`: `--config app/flavor=:dev --config app/n=3`. KEY
is a keyword written without its colon, and EDN is keywords, numbers,
strings, booleans, nil and lists, vectors, maps and sets of them, read with
the compiler's reader; an unset key reads as nil. A malformed value, a
symbol, a tagged literal or a key given twice is an error. Use it for build
inputs such as a build time or a flavor instead of host state a macro would
read itself ([Compile-time services](docs/compiler.md#compile-time-services-lightbuild));
embedders pass the same map with `MacroHost::set_build_configuration` and
`EvalOptions::configuration`.

`--source-map PATH` writes a Source Map v3 of the output to `PATH`, at every
level and with `--esm` too, and ends the JavaScript with a
`//# sourceMappingURL=` comment naming the map's file, so write the JavaScript
next to it; the JavaScript before that comment is the output without the flag.
Each expression maps to its `.cljs` file, line and column, macro-expanded code
to the macro call, and renamed `advanced` locals and definitions to their
source names (the map's `names`); the embedded runtime maps to
`src/js_runtime.js` as the source `light-cljs/js_runtime.js`. Source paths are
relative to the map's directory. `--source-map-inline` embeds the map in the
comment instead (paths relative to the current directory), and
`--source-map-include-sources` adds every source's text. Node's
`--enable-source-maps` then reports a stack trace at the source:

```sh
cargo run --quiet -- --compile --optimize advanced app.cljs --source-map app.js.map > app.js
node --enable-source-maps app.js
# Error: boom 42
#     at explode (/path/to/app.cljs:2:10)
```

Put `--warnings` before the mode flag (`--warnings --compile ...`) for ClojureScript-style warnings (undeclared
Vars and arity conflicts become warnings instead of errors), or with
`--checked-arrays-warn`/`--checked-arrays-error` for array-operand checks.
Compilation errors report line/byte-column and produce no partial output.

### Inspect intermediate stages

```sh
cargo run --quiet -- --text '(+ 1 2)'             # tokens with byte spans
cargo run --quiet -- --parse --text '(+ 1 2)'     # syntax arena
cargo run --quiet -- --analyze --text '(let* [x 10] x)'  # resolved Program
```

## Browser playground

[playground/](playground/) is a static page that runs the compiler in the
browser, published at <https://isaksky.github.io/light-cljs/playground/>:
light-cljs built for `wasm32-unknown-unknown` compiles several in-memory
namespaces at any `--optimize` level, and the emitted JavaScript runs in the
browser's own engine inside a worker.
A dropdown offers examples from a single namespace to a calculator in several.
Its macros run in the browser's own engine too, about three times faster than
the QuickJS the module embedded until light-cljs-357, which made the module
1.2 MB larger, with a watchdog that ends a compile whose macro runs past the 10 s
deadline ([docs/browser-macros.md](docs/browser-macros.md)).
`bb playground/build.bb` builds and serves it; see
[playground/README.md](playground/README.md) for the toolchain it needs.

`bb site/build.bb build` assembles the project site (the landing page in
[site/](site/) plus the playground) into `site/dist`, and
`bb site/build.bb serve` serves it at <http://localhost:8788/light-cljs/>;
[.github/workflows/pages.yml](.github/workflows/pages.yml) publishes it to GitHub
Pages on every push to `master`.

## Library usage

Compile source to JavaScript:

```rust
use light_cljs::{analyze, emit_js, parse};

let source = "(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))";
let syntax = parse(source).unwrap();
let program = analyze(source, &syntax).unwrap();
let javascript = emit_js(&program);
// Or light_cljs::compile(source); compile_namespace(entry, roots) for namespaces;
// prepare_application(&program, options).emit_chunks() for a chunked build's
// files and chunks.json, as values.
```

Evaluate incrementally in an embedded session:

```rust
use light_cljs::{EvalOptions, EvalSession};
let mut session = EvalSession::new(EvalOptions::default())?;
session.eval_string("(def hits (atom 0))")?;
let result = session.eval_string("(swap! hits inc)")?;
assert_eq!(session.number(result.value)?, 1.0);
```

`EvalOptions` supplies host functions, source/file providers, I/O callbacks,
print options and resource limits. See [examples/embedding.rs](examples/embedding.rs)
(`cargo run --example embedding`) and [docs/evaluation.md](docs/evaluation.md).

## Examples

| Path | Shows |
| --- | --- |
| [examples/closure.cljs](examples/closure.cljs) | Single-file compilation |
| [examples/namespaces](examples/namespaces) | Two runtime namespaces (`--entry demo.main`) |
| [examples/macros](examples/macros) | A `.cljs` macro namespace required alongside runtime code |
| [examples/collections](examples/collections) | Several namespaces compiled to an expression returning `batch.main/run` |
| [examples/features](examples/features) | The code-splitting application: startup `app.main`, shared helpers and two features loaded on demand (`--entry app.main --out-dir DIR`; [docs/code-splitting.md](docs/code-splitting.md)) |
| [examples/dev](examples/dev) | The dev-workflow application: `defonce` state, a macro, a record view and reload hooks (`--entry app.main --reloadable --out-dir DIR`; [docs/dev-workflow.md](docs/dev-workflow.md)) |
| [examples/native](examples/native) | Scripts for `--eval` (data processing, macros, lazy pipelines, state, exceptions) |
| [examples/embedding.rs](examples/embedding.rs), [examples/evaluator.rs](examples/evaluator.rs) | Rust embedding |

## Known boundaries

- **Numbers** are JavaScript Numbers everywhere, including at macro-expansion
  time. `N`/`M` literals and integral ratios (`4/2`) become Numbers (large
  integers round); non-integral ratio literals such as `1/3` are rejected.
- **Libraries:** only `cljs.core` (the supported subset), `clojure.string`,
  `clojure.set`, `clojure.walk`, `clojure.edn`, `cljs.reader` and `light.lazy`
  (lazy loading; docs/code-splitting.md) are bundled. Other
  namespaces must be supplied as source via `--source-root` or a source provider.
  The Google Closure Library (`goog.*`), `cljs.pprint`, `cljs.test`,
  `core.async` and `test.check` are not provided.
- **JavaScript packages are host-supplied.** A string libspec such as
  `["react" :as r]` lowers each use to `globalThis.lightCljsPackages["react"]`;
  the host fills that registry. With `--esm` the output is one ES module whose
  static imports a bundler (Vite, esbuild, Rollup) resolves and tree-shakes. The
  compiler never reads `node_modules` or `package.json` and does no package
  resolution or CommonJS conversion.
- **No build tooling:** no per-namespace module output; output
  is one JavaScript expression, one ES module with `--esm`, or with `--out-dir`
  one ES module per chunk of namespaces that `light.lazy/loadable` sites load
  (at `advanced` also of definitions and constants moved toward their
  readers), with the runtime in the startup chunk. A dev build
  (`--reloadable`) writes one unit per namespace for reloading, and `--watch`
  rewrites a saved namespace's unit, which `--serve` pushes to open pages with
  the reload policy (`^:dev/once`, `^:dev/always`, `--reload-dependents`) and
  where `--repl --connect` and editors over nREPL evaluate. The release
  optimizer, `--optimize advanced`, is still being built
  ([docs/advanced-optimization.md](docs/advanced-optimization.md)); what it does
  so far is listed under [Compile to JavaScript](#compile-to-javascript). Sizes
  today: `(js/console.log (+ 1 2))` drops from about 390 KB to about 150 bytes,
  a small record and protocol program to about 6 KB, a program that only builds
  a keyword to 4 KB, a vector to 6 KB and a map or set literal to 12 KB; a
  program that prints keeps the printer (about 21 KB), and one that uses the
  generic sequence operations most of the collection machinery (30-50 KB).
- **Collection performance:** vectors are upstream's persistent 32-way trie, so
  `conj` is amortized O(1), `nth`, `assoc` and `pop` are O(log32 n) and `subvec`
  is an O(1) view, but `rseq` copies. A map of more than 8 keys is a hash array
  mapped trie, as upstream's, so its `assoc`, `dissoc` and lookup are O(log32 n)
  ([benchmarks/compiler.md](benchmarks/compiler.md#hash-maps-as-tries--october-14-2026)),
  and its transient edits the nodes it owns in place
  ([benchmarks/compiler.md](benchmarks/compiler.md#transient-hash-maps--october-14-2026)).
  A set of more than 8 members keeps them in the same trie, as upstream's hash
  set over a hash map, so its `conj`, `disj` and lookup are O(log32 n), its
  equality and `clojure.set` operations make one lookup per member, and its
  transient edits the trie in place
  ([benchmarks/compiler.md](benchmarks/compiler.md#hash-sets-as-tries--october-14-2026)).
  A lazy seq pipeline writes no `WeakMap` entry per step, nor does building an
  array map or set, a transient, a queue or a `reduced` marker
  ([benchmarks/compiler.md](benchmarks/compiler.md#private-payloads-for-the-remaining-kinds--october-18-2026)),
  and `seq`, `first`,
  `rest`, `next` and `count` of a lazy seq or a cell read its kind once instead
  of probing each kind's store, so a `map`/`filter`/`partition` pipeline takes
  about 1.8× upstream's time in Node; of the seqs of vectors, arrays and maps,
  only `first` of a vector's or an array's seq reads a kind and the rest take
  the probing path
  ([benchmarks/compiler.md](benchmarks/compiler.md#kind-tables-for-the-seq-operations--october-18-2026)).
- **Macros** must be `.cljs`/`.cljc` (`.clj` macro companions are never loaded)
  and run in an embedded JavaScript engine.
- **Embedded engines:** the macro environment provides ECMAScript intrinsics only:
  no DOM, Node APIs, filesystem, network or process access unless the host supplies
  them, and no timers. `NamePolicy` restricts name resolution but is not a
  sandbox. Under `--eval` and `--repl` promise jobs (`.then` callbacks,
  post-`await` code) run after each top-level form, never inside one, so a
  form sees the reactions of the forms before it, even earlier in the same
  file, where compiled output runs a whole file first. A promise rejection
  that no handler has caught once a form's jobs have run is a warning after
  the form's result under `--repl` and the script's error under `--eval`, as
  in Node, but a handler that a later form of the script attaches comes too
  late ([docs/evaluation.md](docs/evaluation.md#script-and-repl-commands)). Compiled
  output, where `^:async` functions and `await` work as upstream's
  ([docs/compiler.md](docs/compiler.md#async-functions-and-await)), runs them in
  the host's event loop.
- Some upstream behaviors differ deliberately; [docs/compiler.md](docs/compiler.md)
  documents supported forms, runtime semantics and known differences.

## Testing

The tests need Node.js (on `PATH` or in `$NODE`) and the pinned upstream checkout
in the gitignored `research/clojurescript`, whose sources some tests include (see
[tests/reference/README.md](tests/reference/README.md)). Run them with
[cargo-nextest](https://nexte.st) (`cargo install cargo-nextest --locked` or
`brew install cargo-nextest`), which runs every test in its own process and all
binaries in parallel; the full suite takes about 1.5 minutes on the 24-vCPU
development machine. Plain `cargo test` runs the same tests in about 4 minutes.

```sh
cargo nextest run                                  # full suite
cargo nextest run --test compiler                  # one test binary
cargo nextest run --test compiler records::        # one suite (module) of a binary
cargo nextest run records::record_updates          # tests whose name contains this
cargo nextest run --no-fail-fast -j 8              # keep going after failures; 8 at a time
cargo test --test compiler records::               # the same tests through plain cargo test
cargo clippy --all-targets -- -D warnings
bb playground/build.bb lint                        # the wasm32 playground crate, warnings denied
node tests/support/js_runtime.test.js              # runtime tests directly in Node
# after editing src/js_runtime.js: rewrite its unit `requires:` headers, then review the diff
LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST=1 cargo test --test compiler runtime_manifest::
# after moving runtime code between units: rewrite the core operation table's units column
LIGHT_CLJS_UPDATE_CORE_OPERATIONS=1 cargo test --test compiler core_operations::
```

Integration tests are grouped into a few binaries so they build and link once:
`compiler` (`tests/compiler/`), `evaluation` (evaluator, `--eval` CLI and SCI
reference suites, `tests/evaluation/`), `frontend` (tokenizer, parser, analyzer
and namespaces, `tests/frontend/`), plus `execution`, `cli`, `provenance` and
`benchmark_variants`. Add a suite as a module file in the matching directory and
register it with `mod <name>;` in that directory's `main.rs`. Long fixture loops
can declare one test per row, row range or share of the rows with `row_tests!`
([tests/support/row_tests.rs](tests/support/row_tests.rs), loaded by the `compiler`
and `evaluation` binaries), so the parts run in parallel. A complete source that
must give the same result as an application and as a macro computation, with
fresh and reused macro hosts, in QuickJS and Node, goes in its feature suite's
`phase_oracles` module as a case for
[tests/support/phase_oracle.rs](tests/support/phase_oracle.rs); `cargo nextest run
phase_oracles::` runs them all. [.config/nextest.toml](.config/nextest.toml) runs 16
tests at a time, starts the slowest tests first and sets the slow-test timeout;
the `ci` profile (`cargo nextest run --profile ci`) uses every CPU and reports
every failure.

Macros run in V8 in the default build and in QuickJS with `--no-default-features`,
so run the suite and strict Clippy both ways after changing the macro host,
evaluators or compilation session: add `--no-default-features` to the commands
above. Tests whose expectations differ by engine branch on
`cfg!(feature = "v8-macros")`. CI ([.github/workflows/conformance.yml](.github/workflows/conformance.yml))
runs Clippy and the suite once per backend (`fast-tests` with V8, `quickjs-tests`
without), and its `v8-engine` job keeps the patched V8 in the Actions cache, keyed
by the files that decide the engine ([engine cache](vendor/v8/README.md#machine-wide-engine-cache)),
and hands it to the other V8 jobs, which build with `LIGHT_CLJS_V8_CACHE=require`.
A run that finds the engine cached adds a few minutes for that job; the first run
after a change to the engine's inputs builds V8 from source, an estimated 2 to 4
hours on a 4-vCPU runner, before the V8 jobs start. GitHub also evicts a cache
that no run has read for 7 days, and a branch's first run reads only its own and
the default branch's caches.

`size_ratchet` (`tests/compiler/size_ratchet.rs`) pins the emitted size of a few
small programs (`tests/support/ratchet_fixtures.rs`) at each `--optimize` level
and runs them at every level in Node;
`cargo nextest run --test compiler size_ratchet:: --no-capture` prints the size
table. `generated_optimization` compiles seeded multi-namespace programs with
tagged dead code at every level, runs them in Node and checks which dead code
survives against an expected-survivor list; `--no-capture` prints a table per
seed. Its `lazy_seed_*` tests turn one or two namespaces of each seed's
program into lazy roots and check the chunked build against the single file,
and that startup holds no definition only lazy code reads
([docs/code-splitting.md](docs/code-splitting.md), Phase 3). A failure names
its seed: rerun it with `LIGHT_CLJS_GENERATED_SEED=<seed>`, or run more seeds
with `LIGHT_CLJS_GENERATED_COUNT=<n>` (for example 240).

Tests that need files make `light-*` directories in `$TMPDIR` through
[tests/support/temp_dir.rs](tests/support/temp_dir.rs), which removes each one when
its test ends, whether the test passes, fails or panics. Set `LIGHT_CLJS_KEEP_TMP=1`
to keep them for inspection; each kept directory's path is printed. A test that
nextest kills at its timeout cannot clean up after itself. Suites that take an
artifact directory from their own variable, such as `LIGHT_PHASE_ORACLE_ARTIFACTS`,
never remove it.

Some tests bound macro expansion and evaluation by wall-clock deadlines, so on a
heavily loaded machine run fewer tests at once (`-j`) or rerun a failure alone.
To see where suite time goes, save a run's output and summarize it with
`bb benchmarks/nextest_times.bb run.log`, which prints the wall-clock and summed
test time and the slowest tests. Pass a second log to compare two runs.

Optional comparisons against standard ClojureScript need Java, the Clojure CLI
and, for some, the upstream checkout; they are `#[ignore]`d by default:

```sh
cargo test --test execution -- --ignored --nocapture          # vs. ClojureScript 1.12.134
cargo test --release --test frontend corpus::clojurescript_checkout_has_no_lexical_errors -- --ignored --nocapture
cargo test --release --test frontend parser_corpus::parse_clojurescript_checkout -- --ignored --nocapture
```

See [tests/reference/README.md](tests/reference/README.md) for the reference
tooling and how to set up the checkout.

Benchmarks: `cargo bench --bench tokenizer|parser|subset|compiler|evaluator`; see
[benchmarks/README.md](benchmarks/README.md). Startup and steady-state time of the
ratchet programs at every `--optimize` level, in Node and QuickJS:
`cargo bench --locked --bench compiler -- --levels --output DIR` (see
[benchmarks/compiler.md](benchmarks/compiler.md#startup-and-steady-state-by-optimization-level--october-13-2026)).

## Documentation

- [docs/compiler.md](docs/compiler.md) — compiler reference: pipeline, supported
  forms and runtime semantics, macros, namespaces, interop, differences.
- [docs/evaluation.md](docs/evaluation.md) — `EvalSession` embedding API and the
  `--eval`/`--repl` commands.
- [docs/evaluator.md](docs/evaluator.md) — the QuickJS evaluator, compile-time
  phase contract, form bridge, resource controls.
- [docs/compiler-cache.md](docs/compiler-cache.md) — proposed persistent binary
  cache, explicit dependencies, opt-outs, and development-session reuse.
- [docs/parser.md](docs/parser.md) and [docs/lexer.md](docs/lexer.md) — syntax
  layers.
- [docs/dos-and-donts.md](docs/dos-and-donts.md) — performance guidelines for
  compiler work.

## Acknowledgements

light-cljs stands on the work of the Clojure community:

- [Rich Hickey](https://github.com/richhickey) for Clojure and ClojureScript,
  whose language design, core library and test suite this compiler follows and
  is checked against.
- [David Nolen](https://github.com/swannodette) for leading ClojureScript; its
  analyzer and compiler shaped how this project resolves and emits code.
- [Thomas Heller](https://github.com/thheller) for shadow-cljs, which served as
  the reference for the npm dependency and build model.
- [Michiel Borkent](https://github.com/borkdude) for SCI and babashka; SCI's
  test suite provides the evaluator conformance fixtures, and babashka runs the
  scripts in this repository.

## License

light-cljs is released under the [MIT License](LICENSE). Files adapted from
ClojureScript and SCI stay under the Eclipse Public License 1.0, keep their
upstream copyright notices, and are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); the EPL text is in
[epl-v10.html](epl-v10.html). Compiled programs that use `clojure.string` or
`clojure.set` embed JavaScript compiled from those EPL-1.0 files. The embedded
QuickJS-ng engine and rusty_v8 bindings are MIT-licensed; V8 carries its BSD
license and upstream dependency notices listed in the third-party notices.
