# Compiled JavaScript CLI execution

The script and REPL implementation is `src/eval_cli.rs` and `EvalSession`;
no binary evaluation path constructs a native Context or calls native evaluation.
All language execution uses this Rust compiler's emitted JavaScript in embedded
QuickJS. The renamed `tests/eval_cli.rs` subprocess suite clears PATH/Node/JVM
variables; `tests/eval_examples.rs` copies the binary to isolated directories and
executes six whole programs with explicit sources and no ambient toolchain.

## Source and semantic adaptations

Project-owned `.clj` scripts were renamed `.cljs`. `.cljc` helper/platform
selection uses `:cljs`; `.cljs` inputs retain conditional rejection. Plain stdin,
`--text` and REPL input use the CLJS dialect with conditional selection.
Native exact numbers and native Var result conventions are intentionally retired:
`(/ 1 2)` yields `0.5`; `def` returns its initializer value. Catch classes use
CLJS `:default`; error output uses `*print-fn*`/`*print-err-fn*`. The example greeting
now identifies compiled JavaScript. Historical SCI fixtures are retained as the
original oracle sources; these adapted programs do not claim unchanged JVM parity.

The script printing/string-input fixture adapts SCI `test/sci/io_test.cljc` at
`5fe89056384418be50d232b4278344cb3a1a526b`, with source anchors and adaptations in
`fixture-origins.tsv`. Other CLI policies/acceptance fixtures are hand-authored.
Multiline quoting now chooses the `:cljs` conditional operand explicitly; native
`:clj` platform selection is not retained. The canonical runtime supplies language
IO; the host frontend uses Rust parsing and contextual lowering over that exact
stream cursor. Fully consumed lines compact without losing diagnostic locations.

## Verification, 2026-10-10 local date

- Prerequisite: `cargo test --locked --test eval_sessions`: 20 passed before CLI changes.
- `cargo test --locked --test eval_cli --test eval_examples --test cli --test eval_sessions --test evaluator`:
  54 passed (16 CLI, six examples in one test, eight frontend CLI, 20 session,
  nine evaluator tests); one opt-in evaluator startup measurement ignored.
- `cargo test --locked --lib eval_compiler`: four passed.
- `node tests/support/js_runtime.test.js`: all canonical runtime assertions passed.
- `cargo check --locked --all-features`: passed.
- `git diff --check`: passed.
- `python3 tests/support/eval_cli_terminal.py`: Number division, compiled source
  macros/Math, missing explicit source, stack/memory limits, SIGINT cancellation,
  fresh invocation after failure, terminal namespace/continuation prompts and EOF passed.
  This direct check requires Unix pty/signal support and the built debug CLI;
  `LIGHT_CLJS_BINARY` can select another built executable.
- Direct `./target/debug/light-cljs --eval --text '(/ 1 2)'`: stdout `0.5`.
- Direct `./target/debug/light-cljs --eval --source-root examples/native examples/native/demo.cljs`:
  greeting `Hello from compiled JavaScript`, expected squares/sum map.
- Legacy benchmark driver check:
  `python3 benchmarks/run_interpreter.py --output /tmp/light-cli-retired-benchmark`
  exited 2 with the explicit native-CLI retirement diagnostic and created no directory.
  The analogous type/transducer drivers use the same early replacement error;
  `python3 -m py_compile benchmarks/run_interpreter.py benchmarks/run_interpreter_types.py benchmarks/run_interpreter_transducers.py`
  passed. Historical result files remain unchanged.

CLI coverage includes script/file/stdin/empty/Unicode inputs, macro and helper
imports, `.cljs` precedence, missing source roots, one-time helper runtime effects
without replay during expansion, Number/JS semantics, multi-form/multiline state,
namespace changes, history/error data, malformed input recovery, runtime/caught
reader recovery, output/result ordering, bounded rendering, all replacement
controls and rejected legacy spellings, invalid UTF-8 and compacted input locations.

`cargo test --locked --test provenance` retains one pre-existing failure:
`fixture_ids_are_unique_and_have_complete_provenance` rejects the unchanged
`numeric-number-keys` row because hand-authored fixtures may not claim an
upstream source. The same invalid row is present at starting HEAD `cbf3fc0`;
live `light-cljs-127` already owns the repair. No duplicate follow-up was added.
`cargo test --locked --test provenance derived_provenance_requires_pinned_traceable_sources`
passed. The upstream provenance validator was not weakened.

No full project suite, all-feature execution suite, release startup measurement,
JVM/CLJS reference rebuild or cross-platform execution was run for this CLI step.
Physical native evaluator removal and the broader migration gate remain separate
live Pellets.
