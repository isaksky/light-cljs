# Bounded native backend project attempts, October 25, 2026

## Configuration recorded before compilation

Compiler source: `b3fac88911d69b3925a0b76d9a49fafe25a625ff`.
Release build command, from the repository root:

```sh
CARGO_BUILD_JOBS=3 cargo build --release --locked --no-default-features --features bbp-macros --bin light-cljs
bb benchmarks/generated-project/generate.bb research/project-attempt-200k --seed 42 --lines 200k --namespace-lines 400
```

This selects only big-boy-pants-js, using the real native-form macro backend;
neither V8 nor QuickJS is compiled. Defaults are unchanged. This baseline includes
462, 474 and 487. It predates 472's aggregate memory/automatic-GC work; the raised
macro memory flag is **not** an enforced aggregate native heap budget here.

Machine: Apple M3 Ultra (Virtual), 24 logical CPUs, 32 GiB RAM, macOS 15.7.3
(24G419), aarch64. Homebrew rustc 1.99.1 (58cf8f9c3 2026-10-13), Cargo 1.99.1
(5f94df478 2026-08-27), Babashka 1.12.218, Node 26.12.1. Initial preparation
load averages: 11.51, 6.54, 4.71. Peer 472 may build/test concurrently. Initial
support probes are not performance comparisons. A successful workload would
get a separately coordinated quiet window for five fresh processes per supported
jobs cell in alternating order, then at most one 30-second profile.

Each compiler process has a 120-second external deadline (`attempt.bb` kills
the process tree and reaps its child), with one initial jobs-1 attempt per full
project and at most one reduction pass per failed project. Each exact argv/cwd
and deadline is written to `*.command.edn` before the process starts. Compiler
cache is off, processes fresh, OS filesystem caches left warm (no purge).
Macro timeouts also use 120,000 ms. Native jobs 2/4/8 are explicitly rejected by
this revision (`src/compiler.rs`, `native macro compilation requires --jobs 1`).

Penpot input: upstream `10955f17e27943429a10e90360d0404e3f56e27c`, clean checkout
at `/Users/admin/src/light-cljs/research/penpot`. The actual input is the existing
local **ported** tree `/Users/admin/src/light-cljs/research/penpot-light`, not the
unmodified upstream app. Its five roots are `frontend/src`, `common/src`,
`vendor`, `generated`, and `shims`, as in `benchmarks/penpot.bb`; entry `app.main`,
ESM, optimization `none`. The input manifest `penpot-inputs.tsv` pins every file
in those roots: 996 files, 12,004,510 bytes. Manifest SHA-256:
`032a12356280fadfce9537e0a25d781b29e5d177924293388ae2dbc357f27264`.
The local port script SHA-256 is
`4be905392e8b52549c7046844ac2973bea042584b76e0c279247a7b1d9f205d7`.
This local port is not versioned by upstream; reproducing it requires the exact
manifest-matching port inputs, including its existing shims/vendor content.
No port regeneration, source rewrite, npm install or shared-tree mutation is used.

Generated input uses the existing generator at the compiler commit above,
`generate.bb` SHA-256
`13e707a203894b666553998dc812c9ffb7bffe2f698be50e0106d5be9d552a1d`.
Seed 42, requested 200,000 code lines, namespace-lines 400: 450 workers,
596 source namespaces, 199,129 code lines, 213,984 physical lines, 9,966,430
bytes, 15,450 top-level forms. `generated-input.edn` retains the generator's
source hashes and size/configuration fields; `generation.txt` is its raw summary.
Entry `workload.main`, roots `src` and `macros`, optimization `advanced` as in
the generated release config, emitted as one ESM with an external source map.
This uses direct CLI equivalents of the generated release budgets and disables
cache, allowing source-map comparison (the project's chunked release command
does not support map flags). No smaller generated input may replace this attempt.

Both commands request `--stats-file` plus an external source map. An error can
prevent all three of JavaScript, source map and stats trace from being emitted;
absence will be recorded rather than treated as identity or a zero metric.
