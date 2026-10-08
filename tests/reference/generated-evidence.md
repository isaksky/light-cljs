# Bounded generated and metamorphic coverage

`tests/support/generated.rs` constructs original test sources for the resolved
core. It does not interpret programs or add a compiler IR. Both compilers use
`tests/support/execution.js`, the existing outcome protocol and timeout runner;
the reference uses the existing `execution.clj cases TSV` mode. The ordinary
244 source-derived/hand-authored runtime cases remain a separate corpus.

## Deterministic inputs and limits

Generator **v1** uses a wrapping 64-bit linear congruential sequence with
multiplier `6364136223846793005`, increment `1442695040888963407`, and the upper
32 bits modulo each choice count. Routine seeds are decimal
`0, 1, 2, 3, 17, 42, 99, 255, 1024, 65535, 24301, 18446744073709551615`.
Each seed produces four programs, for **48** cases total. No search or retry
loop filters generated programs. A failing seed is never discarded.

[generated-origins.tsv](generated-origins.tsv) records each seed's version,
original-source FNV-1a fingerprint, form count and depth. The fast manifest test
checks these receipts; FNV is a reproducibility check, not a security hash.
These are original generated origins, separate from the upstream provenance
sidecar. The table makes no claim that a generated source came from upstream.

| Dimension | Bound |
| --- | --- |
| Random generation | Two subexpressions per seed, each with depth parameter 4 and at most 32 forms. Child calls decrease depth and partition the available form budget. |
| Complete original | At most 100 forms and form depth 12, including the observation context; asserted for every selected seed. The fixed corpus actually ranges from 29–54 forms and depth 8–11. |
| Source | At most 8192 UTF-8 bytes per transformed source; all are single-line TSV-safe sources. |
| Functions | Fixed arities 0–2; parameters and return values are scalars. Intermediate expressions can hold callable values with a tracked arity. |
| Recursion | Disabled: anonymous functions only, no globals, self names, mutation or callable parameters. A binding is absent from its own initializer, so captured calls can only reach earlier lexical bindings. |
| Routine execution | 12 seeds × original plus three transformations. One batched Node process per compiler, with a 30-second timeout. |
| Reference build | 180 seconds per selected baseline by default; the existing positive `LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS` override is retained. |
| Manual replay | One seed, or at most 12 complete four-variant groups from a TSV capped at 512 KiB and 8192 bytes per source. |

Scope entries carry a binding identity and either scalar or callable-with-arity
shape. Only visible, shape-compatible references are selected. Calls supply the
known number of scalar arguments. Scalar literals include both false values,
truthy zero/empty strings, negative zero, finite numbers and an escaped Unicode
string. Construction uses `if`, `do`, nested sequential `let*`, fixed-arity `fn*`,
references, calls and the host `observe` callback. No library/macro expansion,
raw JavaScript, arbitrary host calls, collections or probabilistic termination
are involved. The source tree exists only to construct and rename test forms.

Every seed also has a fixed context with distinct initializer, callee, argument,
function-body, captured-value and end markers. Its closure captures an earlier
binding and reads its parameter. Random subexpressions run in both initializer
and function-return contexts. This guarantees observable ordering even for
seeds whose random choices are mostly literals.

## Comparisons and failures

Each original is compared with:

- **Alpha-renaming:** all binding identities and their references use a fresh,
  injective spelling. Both alphabets exclude the free `observe` callback.
- **Identity binding:** `(let* [identityFresh ORIGINAL] identityFresh)` evaluates
  the source once. The introduced name cannot occur in generated sources.
- **Single-form do:** `(do ORIGINAL)` preserves the expression's value/effects.

The fast test compares the entire result, trace and stdout record after removing
only the different case ID. The selectable reference test checks the same
metamorphic relationships independently in standard ClojureScript, then compares
all four variants with light-cljs. `both` checks release versus checkout first.
All generated cases must return; even matching exceptions fail. Missing,
duplicate, reordered or extra outcomes fail. Unresolved sources fail compilation;
crashes, timeout, missing tools and harness failures fail the run. There are no
accepted-invalid cases mixed into this valid-program comparison.

An independent comparator probe swaps callee/argument trace order while leaving
the final value unchanged, changes a returned status to a thrown status, and
removes outcomes: each must fail. Existing runner probes cover process exits,
timeouts, unsupported serialization and retained active-case diagnostics.

IDs contain generator version, decimal seed and variant. Compile/comparison
failures print the relevant ID and source; process failures retain the active
ID/source in stderr. The input directory retains `cases.tsv` on failure.
Reference failures additionally retain both reference builds, settings,
invocations, generated sources, results and the Node driver. A construction
failure prints its seed before any source can be produced.

## Run and replay

```sh
# Bounded Rust/Node checks; no Java required.
cargo test --test execution generated -- --nocapture

# Required CI selection already runs every ignored execution test, including this one.
LIGHT_CLJS_REFERENCE=release cargo test --test execution -- --ignored --nocapture

# Compare the generated corpus with both separately identified baselines.
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --test execution generated_programs_match_standard -- --ignored --nocapture

# Independently regenerate just seed 42 and its three counterparts.
LIGHT_CLJS_GENERATED_SEED=42 \
cargo test --test execution generated_programs_preserve -- --nocapture
LIGHT_CLJS_GENERATED_SEED=42 LIGHT_CLJS_REFERENCE=release \
cargo test --test execution generated_programs_match_standard -- --ignored --nocapture

# Replay an exact retained or manually reduced quartet without regeneration.
LIGHT_CLJS_GENERATED_REPLAY_TSV=/absolute/path/reduced.tsv \
cargo test --test execution generated_programs_preserve -- --nocapture
LIGHT_CLJS_GENERATED_REPLAY_TSV=/absolute/path/reduced.tsv LIGHT_CLJS_REFERENCE=release \
cargo test --test execution generated_programs_match_standard -- --ignored --nocapture
```

Seed and replay selection are mutually exclusive. A replay TSV has precisely
`ID<TAB>SOURCE` rows in complete ordered groups ending in `-original`, `-alpha`,
`-identity`, `-do`, with a shared ID prefix. Incomplete groups are errors.
Comment lines beginning with `#` and empty lines are permitted. The normal
fixed-seed manifest test still checks the complete recorded corpus.

## Reduce and preserve a discovered failure

This is a manual reduction path, not a random retry or mismatch filter:

1. Copy the retained `cases.tsv` out of the temporary directory and keep only
   the failing seed's four rows. Save the exact generator revision, seed,
   baseline identity, settings, original source and mismatching records.
2. Use the replay command to establish that the failure persists independently.
   For a runtime mismatch, both sources must still compile and return. A new
   unresolved name, arity mistake or parse error is not a successful reduction.
3. Remove one unused binding, discarded form, or branch at a time; replace a
   complex scalar with `nil`, or a call with one of its scalar results. Preserve
   referenced binders and the trace markers that expose the failure. Regenerate
   the three counterparts of each proposed original by applying the documented
   transformations, then replay. Keep a reduction only when the *same* observable
   mismatch persists. Stop after one pass through remaining subforms finds no
   smaller reproducer; the source-byte bound makes this a finite procedure.
4. Commit the reduced quartet in a focused
   `tests/fixtures/generated-<behavior>.tsv`. Add a named fast test in
   `tests/support/generated.rs` using `replay_cases(include_str!(...))`,
   `compiled_cases` and `equivalent`; add its reference check to the ignored
   generated test with `compare_reference_cases`. Include the literal expected
   result/trace from the verified reference to prevent both implementations from
   agreeing on a future regression. Record the version, seed, original
   fingerprint, generator commit and reduction rationale in this generated
   evidence document, separately from imported/adapted upstream origins.
5. Repair the compiler and run the focused regression, the normal generated
   corpus and the reference comparison. Never update an expected value merely
   to match the compiler under test. Classify any true reference limitation
   explicitly, retaining its reproducer outside the passing corpus.

No compiler mismatch was discovered in the recorded v1 corpus, so there is no
invented minimized regression or fabricated upstream attribution.

## Local evidence

Validated on **2026-10-08** with Rust/Cargo **1.99.0**, Node **24.18.0**, Clojure
CLI **1.12.4.1618**, Clojure **1.12.4** and Java **25.0.2**. The pinned Maven
release reports ClojureScript **1.12.134**. The clean research checkout at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` reports **0.0.135109660**. Both assert
`:optimizations :none`, `:static-fns false` and `:def-emits-var false` in the live
analysis environment.

The 48 generated programs agreed against both baselines, including all values,
traces, stdout and metamorphic counterparts. The focused two-baseline comparison
took about 9 seconds locally; routine generated checks took under 0.1 seconds.
The existing CI conformance job picks up the new ignored test automatically.
These are local results; no hosted CI execution is claimed.

Final verification also passed `cargo test --locked`,
`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` and
`git diff --check`. The complete ignored execution suite passed **7 tests** with
`LIGHT_CLJS_REFERENCE=both` (the same checkout path/revision above): the original
244 runtime cases, all 48 generated variants, 119 boundary classifications and
four namespace comparisons. Single-seed fast replay for seed 42 passed.
A manually reduced four-row replay passed both the fast comparator and Maven
comparison; changing only one variant's trace and removing its fourth row each
failed as required. These replay checks exercised the documented reduction
format without introducing a synthetic failure into the valid corpus.
