# Compiler-language source inventory

This inventory retains eighteen complete upstream test files selected by the
compiler language coverage work, at ClojureScript commit
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. It records source assertions and
their original expectations. It does not report them executed. See
[evidence.md](evidence.md) for executable witnesses, reports, and strict versus
focused commands.

The files named `inventory.jsonl`, `bodies.jsonl`, and `manifest.json` retain
this **historical selected feature inventory**. The default report now uses the
independent [full intended-language ledger](full-scope.md), including public
names/arities and every runtime test/support file. Select
`report.py --scope historical-selected` to reproduce this batch. Its passing
subsets do not establish full language completion; standard transients,
transducers, sorted collections and multimethods stay intended in the full ledger.

The [after-190 evidence checkpoint](after190-integration.md) adds current paired
arity and actual phase witnesses, complete original SCI contrasts, and separately
bounded whole-source predicate and record-tag reference checks. Its unfiltered
baseline remains incomplete; focused passing receipts cannot close Pellet 125.

```sh
python3 tests/reference/compiler-language/extract_inventory.py
python3 tests/reference/compiler-language/extract_inventory.py --check
python3 tests/reference/compiler-language/test_inventory.py
```

Extraction and checking require the pinned Git object and byte-identical source
files under `research/clojurescript`. The extractor uses Python's standard
library and Git; the unit checks use synthetic source and require no checkout,
Clojure, Java, Node, or network. `--check` writes nothing and compares regenerated
historical and full inventories, bodies, public obligations, auxiliary sources,
and manifests byte for byte. Removing a row, omitting a test
body, changing an expectation, changing a scope rule, or altering pinned source
fails that check until the change is explicitly regenerated and reviewed.

## Files and identity

`inventory.jsonl` has one row per source assertion site. IDs are
`source-basename::top-level-name::assertion-NNN`, numbered in source order within
the complete top-level form. Repeated top-level names get a `~N` suffix. IDs are
stable for this pinned source, independent of execution outcomes or disposition.
A later upstream revision may require an explicit mapping migration.

Each row has `source`, `test`, `body_id`, `body_sha256`, primary `feature`, all
detected `features`, `status`, `disposition`, `category`, `reason`, `active`, and
`inactive_reason`. `assertion` contains its verbatim source, SHA-256, one-based
inclusive line numbers, and zero-based UTF-8 byte offsets with an exclusive end.
`context` locates enclosing binding/iteration/testing forms within the body.

`bodies.jsonl` retains **every complete top-level form**, including namespace
declarations, fixture values, definitions, helper functions, test bodies with no
assertion macros, and reader-discarded forms. Its `source_span` uses the same
lossless source-span schema, and `assertions` gives all associated assertion IDs.
`kind` distinguishes tests from support forms. Helpers are not omitted because
their assertions happen outside a `deftest`: for example, `metadata_test.cljc`
asserts within three shared interface helpers. The full calling test body and
all of its collection/sequence inputs remain available alongside them.

`manifest.json` records the revision, complete source-file hashes, per-file
counts and original copyright/license notices, extractor and output hashes,
feature/category/status counts, and zero-assertion test IDs. Byte hashes cover
comments and whitespace even though comments are not assertion sites. The
upstream source retains its Eclipse Public License 1.0 attribution; inventory
source excerpts are covered by the original notices retained in the manifest.

## What an assertion counts

The structural reader handles strings, regexes, characters, all delimiters,
reader prefixes, tags, namespaced maps, anonymous functions, metadata, symbolic
numbers, and conditionals. It recognizes `is`, `assert`, `are`, and `defspec`,
including qualified names. `thrown?` and related expectations stay inside their
enclosing `is`; they are not counted twice. Each `are` argument row is a separate
record, retaining the complete original `are` expression plus its parameter
vector, template, and exact argument/source spans. Incomplete rows fail
extraction. A `defspec` records the whole quantified property, including its
generator; it does not claim a fixed number of generated cases.

An `is` message expression is preserved as a message. For example, several
upstream apply/bit-operation assertions put a second expression outside the
equality predicate. Extraction neither silently moves that expression into the
predicate nor asserts stronger behavior than the original test.

Counts are **source sites**, not dynamic assertion invocations. Loop iterations,
helper arguments, setup, mutation order, exception paths and fixture dependencies
remain part of the complete bodies. A future adapter must preserve that context
before claiming coverage. A body containing no assertion macros is explicitly
listed; it must not be converted into a passing assertion. `#_`-discarded,
comment-form, quoted-data, and non-CLJS conditional assertions are retained with
an inactive reason and cannot increase the active or passing counts.

## Scope and statuses

The primary sections are functions/apply, recursion, bindings/control, mutable
state, protocols/types/records, reader/metadata, exceptions, core helpers, lazy
sequences, JavaScript interop, and namespaces. Every active source-language
assertion begins `missing`. Current compiler rejection, interpreter support, or
a superficially similar fixture is not evidence of a pass. Checkout-only syntax
also begins `missing`; a `version-difference` needs explicit version-specific
execution evidence elsewhere.

Whole-file retention makes scope exclusions reviewable. Exact upstream AST
fields, generated JavaScript spelling/optimization shape, constant naming,
compiler cache APIs, and private storage helper tests are explicitly
`upstream-internals`. Source namespace validation, rejected malformed syntax,
overload/recur rules, and compiler warnings remain source-language obligations.

The selected feature inventory separately classifies sorted/transient collection
implementations, general transducer constructor/execution APIs, standalone library namespaces,
multimethod/hierarchy and tap-delivery APIs. These categories retain the historical
batch's exclusions; they are not the current intended-language boundary.
All 104 historical sorted sites now have complete executable setup/body adapters
in the intended-language ledger; see [sorted source evidence](../compiler-sorted-collections.md)
for both-engine/reference verification and the remaining cross-feature dependencies. Core
feature gaps and explicit host-hook contracts have owners in the roadmap above.
The extractor follows named fixture/local references
to classify mixed bodies per assertion: a normal set assertion stays in scope
beside a sorted-set assertion, and a collection-taking `map` stays in scope
beside its transducer constructor arity. The full body is retained in both
cases. Built-in tagged values, core printing, regexes, metadata, lazy traversal,
protocol extension, and host array/object behavior are not excluded merely
because implementation is missing.

Scope rules are versioned in the extractor, not inferred from runtime failures.
Classification is source-level inventory guidance, not an executable dependency
resolver or a proof that a extracted assertion can run independently. Complete
setup/helper/iteration preservation must be checked when adding witnesses.

Namespace-program witnesses are listed in `namespace-cases.jsonl`. Their entry
files are read from the recorded source roots and compiled through the namespace
CLI and checked reference program/export mode. They share the same assertion
hashes, complete-body mapping, strict outcome checks and missing-evidence rules
as expression witnesses; see [evidence.md](evidence.md).

### Warning assertions

`warning-cases.jsonl` retains complete upstream body hashes, all active assertion
IDs, every original quoted source probe, and exact ordered warning traces. The
local compiler must accept the source, emit the expected diagnostics, and produce
syntactically valid JavaScript. These probes are not executed: warning assertions
are counted separately as `verified-warning`. An arbitrary rejection, missing
warning, changed order, missing settings evidence, or unavailable reference keeps
the assertion missing. Both reference identities and the none/false/false base
options are checked through the normal analysis oracle. `warnings.clj` captures
delivered warning handlers (excluding suppressed inference passes), preserves
namespace-sequence environments, and verifies checked-array settings. Original
static-fns=true contexts are run a second time on both references and must satisfy
the same exact warning contract as the pinned static-fns=false primary run.
Explicitly different release observations remain `version-difference`.

A negative diagnostic from a mixed positive/negative body may declare
`assertion_subset: true` and an explicit `split_rationale`; its IDs must remain an
ordered subset of the complete hashed body. Every other source assertion remains
independently required by strict completion. This does not turn positive analyses
into negative diagnostic evidence.

## Evaluator migration accounting

The legacy fixture `native_stdout` column is retained as historical evidence.
`report.py` reports it as `native: historical-not-executed`; the CLI's `--eval`
now executes `EvalSession` and has its own `evaluation` column. Matching compatible
historical stdout can pass that evaluation check; differing observations stay
mismatches. Lexer/parser acceptance and either evaluation column cannot satisfy
the required JavaScript/release/checkout semantic layers. The broader
[evaluator gate](../evaluator-migration/README.md) also executes phase semantics,
QuickJS/Node comparisons and the complete SCI fixture accounting separately.
