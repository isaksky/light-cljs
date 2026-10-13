# light-cljs

light-cljs is a ClojureScript-to-JavaScript compiler written in Rust. It reads
`.cljs`/`.cljc` source, resolves and analyzes it in Rust, and emits a single
self-contained JavaScript expression that carries its own persistent-collection
runtime. The same compiler powers a script runner, a REPL and an embedding API
that execute the emitted JavaScript in an embedded QuickJS engine (via
[`rquickjs`](https://crates.io/crates/rquickjs) 0.14.0), so no Node, JVM or
installed `cljs.core` is needed to run code.

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
  `compiler_destructuring.rs`).
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
top-level calls; `none` emits everything as analyzed; `advanced` is the release
mode being built from [docs/advanced-optimization.md](docs/advanced-optimization.md)
and so far differs from `prepare` only by embedding just the runtime units the
program reaches (see
[Application preparation](docs/compiler.md#application-preparation)). Add
`--size-report` to print to stderr which runtime units the output embeds, with
their sizes and the source expression that first demanded each.

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
- **No build tooling:** no per-namespace module output, source maps or advanced
  optimization; output is one JavaScript expression, or one ES module with `--esm`.
  A release optimizer is designed in [docs/advanced-optimization.md](docs/advanced-optimization.md);
  `--optimize advanced` so far only drops unreached runtime units, about 4% of
  the output while every program still reaches the runtime's `core` hub.
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
```

Integration tests are grouped into a few binaries so they build and link once:
`compiler` (`tests/compiler/`), `evaluation` (evaluator, `--eval` CLI and SCI
reference suites, `tests/evaluation/`), `frontend` (tokenizer, parser, analyzer
and namespaces, `tests/frontend/`), plus `execution`, `cli`, `provenance` and
`benchmark_variants`. Add a suite as a module file in the matching directory and
register it with `mod <name>;` in that directory's `main.rs`. Long fixture loops
can declare one test per row or row range with `row_tests!`
(`tests/compiler/main.rs`). A complete source that must give the same result as an
application and as a macro computation, with fresh and reused macro hosts, in
QuickJS and Node, goes in its feature suite's `phase_oracles` module as a case for
[tests/support/phase_oracle.rs](tests/support/phase_oracle.rs); `cargo nextest run
phase_oracles::` runs them all. [.config/nextest.toml](.config/nextest.toml) runs 16
tests at a time, starts the slowest tests first and sets the slow-test timeout;
the `ci` profile (`cargo nextest run --profile ci`) uses every CPU and reports
every failure.

`size_ratchet` (`tests/compiler/size_ratchet.rs`) pins the emitted size of a few
small programs at each `--optimize` level and runs them at every level in Node;
`cargo nextest run --test compiler size_ratchet:: --no-capture` prints the size
table.

Some tests bound macro expansion and evaluation by wall-clock deadlines, so on a
heavily loaded machine run fewer tests at once (`-j`) or rerun a failure alone.

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
[benchmarks/README.md](benchmarks/README.md).

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
