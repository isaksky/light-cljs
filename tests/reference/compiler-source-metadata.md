# Authoritative definition source metadata

`def`, `defonce`, `declare`, and macro-generated definitions overwrite user
`:file`, `:line`, and `:column` with the compiler's source identity and actual
site. Owned forms retain their source span, including incremental REPL line
offsets and macro call sites. Namespace loading preserves the selected source
path, and CLI compilation now supplies its file/text/stdin identity explicitly.
The string-buffer analyzer keeps its existing synthetic `cljs.user` identity;
EvalSession strings use `<eval>.cljs`, CLI text uses `<text>.cljc`, and REPL input
uses `<stdin>.cljc`. Paths are emitted as UTF-16 data, including quotes,
backslashes, and Unicode. Definition doc/name/ns/private/dynamic behavior stays
covered independently. Object metadata filtering is a separate contract.

`compiler_source_metadata.rs` checks literal metadata fields through QuickJS
and Node for declarations, defonce, macro-generated definitions, and
macro-phase reflection without demanding application roots. Incremental
EvalSession checks separately cover cross-file macro reuse, a failed source
followed by valid input, and fresh sessions.
`eval_cli.rs` checks actual files and synthetic text identities in eval/compile
and incremental REPL line offsets. The namespace fixture independently checks
both source paths and actual declaration/expansion locations in both engines.

The exact original `sci-var-reserved-metadata` source and original SCI outcome
remain unchanged in `sci-outcomes.tsv` and the first seven disposition columns.
The compiler evaluator now rejects the fake file and matches the documented SCI
Number projection at its original 1:1 location. The row becomes portable;
`sci-var-reserved-metadata-effects` remains a gap owned by light-cljs-125.
Matching historical SCI observations do not themselves establish CLJS parity.

The reproducible public runner executes the exact historical source and the
same-definition filename/location/doc projection with Maven **1.12.134** and
clean checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. It checks pin
identity and `:optimizations :none`, `:static-fns false`, `:def-emits-var false`
through the existing checked reference driver. All four probes pass on both
pins. The generated reference fixture has its definition at **3:25**, while the
historical local fixture is **1:1**; these are independently verified sites.
A third probe checks the actual `semantic/case2.cljs` filename. A fourth
retains the actual declaration source location on generated `->R`/`map->R`
record factories. Type and record factory metadata use the same compiler
provenance logic as ordinary definitions, while preserving their doc/arglists. Separate original
and projected source hashes, reference fixture hashes, checked observations,
and independent local witness hashes are retained in
[compiler-source-metadata-provenance.json](compiler-source-metadata-provenance.json).

The exact historical reference also exposes an existing `ns-name` result-type
difference: pins return the namespace name string, while local reflection returns
a symbol. That distinct contract has live owner **light-cljs-143** after duplicate
checks. The filename projection does not hide this difference or claim whole
historical-source CLJS parity.

```sh
cargo test --locked --test compiler_source_metadata --test eval_cli
python3 -B tests/reference/compiler-source-metadata.py --output /tmp/light-cljs-source-metadata
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo check --locked --all-targets --all-features
cargo clippy --locked --all-targets -- -D warnings
cargo bench --locked --bench evaluator -- --iterations 20
cargo bench --locked --bench compiler -- --primitive-only --samples 9 --iterations 100
```

The validated twenty-iteration evaluator benchmark recorded total fresh-session
338,640,042 ns, reused-session 2,283,375 ns, and bounded-loop/reset/recovery
1,385,637,791 ns. The compiler primitive benchmark ran nine samples per phase
with 100 iterations and validated checksums; its integer program remains compact
(**88 bytes**). [Raw compiler samples](../../benchmarks/results/source-metadata-2026-10-11/compiler.csv)
and [evaluator totals](../../benchmarks/results/source-metadata-2026-10-11/evaluator.tsv)
retain the observations; unused definition provenance does
not emit the collection runtime into primitive programs. These are local timing
observations, not comparative performance guarantees. Runtime source is unchanged.

Inventory regeneration and `--check` pass with the updated SCI source evidence.
Strict all-target Clippy reports 51 existing lint findings in unchanged production
code (no primary diagnostic intersects the changed lines). These findings are
retained as a validation limitation; this scoped change does not suppress them.
Formatting checks for the changed CLI and tests and `git diff --check` pass;
the repository has previously recorded formatting drift outside this change.

The final scoped source suite also verifies a skipped `defonce` initializer keeps
its first root while recording the second file and its actual 2:2 definition site.
All-target Clippy without `-D warnings` completes successfully with existing
production/test lint warnings.

The ordinary Rust run completed 70 target summaries with
**602 passes and 62 explicitly ignored checks**.
After the generated-factory refinement, all **119** relevant source, record,
type, and macro regressions passed; all four source metadata probes passed on
each pin. The explicit ignored/upstream language gates remain separate; this
scoped provenance repair does not close the incomplete light-cljs-125 milestone.

The final `cargo check --locked --all-targets --all-features` passed.
