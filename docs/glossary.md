# Glossary

Plain names for things that come up in reports, benchmarks and pellets.

## Builds

light-cljs runs ClojureScript macros by executing JavaScript while it
compiles.

- **V8 build**: the standard build. V8 is its only embedded engine and runs
  compile-time macros and data readers as well as interactive sessions.
- **No-engine build**: embeds no JavaScript engine. Build with
  `--no-default-features`. The WebAssembly compiler is also a no-engine build.
  An embedder can provide a `ScriptHost` for macros in clean compilations.
- **QuickJS reference**: a label in historical measurements. QuickJS and the
  experimental Pants build have been removed from this branch.

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
