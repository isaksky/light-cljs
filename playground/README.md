# Browser playground

A static page that compiles ClojureScript in the browser with light-cljs built
for WebAssembly, then runs the emitted JavaScript in the browser's own engine.
Several in-memory files with an entry namespace, the three `--optimize`
levels, warnings, pseudo names and the size report are all available.

```text
playground/
  crate/        the wasm crate: light-cljs plus a thin wasm-bindgen wrapper
  index.html    the page (CodeMirror from a CDN, no npm)
  app.js        files, compile options, result views, local-storage persistence
  compiler.js   the page's handle on the compiler worker: stale results, Stop
  compiler-worker.js  runs the wasm compiler off the page's thread
  macro-host.js the browser engine's side of "browser macros"
  examples.js   the example programs in the dropdown, smallest first
  worker.js     runs one compiled program in a worker the page can terminate
  style.css
  build.bb      builds crate + page into playground/dist and serves it
  check.mjs     Node check of a built dist: every example, and a chunked build,
                at every level, and the compiler worker (CI runs it)
  dist/         the built site (ignored by git)
```

## How it works

`crate/src/lib.rs` exports `compile(paths, sources, entry, optimize,
warnings, pseudo_names, size_report, esm)`. It feeds the files to
`light_cljs::compiler::analyze_namespace_entry_with_provider` through an
in-memory `SourceProvider` (namespace dots become directories, hyphens
underscores, `.cljs` before `.cljc`), prepares the application at the chosen
level and emits the JavaScript. Macros expand in the embedded QuickJS, which is
compiled into the same wasm module. `compile_chunks(paths, sources, entry,
optimize, warnings, pseudo_names, size_report)` compiles the same way into a
chunked build, the CLI's `--out-dir` ([docs/compiler.md, "Chunked
builds"](../docs/compiler.md#chunked-builds)): its result holds the file
names (`files`), their ES modules (`chunks`) and `chunks.json` (`manifest`)
instead of `js`. The page does not call it yet.

The page never runs the compiler on its own thread. It compiles the wasm
module once (`WebAssembly.compileStreaming`) and hands it to
`compiler-worker.js`, a module worker that instantiates it and answers each
compile request, by id, with the result's fields as a plain object; the
editor stays responsive while a compile runs. `compiler.js` gives each
request an id and shows only the newest one's outcome, so a compile that a
later one superseded resolves `stale` and touches no view. Stop terminates a
busy compiler worker and starts another from the same module; the next
compile then pays one instantiation and the macro bootstrap again. A worker
that crashes (a Rust panic) is dropped the same way and replaced on the next
compile.

Two things differ from the native build:

- `std::time::Instant` does not exist on `wasm32-unknown-unknown`, so the
  evaluator's session clock (`src/evaluator.rs`, `host_clock`) reads the clock
  that the engine's wasm shim already imports, `__rquickjs_host_now_us`. The
  crate defines that symbol from `Date.now`, which resolves the import at link
  time, so the module needs no `env` import object.
- The emitted program runs in a web worker, not inside QuickJS: a fresh worker
  per run, `console.*` relayed to the page, terminated on completion or after
  10 seconds. Call `(enable-console-print!)` before printing, as in
  ClojureScript.

With the "browser macros" checkbox, a compile request carries `macros:
"browser"` and its macros run in the compiler worker's own JavaScript engine
instead of QuickJS: `set_browser_macros(true)` makes the crate hand
`MacroHost::set_script_host` a host that calls `globalThis.lightCljsMacroHost`,
which `macro-host.js` provides, and `serve_macro_host` is the way back in for
the compiler services macros call. The output is the same; macro-heavy
compiles are about three times faster, and a macro that never returns runs
until Stop, since the 10 s deadline is not checked inside the browser's
JavaScript. [docs/browser-macros.md](../docs/browser-macros.md) has the
details, the isolation differences and the measurements. `check.mjs` compiles
every example at every level, and programs that stress macro semantics, with
both engines and requires identical results, and stops a spinning compile in
each.

The example dropdown replaces all files with one of the programs in
`examples.js`, from a single namespace up to a calculator spread over several.
Most log through `js/console.log` so the emitted JavaScript stays small and
shows one feature; "Printing" and the calculator use `(enable-console-print!)`.
Each entry carries the value its program must produce, and `check.mjs`
compiles and runs all of them at every optimization level, checking that they
log the same lines, and that `compile_chunks` writes each as one file, its
`esm` module. It also compiles a program with two lazy features through
`compile_chunks` at every level, checks `manifest` against `files` and
`chunks`, and runs the modules through the compiler tests' Node driver
(`tests/support/chunked_execution.mjs`), which must print what the single
file prints; a failed compilation must leave every output empty. Last it
loads `compiler.js` and runs `compiler-worker.js` in a `worker_threads`
worker: the worker's results must be the direct compile's, a superseded
request must resolve `stale`, and a compile that spins in a macro, stopped
after a second during which Node's own thread keeps ticking, must leave a
fresh worker whose next compile runs correctly; worker doubles that fail
how a panic or a thrown error would must resolve the pending compile
`failed` and be replaced on the next one. To add
an example, append to `EXAMPLES` and run `bb playground/build.bb check`.

Compile time in the browser is a few hundred milliseconds for the first
compile (the macro bootstrap) and tens of milliseconds after that. The wasm
module is about 6.4 MB (around 2.0 MB gzipped).

## Building

Requirements:

- A Rust toolchain with the `wasm32-unknown-unknown` target. Homebrew's `rust`
  ships no wasm target, so use rustup. To keep Homebrew's `cargo` as the
  default, install the toolchain in an isolated home, which `build.bb` finds
  on its own:

  ```sh
  brew install rustup
  RUSTUP_HOME=~/.rustup-wasm CARGO_HOME=~/.rustup-wasm/cargo \
    "$(brew --prefix rustup)/bin/rustup" toolchain install stable \
    --profile minimal --target wasm32-unknown-unknown
  ```

  With a regular rustup install, `rustup target add wasm32-unknown-unknown`
  is enough.
- A clang that emits wasm32 objects, for QuickJS: `brew install llvm` on macOS
  (Apple's clang cannot); the distribution's clang on Linux.
- The wasm-bindgen CLI at the version `crate/Cargo.toml` pins:
  `brew install wasm-bindgen`, or `cargo install wasm-bindgen-cli --version <pinned>`.
- [babashka](https://babashka.org) for the build script, and Node for `check`.

Then:

```sh
bb playground/build.bb build       # playground/dist
bb playground/build.bb serve       # http://localhost:8787/
bb playground/build.bb check       # node playground/check.mjs against dist
bb playground/build.bb             # build, then serve
```

The script locates the toolchain as described in its header comment; each
piece can be overridden with an environment variable (`LIGHT_CLJS_CARGO`,
`CC_wasm32_unknown_unknown`, `AR_wasm32_unknown_unknown`, `WASM_BINDGEN`).
`wasm-opt` is used when it is on `PATH` and skipped otherwise.

The page must be served over HTTP (modules and workers do not load from
`file:`); any static server that serves `.wasm` as `application/wasm` works.

## Notes

- `vendor/rquickjs-sys` carries the crate's wasm support files
  (`wasm-shim/shim.c`, `vendor/wasi-libc`), see its `PATCHES.md`.
- On macOS, rustup's `rust-lld` loads `libLLVM.dylib` through an rpath that
  rustup's layout does not satisfy (seen with Rust 1.99); the script sets
  `DYLD_LIBRARY_PATH` to the toolchain's `lib` for the build.
- The crate is not a member of the root workspace: it has its own `Cargo.lock`
  and `target/`, and `cargo test` at the root does not build it. CI builds it
  and runs `check.mjs`.
