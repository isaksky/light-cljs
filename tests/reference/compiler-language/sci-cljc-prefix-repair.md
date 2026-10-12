# Contextual CLJC prefix consumption repair

The three complete original sources owned by light-cljs-172 now retain canonical
`:cljs` reader behavior in actual `.cljc` files:

| Unchanged original source | Both required references and all eight local frames |
| --- | --- |
| `[#_ #?@(:clj [1 2]) 3]` | `[]` |
| `'[0 ^:private #?@(:clj [x y]) z]` | `[0 z]` |
| `'[0 '#?@(:clj [1 2]) 3]` | `[0 (quote 3)]` |

Selection consumes a pending form stream. Inactive and empty selected conditionals
leave prefixes reading until a selected value exists. Active splices share their
pending values with the owning prefix. Retained discard prefix spans preserve
nested ownership and configured reader effect order; inactive branch suppression
also includes leading discarded forms. Metadata and quote spans extend through
their selected targets. Incremental source sessions read another syntactic root
only when an unfinished top-level prefix needs it, retaining collection boundaries
and located errors.

## Current proof and reproduction

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/sci_cljc_prefix_repair.py --output /tmp/reader-prefix-current
cargo test --locked --test compiler_cljc_prefixes
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_sci_cljc_source_probe.py -v
cargo bench --locked --bench compiler -- --samples 9 --iterations 8
```

The current producer retains every original expression, historical row/body/source
hash and expected value. Applications place the whole expression in `do`; demanded
compiler-emitted `upstream` macros compute that same whole expression and quote its
finite result. Both fresh and warmed/reused MacroHosts compile `user.cljc`, and
QuickJS and Node execute emitted application units. Real ClojureScript release
1.12.134 and clean checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e independently
read original bytes with `cljs.analyzer/forms-seq*`, filename `sci-original.cljc`,
`:cljs`, and asserted none/false/false optimization/static-fns/def-emits-var settings.

Twenty-three literal controls execute in both phases, both host lifecycles and
both engines, and independently agree with both pins. They cover active/inactive
scalar and splice selection, empty splices, nested discard and quote ownership,
metadata application, top-level prefix continuation and suppressed leading tags.
Five focused Rust tests also check malformed active splices, exhausted prefix
locations, recovery in MacroHosts/EvalSessions, source spans, and exactly-once
configured reader effects in `.cljs` and `.cljc` hosts.

The current receipt is `sci-cljc-prefix-repair-receipt.json`, produced under
`/tmp/light-cljs-172-reader-cljc-final-v3`. It binds current production source,
producer/driver source, exact Cargo-reported execution binaries, Node and Clojure
launchers, and retained raw original/control sources, commands, emitted code and
observations. Earlier v1/v2 production snapshots and the original nonpassing
receipts remain retained. The previous nonpassing source probe and focused-gap
receipts have not been rewritten.

The affected reader, namespace, source metadata and safe parser checks pass,
alongside the two exact conditional source/macro regressions and all six original
accounting guards. The parser nesting-limit test is excluded. Normal release
compiler performance validates 47 scenarios with nine samples and eight iterations
per sample; raw output is retained in `/tmp/light-cljs-172-performance-current`.
A compact source/executable/raw performance binding is included in the current
receipt. No cancellation or singular/plural limit suites ran.

## Acceptance boundary

The full thirty-source diagnostic now observes two original agreements and
21 canonical feature differences, including the three repaired originals; seven
originals remain unresolved. All thirty still have **zero shared acceptance
credit**. These scoped repair observations do not replace historical SCI `:clj`
expectations, establish full-language completion, or supply an accepted adapter's
fail-closed raw replayer. Unfinished ordinary owner light-cljs-125 retains those
integration requirements. Externally blocked light-cljs-156 remains untouched.
