# Phase10 cold Penpot comparison

All five jobs ran sequentially with fresh processes. Shadow caches were removed
by `bb benchmarks/penpot.bb /private/tmp/light-cljs-phase10`; the existing port
sources were not regenerated or edited. No tests, builds or profiling overlapped
the benchmark jobs. `results.edn` records commands and measured results.

`tests.log`, `clippy.log`, `build.log` and the three `node-*.log` files validate the
implementation. `equivalence.txt` compares application bodies with phase6b after
excluding the embedded runtime and records complete-output syntax checks.
Penpot has not been bundled and executed, so these checks do not establish
end-to-end application correctness.

`source-hashes.txt` identifies the executable, implementation patch and untracked
name-index source saved in `/private/tmp/light-cljs-phase10/`. `port-hash.txt`
hashes each sorted repository-relative source path followed by its exact bytes
for the five port source roots. The benchmark scope difference is retained:
shadow main also includes render, rasterizer and code-highlight; light main
includes its five lazy modules, with render measured separately.
