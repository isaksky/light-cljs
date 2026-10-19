# Bundling and running the Penpot build in a browser

Status: not started. This page records the scope of light-cljs-309, which the
user closed on October 18, 2026 without doing the work, so that whoever picks
it up starts from here. The Penpot performance investigation
([benchmarks/penpot-investigation.md](../benchmarks/penpot-investigation.md))
verifies that the port compiles, that outputs are byte-identical across
compiler changes and that they pass ESM syntax checks. Nobody has bundled the
application or run it.

## Goal

Bundle the existing `research/penpot-light` port's light-cljs output and
exercise it in a browser: startup, rendering, and loading the lazy modules.
Record reproducible commands and outcomes. Fix compiler defects found this way
with focused regressions in the existing test binaries, and keep edits to the
third-party port (`port.bb`, `shims/`, `overrides/`, `patches/` under
`research/penpot-light`, which is gitignored) separate from compiler source.

The suggested bounded form: one bundle plus a recorded startup smoke run, with
each defect found filed as a pellet rather than fixed in the same change.

## Where things stand

- Penpot `app.main` builds unmodified (no overlay of the port's `vendor/`) in
  every mode: `--compile --esm` at `none` and `advanced`, `--warnings`,
  `--compile --reloadable --out-dir` and `--watch` with and without
  `--cache-dir`, since light-cljs-360, light-cljs-361 and light-cljs-364
  ([benchmarks/compiler-cache.md](../benchmarks/compiler-cache.md)). The build
  covers 922 namespaces, the lazy module entries included.
- `bb benchmarks/penpot.bb OUT main-none` (or `bb port.bb build` inside the
  port) writes one ES module. JavaScript packages stay bare imports that a
  bundler resolves (README, "JavaScript packages are host-supplied"); the
  compiler reads no `node_modules`.
- The port's smoke entry, `smoke/penpot_smoke/core.cljs`, already compiles,
  bundles with esbuild and runs in Node (points, matrices, transit, uuids,
  malli, cuerdas). This is the only bundled run so far, and it never reaches
  rumext, React or the DOM.
- Penpot's lazy modules are reached through `light.lazy/loadable` in the port
  (`shims/shadow/lazy.cljs` is empty). Separate chunk files need `--out-dir`
  output (docs/code-splitting.md, Phase 2); the single-file `--esm` build
  inlines them.

## What the work involves

1. Install Penpot's npm dependencies for the port's output: the frontend's
   `package.json` plus `transit-js`, which the port's transit override needs
   and Penpot does not list.
2. Bundle the output with esbuild (or Vite, which Penpot itself uses),
   resolving the bare imports, once as one file and once from `--out-dir`
   chunks so the lazy modules load as separate files.
3. Supply what Penpot's own build generates around the JavaScript: the page
   from `resources/templates/index.mustache`, compiled SCSS, fonts and other
   static assets (`scripts/build-app-assets.js`), the `:worker` build, which
   is a separate shadow-cljs target, and the generated
   `src/app/render_wasm/api/shared.js` plus the render-wasm module when the
   new renderer is reached.
4. Serve it without a backend. Penpot's Playwright suite already does this:
   `scripts/e2e-server.js` serves `resources/public` statically and the tests
   answer RPC calls from JSON fixtures in `playwright/data`. Run headless
   Chrome against that, check that the login page or a mocked dashboard
   renders with no uncaught errors, and open a view behind a lazy module
   (workspace, viewer or settings).
5. Compare against the shadow-cljs build of the same sources served the same
   way, so that differences are attributed to the compiler or to the port.

## Known blockers and gaps

- The port stubs `cljs.spec.alpha`, `cljs.pprint`, `expound`, `cljs.test`,
  `malli.generator` and others, and `shadow.resource/inline` throws, so any
  startup path that reaches them fails in the port, not the compiler.
- Behavior gaps listed in the port README: `(str record)` prints
  `[object Object]`, and several iterators and `goog.string.StringBuffer` come
  from patches and shims.
- Penpot's `:js-provider :external` with `target/index.js` means its own build
  bundles npm packages separately from the ClojureScript; the light-cljs
  output needs one bundler pass over both instead.
- A full browser run needs a backend or mocked RPC; without one only the
  pages that start before login are meaningful.
