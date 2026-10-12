# Declaration values

The production analyzer now accepts nonempty `declare` in value positions. It
uses the existing uninitialized definition expression, whose value is JavaScript
`undefined`; `(declare)` remains `nil`. Declaration registration, ownership,
metadata, forward resolution and demand roots use their existing paths. The
emitter and runtime did not change.

The unchanged SCI source is:

```clojure
[(var? (declare x)) (get (meta (declare y z)) :name) (declare)]
```

Both independently executed pins (release 1.12.134 and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`) return the complete typed vector
`[false,nil,nil]`, empty ordered effects, stdout and stderr, at
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.
The original seven SCI fields still expect `[true,z,nil]`. That expectation and
its row/body/source hashes remain immutable; original SCI semantic credit is
**zero**. This focused repair does not change the strict language ledger or
claim completion of light-cljs-125.

[Sources](declare-value-sources.json) retain eight complete independently pinned
controls. Single-name function returns and multiple declarations are
`undefined` (`undefined?` true, `nil?` true, `var?` false, `identical? nil` false).
Zero names return `nil` (`undefined?` false, `identical? nil` true). Controls also
cover nested `do`/`let`/`if`, callback effects, literal undefined callback results,
later definitions/redefinitions, metadata and forward resolution.

The Rust driver checks complete typed value/effect/stdout packets in Node and
QuickJS, fresh and reused application engines, and fresh and reused macro hosts
for two cycles. Its demanded macro runs the complete source and serializes its
packet **inside the macro phase**, preserving undefined values before expansion
serialization. The original and six other controls agree across both phases.
The forward control reads an uninitialized root: applications see undefined,
while macros retain the intended `unavailable definition: cljs.user/y` failure.
Its distinct thrown packet receives no canonical macro agreement credit.
Separate tests verify the exact demand failure, current-namespace ownership,
invalid names, subsequent recovery, pending/unrelated-root demand and source
generations.

Both pins separately reject the direct single-name binding
`(let [d (declare x)] [(undefined? d) (nil? d)])` because they emit `var d = ;`.
The production compiler accepts it consistently as undefined. The separate
foreign-namespace control remains an analysis rejection. These reference phase
failures are retained in the [receipt](declare-value-receipt.json).

The receipt preserves all original SCI fields, complete both-pin observations,
source hashes, compiler settings/identities, raw artifact hashes, actual reference
executable hashes, the tested local executable hash and 64 phase/session packets.
Eight mutation checks reject changed sources, original expectations, credit,
packets, missing frames, raw artifacts and compiler provenance.

Run the focused local proof and verification with:

```sh
cargo test --locked --test compiler_declare_values -- --nocapture
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/declare_values.py \
  --reference-root /private/tmp/light-cljs-191-final-pins-v1 \
  --phase-log /private/tmp/light-cljs-191-driver-v4.log \
  --output /tmp/declare-value-receipt.json
```

The reference tree retains fresh complete source/compiler/execution artifacts;
`sci_execution.reference_cases` regenerates each pin from the frozen source rows.
Temporary paths and executable hashes describe this run and are not portable
installation requirements.

The compiler benchmark now validates the complete original vector's printed
result in Node before measuring its phases. Nine samples with four iterations
measure medians of 1.052µs parsing, 38.458µs analysis, 16.469µs emission and
55.865µs total compilation. The unchanged integer control totals 1.927µs.
These are resident release-build measurements on this Mac VM, not execution
throughput or a claim of improvement. The full existing benchmark's macro and
namespace workloads also completed their output/checksum validation.
[Measurement summary](declare-value-performance.json) retains distributions.
