# Glossary

Plain names for things that come up in reports, benchmarks and pellets. Use
these in new writing. Older docs, test names and the `bbp-macros` feature flag
still use the earlier words in parentheses; renaming them is tracked
separately.

## Builds

light-cljs runs ClojureScript macros by executing JavaScript while it
compiles. The builds differ only in which JavaScript engine does that;
all must produce byte-identical output.

- **V8 build**: macros run on V8, Google's engine, which compiles hot code to
  machine code while it runs. This is the build that ships today (formerly
  "default build", "default backend").
- **Pants build**: macros run on big-boy-pants-js
  (`crates/big-boy-pants-js`), the engine being built into light-cljs. It
  interprets JavaScript, so it is slower per thread, but it is designed so
  many threads can share one set of compiled macros. Built with
  `--no-default-features --features bbp-macros` (formerly "native build",
  "native backend", "bbp backend").
- **Goose build**: macros run on Goose: user macros and their helpers execute
  in bbp's Goose runtime, and core macros expand in light-cljs's own Rust
  code. No V8 or QuickJS is linked, and the compiler creates no compile-time
  engine at all. Built with `cargo build --release --no-default-features
  --features goose-macros --bin light-cljs`; Goose is selected at run time
  by pointing `LIGHT_CLJS_GOOSE_RUNNER` at a runner built with `goose build
  crates/big-boy-pants-js/src/compiler_session.goose -o RUNNER`
  (benchmarks/generated-project/README.md). Without that variable the same
  binary runs as the Pants build. The V8 build with `--features
  goose-macros` selects Goose the same way.
- **QuickJS reference**: the older QuickJS setup, kept only to check that the
  Pants build gives the same answers (formerly "legacy", "QuickJS oracle").

## Measurements

- **N threads**: how many threads compile in parallel (the `--jobs N`
  option).
- **Clean build**: a build with no cache (`--no-cache`).
- **Rebuild, nothing changed**: a second build with the cache filled and no
  source changes.
- **Rebuild after a one-file edit**: the same, after changing one source file
  that nothing else depends on (formerly "leaf edit").
- **Peak memory**: the most physical memory the build process held at once
  (tools print it as "RSS", resident set size).
- **Slowdown vs V8**: how many times longer the Pants build takes than the V8
  build to run the same macros, for example "5x slower than V8" (formerly
  "k").
