# Compiled language integration evidence

This is historical evidence for the selected compiler-language batch. The adopted
[evaluator migration and broader language contract](../../docs/evaluator-migration.md#language-coverage-and-completion-boundaries)
retain these observations but do not treat this batch's zero-missing result as
whole-language completion. Its `.clj` macro fixtures and runtime-defmacro proofs
describe the previous architecture. `light-cljs-103` preserves the namespace
programs in `tests/fixtures/reference-namespaces` and uses ordinary `.cljs`/`.cljc`
requires for their active local counterparts. The legacy import grammar and
`:clj` macro-reader feature are explicit dialect differences; runtime results
remain compared with the preserved reference programs.

This integration retains the pinned source inventory and requires executable
observations for each selected assertion. It does not claim to build Penpot or
execute the entire upstream suite. The authoritative current counts and outcomes
come from `compiler-language/report.py --run --strict`, including both references.

## Integrated source program

`tests/fixtures/namespaces/integrated` contains a `.cljs` entry, shared `.cljc`
source, and a compiler-emitted `.cljs` macro namespace imported by ordinary
`:require`. The original native `.clj` dialect is retained under
`tests/fixtures/reference-namespaces/integrated`. The program combines overloaded and
variadic functions, `apply`, a 100,000-step stack-safe recurrence, destructuring,
dynamic binding, exceptions/finally, atom watches, metadata, a record, a custom
type implementing core protocols, reified IFn behavior, and lazy traversal.

The literal contract in `expected.json` checks the entire returned language
vector, ordered watch/macro/throw effects, and both output lines. The reference
harness compiles and executes the preserved reference namespace source against Maven 1.12.134
and checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, with `:optimizations
:none`, `:static-fns false`, and `:def-emits-var false`. Native macro execution
is compilation infrastructure; generated JavaScript execution establishes the
runtime result.

`tests/support/language_combinations.rs` generates 216 deterministic programs:
12 feature expressions, 9 surrounding evaluation contexts, and success/failure
paths. Each source is retained in the reference artifacts. All programs check
literal results or Error categories, ordered effects, and stdout. Processes
have timeouts and preserve failing source/diagnostics. This bounded cross-product
is interaction coverage, not an exhaustive program generator.

## Runtime behavior completed during integration

Faithful whole-body and selected-section witnesses exercise ordinary helpers,
callable ABI and lazy argument tails; `some-fn`/`every-pred` predicate order;
`memoize` nil/false caching and retries; grouping/frequencies and map transforms;
comparison/sorting; vector partitioning and splitting; `iteration` seq/reduce
consumption; printing options; tagged literals; JavaScript conversion protocols;
munge/demunge; public sequence/entry constructors and metadata; no-op set update
identity; and dynamically equal set-literal errors. Independent Node assertions
check host identity, callback receivers, immutable stored representatives,
reduced short-circuiting and literal values without the shared result encoder.

The collection-taking source assertions preserve original setup, order, loops
and expected expressions. Mixed bodies omit only independently inventoried
unrelated assertions and their independent setup. Full original bodies, source
spans and hashes remain available for review. The source and runtime scope audits
explain each changed classification; unsupported implementations are never a
reason to exclude a source-language assertion.

Four property sites have explicit deterministic domains: both booleans; 245
Range/IntegerRange constructor combinations; 50 split-position/vector pairs;
and 49 initk/seed combinations for iteration. The iteration generator substitutes
a documented integer stream with a terminating tail for the external test.check
RNG. The original iteration/reduction equality remains unchanged. These are
bounded property samples, not an assertion that all unbounded inputs were tested.

Direct `Range` and `IntegerRange` construction preserves the upstream object
contract: `seq` returns the constructed object even when its bounds would make
`range` return an empty list. `IntegerRange` retains the caller-supplied count
field, including nil or zero. Independent probes distinguish these constructor
semantics from the validating `range` factory.

`uri?` observes the constructor identity of the host's `goog.Uri`. Its source
witness preserves all original arguments; local host setup provides that external
constructor brand and reference builds retain the real Closure constructor.
This tests the core predicate, not URI parsing or Closure library implementation.

Extended destructuring and selector evidence is recorded separately in
[compiler-destructuring-completion.md](compiler-destructuring-completion.md).
Namespace metadata/rename evidence is in
[compiler-namespace-positive-evidence.md](compiler-namespace-positive-evidence.md).

## Honest failure and version accounting

Invalid readable source uses a separate `verified-rejection` disposition. Every
probe requires lexer/parser acceptance, the exact local analysis/header error,
and the exact reference error phase, cause message, warning set and exception
class. A generic unsupported error, missing source, crash, omitted case or timeout
cannot establish this disposition, and it does not inflate runtime passing counts.
Complete original negative-test bodies and all their probes remain linked. Mixed
positive/negative bodies declare an ordered assertion subset and retain separate
witnesses for every other selected assertion. Verified warning cases execute the
accepted source with exact local and reference warning observations; release and
checkout warning differences remain separate from warning agreement.

The strict gate additionally verifies 27 previously unresolved rows from the
independent boundary matrix. Each retains its original source hash, compiler API,
settings and exact ordered diagnostics. Valid quiet forms use the default API;
warning-producing forms use the explicit warning API. Reader and namespace forms
use their applicable source entry points. Executable proofs cover host paths,
definitions and Vars, macro callable behavior, repeated aliases, metadata and
reader conditionals. These supplemental rows add no imported assertion passes.
Four malformed low-level binding/function shapes retain explicit differences
from upstream's permissive primitive analyzer; their public `let`/`fn` spellings
must reject in both references. The ampersand binding retains its separately
verified release/checkout difference.
The [supplemental boundary evidence](compiler-language/boundary-support.md)
records those distinctions and the live gate's failure-injection checks.

Actual release/checkout differences retain independently checked outcomes.
Examples include extended map destructuring/selector availability, mutable Var
string folding, variadic binding validation, and dynamic duplicate set literals.
The local literal constructor follows the checkout: it evaluates every element
before detecting structural duplicates and throwing Error. `hash-set` and `set`
still deduplicate. The two older independent set probes now explicitly retain the
release's returned deduplicated values and checkout/local errors. Identical
readable forms such as `#{(f 1) (f 1)}` reject before execution in both references;
the compiler macro bridge now retains this distinction from differently spelled
forms whose evaluated results are equal. Differences
are reported separately from assertions that agree across all three compilers.

## Reproduction

The final frozen-source run passed **726 Rust tests** across 71 suites, both
direct Node suites, **55 Python accounting tests**, and all-target Clippy with
warnings denied. The 43 opt-in compiler comparison tests then all passed against
both pinned references, including the complete 1,694-program corpus, integrated
namespace program and 216 generated combinations. Four other opt-in benchmark/corpus/SCI
tests remain outside this compiler verification; they are not counted as passes.

The fresh strict gate verified 2,295 agreeing assertion sites, 43 source
rejections, 31 warnings, and 166 explicit version differences: **zero missing**
among 2,535 selected sites. Its independent supplemental gate verified all 27
contracts, comprising 32 diagnostic probes and 13 runtime proofs. The
[retained verification record](../../benchmarks/results/compiler-2026-10-10/language-completion/verification.json)
records commands and counts, with hashes of all 290 tested source and fixture
files. Formatting, exact inventory regeneration, and diff whitespace checks
passed, and the pinned checkout remained clean.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS=180 \
cargo test --locked --test execution -- --ignored --nocapture --test-threads=1
python3 -B tests/reference/compiler-language/report.py --run --strict \
  --reference both --checkout research/clojurescript \
  --output /tmp/compiler-language-completion.json
python3 benchmarks/measure_compiler_language.py \
  --output benchmarks/results/compiler-2026-10-10/language-completion
```

The conformance job checks the exact inventory, runs all explicitly pinned
compiler comparisons, and requires strict assertion completion. Failure artifacts
retain sources, commands, stdout/stderr, identities and effective settings.

The benchmark validates the complete integrated observation in nine fresh Node
processes and records raw/gzip bytes, one runtime prelude, parse time, namespace
initialization, and execution. File reads, process launch and result encoding
are excluded. Host scheduling/caches are uncontrolled; these are Node timings,
not browser measurements or a performance budget.

The retained [October 10 measurement](../../benchmarks/compiler.md#language-integration-october-10-2026)
records 183,146 emitted bytes (42,666 gzip) and a 10.090 ms median from parsing
through complete integrated execution. A literal `42` still emits 88 bytes with
no runtime. `(+ 1 2)` emits 174,302 bytes with the full runtime, making the fixed
cost of a core call explicit. These are measurements of different programs, not
an unchanged-fixture performance comparison.
