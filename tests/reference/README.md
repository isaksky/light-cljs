# Reference tooling

This directory holds the scripts and pinned data used to compare light-cljs with
upstream ClojureScript (and, for some evaluator fixtures, SCI). Ordinary
`cargo test` needs none of it: it reads committed data only. The tools here are
for running the optional reference comparisons and for regenerating that data.

Most tools need the upstream ClojureScript checkout at `research/clojurescript`
(gitignored), pinned to `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`:

```sh
git init research/clojurescript
git -C research/clojurescript remote add origin https://github.com/clojure/clojurescript.git
git -C research/clojurescript fetch --depth 1 origin a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e
git -C research/clojurescript checkout --detach FETCH_HEAD
```

The JVM tools also need Java and the Clojure CLI; the release comparison uses
the Maven ClojureScript release `1.12.134`. Node runs the emitted JavaScript.

## Upstream execution comparison

`execution.clj` compiles the same programs with standard ClojureScript and runs
them in Node. It is driven by ignored Rust tests (for example in
`tests/execution.rs`), which compare values, ordered effects and stdout with
light-cljs output:

```sh
# Release 1.12.134 only (the default selection)
cargo test --test execution -- --ignored --nocapture

# Release and the pinned source checkout
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --test execution -- --ignored --nocapture
```

`LIGHT_CLJS_REFERENCE` accepts `release`, `checkout` or `both`. Optional
`CLOJURE`, `NODE` and `LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS` override the tools
and timeout. `analysis_boundaries.clj` is loaded by `execution.clj`'s analysis
mode. `compiler-array-settings/original/` holds unchanged upstream array-access
test programs used by `tests/compiler/array_settings.rs`.

## Reader and parser data

| File | Purpose | Regenerate |
| --- | --- | --- |
| `reader_classes.clj` / `reader_classes.txt` | JVM reader whitespace, terminator and digit classes used by the tokenizer | `clojure -Sdeps '{:paths ["research/clojurescript/src/main/clojure"]}' -M tests/reference/reader_classes.clj` |
| `parser_literals.clj` / `parser_literals.tsv` | Literal spellings and decoded UTF-16 values from the vendored JVM reader | `clojure -Sdeps '{:paths ["research/clojurescript/src/main/clojure"]}' -M tests/reference/parser_literals.clj` |
| `reader-sources.tsv` | Pinned upstream reader test sources and body byte spans | `generate_reader_coverage.py` (see below) |
| `generate_core_var_metadata.clj` | Writes `src/core_var_metadata.tsv` from the pinned `cljs.core` analyzer data (`--check` compares instead of writing) | Run with the Clojure CLI from the repository root, with the checkout's `src/main/clojure` and `src/main/cljs` on the classpath |
| `core-var-metadata-boundaries.tsv` | Mapped core names with no pinned `cljs.core` declaration | Hand-maintained |

`generate_reader_coverage.py` uses the small byte-span reader in `clj_reader.py`.

## Provenance

`fixture-origins.tsv` records where each fixture in `tests/fixtures/` came from
(hand-authored, adapted upstream test, or generated); `tests/provenance.rs` checks
it. `generated-origins.tsv` lists seeds for the generated cases, and
`lexical-reference-limits.tsv` records upstream behaviors that are deliberately
not used as expectations.

## SCI fixtures

See [sci/README.md](sci/README.md) and the attribution in
[sci/NOTICE.md](sci/NOTICE.md).
