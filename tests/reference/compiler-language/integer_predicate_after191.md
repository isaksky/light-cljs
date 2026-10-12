# Whole original Closure integer predicate diagnostic after191

The complete original mixed integer table demonstrates a reproducible predicate
 discrepancy on initialized genuine Closure objects under the previously audited
explicit embedding profile. Every fresh/reused application and macro context,
in both QuickJS and Node, executes all 112 original comparisons: 98 pass and
14 fail. Fresh default release and checkout builds of the unchanged original
namespace each pass 112 assertions, with zero failures or errors.

| Genuine receiver | Predicates whose original true expectation returns false locally |
| --- | --- |
| Long zero | int?, nat-int? |
| Long positive | int?, pos-int?, nat-int? |
| Long negative | int?, neg-int? |
| Integer zero | int?, nat-int? |
| Integer positive | int?, pos-int?, nat-int? |
| Integer negative | int?, neg-int? |

All 64 original Number-row comparisons pass. All six independent instanceof
controls prove the initialized receivers are instances of the actual loaded
Closure Long/Integer constructors, and six toNumber controls confirm zero and
positive/negative 100000000000 values. There are no fake classes, constructor
stubs, Number-only replacement tables, shortened loops or changed expectations.
`src/js_runtime.js` implements int? and signed/natural int predicates using the
Number-only isInteger guard; these actual initialized objects reach that guard.
Missing public LongImpl resolution is not used as predicate evidence.

Original provenance is `src/test/cljs/cljs/predicates_test.cljs`, whole namespace
SHA256 `630745c091260bef35c6cf57540056aff75e4a8f34b382c2dd2a732a980e597e`.
Both the complete int-val-table definition and complete test-int-preds body are
retained byte for byte, including all 14 rows, eight predicate functions,
negative zero, large Numbers, genuine constructor calls, loop and assertion
message. Their hashes are respectively
`ef0594f249eb5094f5641283824eb6a6bfb3fe66c9dec755f4240acd06071479` and
`4effa47d1bb9402702f5b48342dba0c244a6846e7a5590d09b276accdc0f97ee`.
The single original lexical assertion is exercised 112 times; this diagnostic
adds zero original-site, ordinary or phase pass credit.

The local namespace uses explicit adapters outside those two original forms:
the same Integer import, a LongImpl alias to goog.module.get('goog.math.Long'),
and external deftest/is macros that collect each original comparison truth value,
predicate index and unchanged assertion message. The harness does not reproduce
all cljs.test registration/reporting behavior. A demanded macro computes the
whole original loop and quotes its actual diagnostic records. Results are not
embedded as expected constants in the test macro. Original namespace-wide
execution parity and ambient Closure loading are not claimed by local evidence.

Closure dependencies use the exact compiler-emitted bytes validated by
hosted_import_source_context.py, loaded through their genuine Closure loader.
Their source archive is google-closure-library 0.0-20250515-f04e4c0e, archive
SHA256 `451cc32a6897448c925a9c72338a7a573a874bd3da0b385eda87e81c060d7e1a`.
All 13 compiled dependency hashes, corresponding raw archive source hashes,
dependency graph/order, loader bytes and origin are retained in the catalog.
The freshly compiled original predicate namespaces from both pins retain the
same base/deps/Long/Integer dependency bytes as this audited embedding.

The source obligation remains in-scope. All four public and four local predicate
obligations remain intended; the public rows have no dialect_contract. Existing
`docs/compiler.md` says, "External Closure Long/Integer construction remains
separate from JS Number predicates." This documents the current broad limitation
and is retained alongside the complete ledger rows in
integer_predicate_after191_scope.json. It does not change the intended original
contract. This diagnostic supports a focused owner for class-aware predicate
behavior if that intended contract is pursued; policy and canonical owner are
managed by the parent. No class-loading or public LongImpl repair is proposed.

The accepting diagnostic artifacts are:

- `/tmp/light-cljs-125-integer-predicate-after191-demanded` — four complete phase
  sources, emitted modules, actual QuickJS/Node inputs and outputs, genuine
  loader, warmup, source catalog and unchanged compiler/producer snapshot.
- `/tmp/light-cljs-125-integer-predicate-after191-demanded.log` — actual named
  Rust execution, eight full frames and Cargo executable metadata. The narrow
  diagnostic runner passed in 3.43 seconds; this means collection completed,
  not that the 112 local assertions passed.
- `/tmp/light-cljs-125-integer-predicate-after191-pins/{release,checkout}` — fresh
  unchanged full original namespace builds, complete compiler-emitted trees,
  checked identity/settings, raw Node counters and exact producer hashes.

Release is 1.12.134; checkout is a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e.
Both use optimizations none, static-fns false and def-emits-var false.
The permanent failclosed proof independently reconstructs whole original phase
sources and actual engine inputs, checks 112 complete records and the independent
14-failure matrix, genuine receiver controls, both default pin commands and
full artifact censuses, archive/dependency origin and unchanged source snapshot.
The receipt is integer_predicate_after191_receipt.json, SHA256
`e5addc4c3766110eec7120ab29c7cee0cde2b43959755082de9b3e7b3e0ed5c1`.
All 15 raw tamper controls rejected and restored to the identical pristine replay;
the raw control receipt SHA256 is
`96a4a9543bb09f0de71a69e42d5329d5d3289d601ad17e43bc67a024c7b49ada`.
The actual executable also has a durable byte-identical copy identified by
integer_predicate_after191_executable_snapshot.json.

Earlier namespace-API rejection and undemanded-loader attempts remain preserved
as zero-credit history. The latter completed all four application observations,
but macro initialization lacked goog because the lazy macro environment never
demanded the top-level loader expression. Moving only the external genuine
loader invocation inside the demanded macro established all eight contexts;
no original table or body byte changed. Those startup boundaries are distinct
from the later predicate discrepancy on six initialized genuine objects.

The 23-original/131-pair arity proposal was not edited. No shared tables,
production files, queue, owner, staging, commit, push, PR or Pellet156 work was
performed. Strict completion remains incomplete and every diagnostic credit is
zero.

## Reproduction

Use fresh output directories. Python subprocesses disable bytecode writes.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/original_integer_predicate_after190.py --reference release --output PINS/release
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/original_integer_predicate_after190.py --reference checkout --output PINS/checkout
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/integer_predicate_after191.py --prepare ACTUAL
LIGHT_INTEGER_PREDICATE_AFTER191=ACTUAL cargo test --locked --test integer_predicate_after191 whole_original_integer_table_is_observed_in_actual_hosts --message-format=json -- --ignored --exact --nocapture > ACTUAL.log 2>&1
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/integer_predicate_after191_proof.py --actual ACTUAL --log ACTUAL.log --references PINS --output PROOF.json --controls
```

The audited existing compiled dependency root is recorded in the helper. An
unavailable or altered origin/profile rejects; no replacement dependency or
class implementation is synthesized.
