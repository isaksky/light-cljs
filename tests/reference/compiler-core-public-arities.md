# Complete public core arity workflows

The core gate now executes 80 complete workflows: 22 original upstream adapter
bodies and 58 independent literal workflows. All 64 earlier fixture rows remain
byte-identical. New reviewed names are registered after the historical
qualification pass, so adding an operation cannot rewrite an older qualified
source or its expectation.

The 16 added sources retain complete values, ordered effects, metadata, identity
and error observations. Twelve exercise transducer initialization/completion,
early termination and callback failure, state isolation and final partition
flushing, metadata-transform callback arities/failure, lazy argument realization,
and built-in/custom clone/find dispatch. Four exercise saved and qualified print
setters, exact callback strings, callback exceptions and printer restoration.
The print state workflow explicitly binds print-newline true; it does not depend
on a checked Node bootstrap default.

The public rows previously absent in both ordinary and phase evidence are:

| Operation | Declaration arity rows |
| --- | --- |
| completing | 1, 2 |
| transduce | 1 (three arguments) |
| take, drop-while, mapcat, remove, partition-all, take-nth, partition-by | 1 each |
| drop, dedupe, random-sample | 1, 2 each |
| type, -clone, -find, cons | 1 each |
| apply | 4 (five arguments) |
| vary-meta | 1 through 6 |
| set-print-fn!, set-print-err-fn! | 1 each |

These are 29 exact public declaration rows. Type evidence establishes class
identity preservation across vector cloning; it does not establish a full type
domain contract. Protocol method definitions supply no executing call counts.

All six Rust core tests pass with the current driver. Each complete expression
runs in QuickJS and Node applications and inside an actual MacroHost defmacro
body. Six Python guards verify method-definition exclusion, the unchanged 64
historical rows and exact source/expectation hashes for all 16 additions.

Both checked pins execute all 16 unchanged added workflows: release 1.12.134 and
checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e, using optimizations none,
static-fns false and def-emits-var false. The adjacent independent literals check
all values/effects/errors, allowing map print order without changing data.
The source-bound evidence is
/tmp/light-cljs-125-core16-integration/pin-receipt.json, SHA-256 30d226b9693356e9d82017f77efe5d9c605745f0805a99c8a24866d3f57a3b39.
Earlier complete proposals and failed harness/profile attempts remain retained;
the successful run does not replace their history.

The generator produces 963 reviewed operation receipts and 52 mapped literal
oracles. All 16 additions independently execute against both pins even when a
qualified workflow adds no separate obligation and therefore has no standalone
report mapping. Receipt presence alone supplies no passing execution credit.

Reproduce local evidence with:

```sh
python3 -B tests/reference/compiler-core-gate.py
python3 -B tests/reference/compiler-language/test_core_gate.py
cargo test --locked --test compiler_core_gate
python3 -B tests/reference/compiler-core-gate-mappings.py --write-owned --export-owned /tmp/core-owned-export
```

The export contains only core-owned rows for coordinated shared-table
integration. The --write-owned option writes only core-gate-call-receipts.jsonl;
shared mapping and full-case tables require the coordinated --write step.
