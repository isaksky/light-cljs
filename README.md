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
                                         └── macro expansion ◀┘ compiled to JS, run in QuickJS (evaluator.rs)
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
  QuickJS heap (`macro_host.rs`, `evaluator.rs`, `form_bridge.rs`). Core macros
  are bootstrapped from `src/compiler_core_macros.cljs` and
  `src/compiler_sequence_macros.cljs`; user macros are ordinary `defmacro`s in
  `.cljs`/`.cljc` files. Ordinary helpers that macros call are compiled on demand
  (`compilation_session.rs`).
- **Application preparation** (`src/application.rs`) conservatively removes
  unused inert definitions and specializes some calls, preserving effect order.
- **Emitter** (`src/emitter.rs`) writes the JavaScript expression; the shared
  runtime is `src/js_runtime.js`, split into units with declared dependencies
  (`src/runtime_manifest.rs`). Bundled `clojure.string`, `clojure.set`,
  `clojure.walk`, `clojure.edn` and `cljs.reader` live in
  `src/compiler_libraries/`.
- **Evaluation** (`src/eval_session.rs`, `eval_compiler.rs`, `eval_cli.rs`)
  compiles forms incrementally and executes them in a persistent QuickJS heap.

## Building

Requires a Rust toolchain with edition 2024 support (Rust 1.85+) and a C compiler
(QuickJS is built from source by `rquickjs`). Node.js is only needed to run the
compiler output outside the embedded engine and for parts of the test suite.

```sh
cargo build --release
```

## Command-line usage

Run `cargo run -- --help` for the full option list.

### Evaluate scripts and use the REPL (embedded QuickJS)

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
printed result. Details: [docs/evaluation.md](docs/evaluation.md#script-and-repl-commands).

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

`--optimize none|prepare|advanced` after `--compile` (single files and `--entry`
builds, with or without `--esm`) picks the optimization level. `prepare`, the
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
light-cljs built for `wasm32-unknown-unknown` (QuickJS included, for macro
expansion) compiles several in-memory namespaces at any `--optimize` level, and
the emitted JavaScript runs in the browser's own engine inside a worker.
A dropdown offers examples from a single namespace to a calculator in several.
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
// Or light_cljs::compile(source); compile_namespace(entry, roots) for namespaces.
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
| [examples/native](examples/native) | Scripts for `--eval` (data processing, macros, lazy pipelines, state, exceptions) |
| [examples/embedding.rs](examples/embedding.rs), [examples/evaluator.rs](examples/evaluator.rs) | Rust embedding |

## Known boundaries

- **Numbers** are JavaScript Numbers everywhere, including at macro-expansion
  time. `N`/`M` literals and integral ratios (`4/2`) become Numbers (large
  integers round); non-integral ratio literals such as `1/3` are rejected.
- **Libraries:** only `cljs.core` (the supported subset), `clojure.string`,
  `clojure.set`, `clojure.walk`, `clojure.edn` and `cljs.reader` are bundled. Other
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
  is one JavaScript expression, or one ES module with `--esm`. The release
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
  ([benchmarks/compiler.md](benchmarks/compiler.md#hash-maps-as-tries--october-14-2026)).
  Sets are still linear entry arrays, whose lookups and updates are O(n)
  ([docs/compiler.md](docs/compiler.md)).
- **Macros** must be `.cljs`/`.cljc` (`.clj` macro companions are never loaded)
  and run in QuickJS, not the JVM.
- **Embedded engine:** the QuickJS environment provides ECMAScript intrinsics only:
  no DOM, Node APIs, filesystem, network or process access unless the host supplies
  them. `NamePolicy` restricts name resolution but is not a sandbox. Promises are
  not pumped.
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

`size_ratchet` (`tests/compiler/size_ratchet.rs`) pins the emitted size of a few
small programs (`tests/support/ratchet_fixtures.rs`) at each `--optimize` level
and runs them at every level in Node;
`cargo nextest run --test compiler size_ratchet:: --no-capture` prints the size
table. `generated_optimization` compiles seeded multi-namespace programs with
tagged dead code at every level, runs them in Node and checks which dead code
survives against an expected-survivor list; `--no-capture` prints a table per
seed. A failure names its seed: rerun it with `LIGHT_CLJS_GENERATED_SEED=<seed>`,
or run more seeds with `LIGHT_CLJS_GENERATED_COUNT=<n>` (for example 240).

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
QuickJS-ng engine is MIT-licensed.
