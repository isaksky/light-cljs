# Public `prim-seq` fixed arities

`light-cljs-157` implements the public one- and two-argument `prim-seq` entry
points with the complete pinned behavior. The one-argument form supplies zero;
the two-argument form creates an `IndexedSeq` only when the supplied offset is
less than the primitive's JavaScript `length`:

```clojure
(defn prim-seq
  "Create seq from a primitive JavaScript Array-like."
  ([prim]
     (prim-seq prim 0))
  ([prim i]
     (when (< i (alength prim))
       (IndexedSeq. prim i nil))))
```

The exact declaration SHA-256 is
`004e70e09f368c0022d8d272bab4b9d5abdf43ad78ffaffa8bf18967f5bcd5cc` on
Maven ClojureScript **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. The source locations differ:
release line **1751**, checkout/local metadata line **1752**. This is the only
adaptation to the complete local literal contract when executing the release.
The retained local metadata row SHA-256 is
`d8da108ae6f6bb26bd73ef158faf301fad4774a3ff9338718bd38d65ef706d31`; the
[receipt](compiler-prim-seq-receipts.json) retains the complete row, both exact
declarations, complete `IndexedSeq`/iterator/ES6 registration forms, fixed-arity
signatures and their hashes, original inputs, generated fixture hashes and
actual observations.

The reference runner checks the loaded compiler identity and effective
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false` while
compiling fixtures, then runs the generated JavaScript in Node. **12 cases
passed on each pin**: the unchanged original literal expression, its unchanged
historical equality wrapper, both complete original independent workflows,
seven focused contracts and the complete local literal contract. The original
[unresolved-probe file](compiler-core-unresolved-probes.json) remains byte for
byte intact, with SHA-256
`355ab486524eedf9c5fd49867d9bf297c67d020e0f6b1ed5c1cd01f0dce17584`.
The receipt retains the full original workflow sources and expectations:
`cljs-core-gate-literal-sequence-consumer-arities` and
`cljs-core-gate-literal-qualified-sequence-consumer-arities`. Both also return
the exact expected `true\n` locally as ordinary applications and macro
computations, with exit zero and empty stderr.

Coverage includes direct, `cljs.core`/`clojure.core` qualified, saved and `apply`
calls for both fixed arities; arrays, ordinary JavaScript array-like objects,
nil elements, missing/zero `length`, empty/end/past-end inputs, and UTF-16
surrogate code units. Negative and fractional offsets remain unrounded:
`first` accesses the supplied property, `count` is `max(0,length-offset)`,
`rest`/`next` increment the offset, and `nth` checks the absolute index. A nil
offset remains nil in the first property access while numeric operations
coerce it to zero. These details are independently asserted on both pins.

The resulting `IndexedSeq` retains nominal/protocol and primitive-array/offset
identity, sequence/count/index access, metadata, rest/next termination,
equality/hash, reversible traversal, ordinary/early reduction and iteration.
It observes later array mutations. Argument expressions execute exactly once
in left-to-right order, including the branch returning nil. Fractional
traversal also exposes the required local reduction termination condition:
an incremented index must stop when it reaches or exceeds array length.

The [local contract](../fixtures/compiler-prim-seq.cljs) executes in QuickJS
and Node applications and in compiler-executed macros with fresh and reused
macro hosts through
`public_prim_seq_runs_full_literal_contract_in_applications_and_macros`.
The separate original-probe/phase-root and invalid-arity/unsupported-result
tests retain located failures and successful recovery. Exactly three focused
public/local mappings and phase bindings are reconciled: public arities one
and two and the local metadata declaration. Static registration is not counted
as execution. The incomplete broad language gate retains its existing status.

Executed reference and workflow command:

```sh
python3 -B tests/reference/compiler-prim-seq.py --write --local-binary target/debug/light-cljs --output /tmp/light-cljs-157-prim-seq-reference-final
```

The fresh directory retains `release/` and `checkout/` compiler invocation,
stdout/stderr, generated source/JavaScript and Node execution artifacts. A
successful `--write` reference run also persists the checked receipt above.
Running `python3 -B tests/reference/compiler-prim-seq.py` verifies the three
focused bindings without recompiling either reference.

Local verification completed **87 Rust tests**: 34 focused compiler tests,
including the three new tests, and 53 execution tests. The 43 explicitly
ignored external-reference tests are not counted as passes. Node runtime and
core-helper checks and metadata/inventory reconciliation passed.

The nine-sample emitted-output/startup observation is retained in the
[raw benchmark JSON](../../benchmarks/results/compiler-2026-10-11/prim-seq-output-startup.json).
Each sample checks value, effects and stdout. The emitted artifact is
**346,370 bytes** (**82,486 gzip bytes**); median parse, initialization,
execution and total times are **4.446 ms**, **3.202 ms**, **7.552 ms**, and
**15.296 ms**. The primitive-only and small core-call programs each emit
**88 bytes**, with zero runtime preludes. This is a measured smoke result without a comparative
baseline claim.
