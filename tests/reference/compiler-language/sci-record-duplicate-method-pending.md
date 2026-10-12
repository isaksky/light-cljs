# Repeated record protocol method dispatch

The rejection below is retained historical evidence at production HEAD
`d838fca9518d01877fab1d0d5e4fb7746bc288dd`. The focused production change now
selects the last protocol method for each dispatch key and arity. Current
execution evidence and its reproduction command are recorded below; this
single original does not establish the comprehensive 125 gate.

The portable original `sci-record-method-duplicate-method` is unchanged:

```clojure
(defprotocol P (p [x])) (defrecord R [x] P (p [_] x) (p [_] 9)) (p (->R 3))
```

Source SHA-256 is
`a665b0da5f38af34aa5198dbe3e476edb3703dc2fdb1bc98bc8e6feb005965a6`;
original corpus row SHA-256 is
`afbfd88e0edf6735ff6783f2a4550ba9b90eeeed2d1b87993ed36cc3f005aaf9`;
original disposition row SHA-256 is
`91595b83c937b97adcbccba11672f288e289a9c6f553c6d023dac2ae0b489119`.
The original outcome is integer 9, with empty typed effects and stdout/stderr.

Both required pins accept the complete original source in `user` and return 9
with `optimizations:none/static-fns:false/def-emits-var:false`. No optional core
exclusion, library, renamed receiver, dropped implementation or changed result
is supplied. At that captured HEAD, ordinary namespace compilation rejects the second method
with the located diagnostic `duplicate protocol method arity`, empty stdout and
exit 1. Fresh/reused actual MacroHosts reject it both when the original declaration
setup remains top level and when the entire unchanged contiguous original executes
inside a macro `do`. A clean macro compiles after each retained rejection.
No application engine execution or macro pass is inferred from a compile failure.

This is a specific pinned public record implementation discrepancy. The group
keeps protocols/types/records/extensions in intended scope. The record contract
promises pinned user protocol method behavior and does not retire repeated
same-arity implementations. Earlier native verification described stricter
validation, but that historical infrastructure cannot override the current
pinned language contract. This result does not imply relaxing genuine duplicate
protocol signature declarations, unknown protocols, missing receivers, invalid
method arities or duplicate/immutable field diagnostics.

All source, pin identity/settings, emitted dependencies, raw observations,
current compiler/CLI/audit-driver/binary hashes, four phase rejections, clean
controls, empty open duplicate searches and proposed focused acceptance are at
`/tmp/light-cljs-125-record-duplicate-method-confirmed-gap.json`, SHA-256
`c56459058e24afd870a2455ab73afef4412b855bca5a3a8c23d970fbc668811d`.
Raw artifacts are retained under:

- `/tmp/light-cljs-125-record-declaration-candidate-local-v1`
- `/tmp/light-cljs-125-record-declaration-candidate-reference-v1`
- `/tmp/light-cljs-125-record-duplicate-method-phase-v1` and its matching log.

Focused acceptance must preserve this complete original and literal 9, execute
both application engines and fresh/reused MacroHosts, and follow canonical
replacement of the existing method/arity by the later implementation. It must
protect other arities, receiver fields, declaration bundle publication, ordered
once-only effects, source/frontier/generation visibility and recovery after
failure. Existing malformed declaration/method tests remain required. This
cohort implements no production repair and changes no original row. After the
fresh live duplicate check, Pellet **light-cljs-165**, **Accept repeated record
protocol method arities with canonical last-method dispatch**, was added
immediately before 125. The live queue owns its status and implementation;
these artifacts retain the demonstrated discrepancy rather than a task list.

## Current executable evidence

The unchanged source returns **9** in ordinary namespace execution and genuine
computed MacroHost results, using both original top-level declaration setup and
the entire contiguous original inside macro `do`. Every adapter executes on
QuickJS and Node with fresh and warmed reused MacroHosts. Empty effects and
stdout/stderr remain literal expectations. Four complete slot sources across six
phase/lifecycle variants, plus one complete generation source across four,
provide **28 compilations and 56 engine observations**.
The other sources protect receiver fields, existing/current instances, unrelated
methods and arities, and ordered effects, and independently establish the
related `deftype`/`reify` protocol replacement behavior in both required pins.
Existing Object-method duplicate diagnostics are outside this protocol-slot
change. Malformed-method failure and clean compilation use the same host after
every variant.

The checked producer runs both required pins at
`optimizations:none/static-fns:false/def-emits-var:false`, checks immutable original
source/row/expectation hashes, retains raw source, emitted JS, engine observations,
build output and actual executable hashes, and rejects producer drift during the
run. It records no comprehensive language pass or excluded original obligation.

Current receipt:
[`sci-record-duplicate-method-receipt.json`](sci-record-duplicate-method-receipt.json),
SHA-256 `8aaee6479f33194e6e0445041d35769179fac38856e6d361d4ff45c4a799b1a8`,
with raw artifacts at `/tmp/light-cljs-165-verified-v4`. All ten reference
observations and all 56 current engine observations passed. Original corpus and
SCI disposition rows remain unchanged. The inventory refresh changes only the
current analyzer hash; it grants no coverage.

The complete generation source preserves old/current methods, saved factories
and assoc copies in ordinary application and entire-source macro execution,
fresh/reused. In that historical receipt its split top-level-setup macro adapter was separately
nonpassing: old instance method3 becomes13. The independent source without any
repeated methods has the identical discrepancy, while both pins and complete
sequential execution return `[3 17 15 14]`. This independent phase-root gap has
focused live owner **light-cljs-166, Preserve source-visible record generations
in demanded macro initializers**, immediately before125. Raw complete-source/
pin/actual-computed-macro evidence is `/tmp/light-cljs-165-generation-gap-v2`;
the namespace-driver failure is `/tmp/light-cljs-165-verified-v3/audit`.
Those split-generation observations receive no passing receipt frames or credit.

Focused record/protocol/type/reify/extension/core-extension/bootstrap/macro
regressions passed **150 tests**, with five filtered cases, and the atomic
declaration-product bundle test passed separately. Direct Node runtime checks,
source inventory freshness, changed Rust formatting, and whitespace checks
passed. The macro filter accidentally included two singular-named `_limit_`
tests; both passed and were not repeated. No cancellation/native-crash suite was
run. Whole-repository formatting and all-target Clippy remain nonpassing because
of existing formatting drift and 55 diagnostics outside the changed code.

Normal validated compiler performance smoke produced **423 timing samples across
47 phase pairs** (nine samples, eight iterations) at
`/tmp/light-cljs-165-compiler-bench.csv`. Collection total compilation median was
0.0465 ms; fresh/reused macro analysis medians were 22.928/22.851 ms. The existing
record/type/reify/protocol cost driver validated 14 programs with nine fresh V8
parse/evaluate samples each at `/tmp/light-cljs-165-record-costs-v1`; record-update
and record-method medians were 9.123 and 8.861 ms. These are current local smoke
measurements with unpaired historical baselines, not improvement claims.

```sh
python3 -B tests/reference/compiler-language/sci_record_duplicate_method.py \
  --output /tmp/light-cljs-record-last-method-fresh
```


## Subsequent generation repair

The original split-generation failures above remain retained, unchanged. The
source-visible initializer generation repair now executes those exact sources
without excluding the split adapter. See
[sci-record-generation.md](sci-record-generation.md) and its separate receipt
for current evidence. The older 165 receipt retains its original scope and
zero credit for the formerly excluded frames.
