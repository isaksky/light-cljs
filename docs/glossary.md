# Glossary

Plain names for things that come up in reports, benchmarks and pellets.

## Builds

light-cljs runs ClojureScript macros by executing JavaScript while it
compiles.

- **V8 build**: macros run on V8, Google's engine, which compiles hot code to
  machine code while it runs. This is the build that ships today (formerly
  "default build", "default backend").
- **No-engine build**: embeds no JavaScript engine. Build with
  `--no-default-features`. An embedder can provide a `ScriptHost` for macros.

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
