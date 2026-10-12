# Complete core-helper arity and receiver candidates

This separate Pellet 125 cohort exports **35 additional candidate links**:
18 public declaration/signature rows and 17 local API operation cross-checks,
with 35 exact actual-phase requirements and ten complete original helper
oracles. The earlier 24-row receipt remains unchanged. Shared catalogs,
production sources, evaluator options, original fixture bytes and queue state
remain untouched by this work. It adds zero original source assertion credit.

The source is the unchanged `compiler-core-helpers.tsv` workflow corpus. Its
existing test only runs Node applications, so the new
`compiler_arity_resume_extended.rs` driver executes each selected whole source
in both QuickJS and Node, in applications and actual `defmacro` computations,
with fresh and reused MacroHost lifecycles. A real macro warmup precedes reused
execution. Original JSON literals are compared as exact strings after the
original result is produced. The original Node memory option and default
evaluator options are retained; the 50000-step trampoline is unchanged.

| Complete original workflow | Demanded new operations |
| --- | --- |
| Trampoline and comparator | `comparator`; returned callbacks and all original callback effects |
| Map metadata updates | `update-keys`, `update-vals`; result maps, metadata, nil inputs and original map |
| Vector partitions | `partitionv` at two/four arguments, `partitionv-all`, `splitv-at`; full results and padding |
| Iteration order | `iteration`; reduced and full consumption with original step/value/key effects |
| Tagged values | local `tagged-literal` and `tagged-literal?`; original lookup/hash/printing and identity controls |
| Printing | `with-out-str`; every original string, print setting and nested capture |
| JS conversion | `clj->js`, both `js->clj` arities, genuine converter receiver methods; original options, nested values, Date identity and metadata |
| Munging | `munge`, `munge-str`, `demunge`; reserved names, source symbols, dot spelling and astral Unicode |
| Reduced cat | `cat`; returned reducer, termination and complete `[1 2 3]` effects |
| Subvec metadata and source | two-argument `subvec`; nested view, source vector identity, clone, metadata and original values |

The converter workflow separately proves genuine `IEncodeClojure` receiver
`custom` and `IEncodeJS` receiver `out` dispatch. The returned `true` and literal
property 7 are demanded by the converter calls. The declared `-key->js` method
returning `"unused"` is never demanded and receives zero credit. Method
definitions, receiver identity alone, function-valued operator spellings,
quoted forms and metadata registration cannot grant passes.

The final executed proof is
`/tmp/light-cljs-125-arity-extended-verified-v3/receipt.json`, SHA-256
`1cae8da54a11c75356c7cefe7ccd974343b43f17e0f2e4240c67375938067533`,
against production HEAD `8be52ad6d2e37e0c9e9a7a64a61244f14f7094d9`.
It binds current compiler/build/library source hashes, exact helper/driver/test
executable hashes, all **80 ordered actual source/phase/engine frames**, module
and emitted JS hashes, complete original sources/literals, genuine receiver
proofs and all **20 passing required-pin outcomes**. The adjacent JSONL files
contain parent-owned integration candidates. Raw output and reference outcomes
are retained at `/tmp/light-cljs-125-arity-extended-verified-v3.log`.

Actual verification:

```sh
LIGHT_ARITY_EXTENDED_ARTIFACTS=/tmp/light-cljs-125-arity-extended-actual-v2 cargo test --locked --test compiler_arity_resume_extended complete_core_helpers_execute_in_actual_phases_and_lifecycles -- --exact --nocapture
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_arity_resume_extended.py -v
python3 -B tests/reference/compiler-language/arity_resume_extended.py --export-owned /tmp/light-cljs-125-arity-extended-candidates
```

The initial actual execution passed in 9.91 seconds, one test with zero failures
or ignored tests. The provenance-bound final replay repeats that same exact
test using `--message-format=json`, preserves its binary identity, checks every
raw frame and reexecutes both pins through `report.reference_cases` with the
unchanged exact whole-source contracts and timeout 600. The seven focused
guards pass in 0.096 seconds. They reject altered original loops/literals,
changed public signatures/options, missing/reordered/failed actual frames and
undemanded receiver method declarations.

An initial draft driver failed to compile because it referenced unavailable
`serde_json`; the final driver quotes the unchanged compact fixture literal
without adding dependencies. Early export attempts rejected a definition-order
mistake and an overly strict ASCII-only reserialization check. Those attempts
supply no passing evidence. The final frozen guard preserves the original
Unicode JSON text and literal hash, with no changed expectation or source body.

These are current focused candidates, not comprehensive strict acceptance.
Unreviewed arities, source branch macros and unrelated existing helper bodies
stay uncredited. No new production discrepancy is asserted. No cancellation or
limit suite, 156 investigation/repair, shared ledger write, commit or queue
mutation was performed.
