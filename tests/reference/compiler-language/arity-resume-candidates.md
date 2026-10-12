# Narrow whole-workflow arity candidates

This independent Pellet 125 cohort exports **24 candidate operation links**
(15 public declaration/signature rows and nine local API rows), 24 exact phase
requirements and nine unchanged whole-source pin oracles. It writes no shared
catalog, report, manifest or ledger file and adds zero source assertion credits.
The production baseline is `8be52ad6d2e37e0c9e9a7a64a61244f14f7094d9`.

The helper's explicit table selects demanded calls from existing complete source
workflows. Receiver methods, returned factories/callbacks and helper definitions
are credited only where the retained workflow actually demands their results.
Source discovery alone cannot select a new row.

| Complete existing workflow | Newly bound operations |
| --- | --- |
| MultiFn construction and supplied method tables | `get-method`; each retrieved method is invoked and returns literal 41 |
| Supplied tree/range/iterator slots and effects | `compare`, `let`, `loop`; lookup demands the comparator and the consumed tree stack |
| Supplied JS tree construction and getter effects | runtime variadic and macro `js-obj`; lookup, identity and cached hashes observe the result |
| Full original transducer body | `replace` transducer construction and `==`; complete original sequence consumption and hash assertions |
| Full original multimethod body and hierarchy setup | `-get-method`; the retrieved receiver method is applied to its original arguments |
| Full original iterator helper, record and collection body | `-iterator`; map/set/queue/record receivers traverse every original comparison loop |
| Full original predicate setup table and test body | `uuid`; the constructed UUID is consumed by the complete predicate table |
| Full original reductions body | both `reductions` arities; initially reduced values, infinite inputs and reduced termination |
| Full original vector body | zero-argument runtime `js-obj` and three-argument `subvec`; sentinel identity, errors, metadata, nested views and reductions |

Nine local `core_var_metadata.tsv` rows are operation cross-checks. They establish
no complete metadata-value coverage. Thread-injected two-argument `subvec` calls
are excluded, as are unrelated source-branch macros and the unreviewed remaining
public/local rows. No distinct production repair was demonstrated by this audit.

Reproduce candidate exports:

```sh
python3 -B tests/reference/compiler-language/arity_resume_candidates.py --export-owned /tmp/light-cljs-125-arity-candidates
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_arity_resume_candidates.py -v
```

The seven focused guards pass in 0.195 seconds. They reject changed whole-source
literals, original setup text, receiver signatures, detached actual MacroHost
drivers, quoted/discarded forms, threaded arities and method declarations.

The following exact existing tests were actually rerun against this baseline;
each passed one test with zero failures and zero ignored tests. Each test retains
its complete existing fixture cohort and executes QuickJS/Node applications and
actual `defmacro` computations through `MacroHost`:

```sh
cargo test --locked --test compiler_core_factories core_factories_execute_in_applications_and_macros -- --exact --nocapture
cargo test --locked --test compiler_public_protocols public_protocol_api_arities_and_macro_phase_execute -- --exact --nocapture
cargo test --locked --test compiler_public_constructors public_constructor_api_arities_and_macro_phase_execute -- --exact --nocapture
```

Execution times were 18.10, 5.02 and 22.45 seconds, respectively. Complete logs
are `/tmp/light-cljs-125-arity-{core-factories,public-protocols,public-constructors}-8be52ad.log`.
Both checked references freshly executed all nine unchanged complete sources
using `report.reference_cases(..., pin, ROOT/'research/clojurescript', ..., 600)`;
all 18 outcomes passed. The final receipt preserves exact cases, expectations,
reference observations, compiler source hashes, driver hashes, audit executable
hashes and raw log hashes at
`/tmp/light-cljs-125-arity-resume-current-v3/receipt.json`. Its sibling JSONL files
contain the precise candidate additions for parent-owned integration. The final
raw reference log is `/tmp/light-cljs-125-arity-final-current-v3.log`.

Earlier export attempts rejected an incorrect raw `let` argument count; the
complete retained workflow actually supplies 14 arguments. The corrected exact
call binding is retained in the final candidate receipt. A separate v2 proof
attempt stopped before reference execution because it imported the wrong
snapshot provider; it supplies no passing evidence. Neither diagnostic changed
source bodies, options, literals, effects or public acceptance requirements.

These focused receipts establish candidate evidence, not a full strict report
or zero-missing completion. The parent owns integration and comprehensive gate
validation. The external 156 cancellation restriction was respected: no
cancellation/limit suites, investigations or repairs were performed, and no
queue mutation or commit was made.
