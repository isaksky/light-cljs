# Glossary

Plain names for things that come up in reports, benchmarks and pellets.

## Builds

light-cljs runs ClojureScript macros by executing JavaScript while it
compiles.

- **V8 build**: macros run on V8, Google's engine, which compiles hot code to
  machine code while it runs. This is the build that ships today (formerly
  "default build", "default backend").
- **QuickJS-only build**: uses QuickJS for compiler macros as well as
  interactive sessions. Build with `--no-default-features --features quickjs`.

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
