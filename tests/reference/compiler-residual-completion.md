# Remaining complete residual body evidence

On 2026-10-11 at starting HEAD `9f9a3c5`, the standalone
`compiler-residual-completion.py` adapter reproduced three complete pinned
assertion-free bodies and one complete macro module. It verifies original
source spans and hashes against checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. Its proposal file contains no
new source-assertion witnesses or invented assertion counts.

| Complete source | Retained setup and executed oracle |
| --- | --- |
| `core_test.cljs::var-desugar-test` | Both original dotted return-position expressions execute; actual final value `true`. |
| `core_test.cljs::test-cljs-3313` | Original `ExtMetaProtocol` definition and entire original `PersistentArrayMap` extension; actual metadata-precedence result `true`. |
| `core_test.cljs::test-new-method` | Original global `Object` alias is bound to the identical ECMAScript constructor, `(def Object js/Object)`; original constructor-function call returns `true`. |
| `array_access_test.cljc` | Entire 591-byte original macro module preserved byte-for-byte; supplemental invocation returns `[true true]` for nil error-printer binding and subsequent restoration. |

The array-access driver uses ordinary `:require` locally. Its separately hashed
canonical driver uses `:require-macros`, required by the upstream JVM macro
compiler. Only that import clause differs; the original module and executable
supplemental contract are identical. The initial ordinary-import reference
attempt returned unevaluated macro data and is not passing evidence. The final
reference tree and explicit adaptation are recorded in the case proposal.

Final checks passed:

```
python3 -B tests/reference/compiler-residual-completion.py --check --catalogs
cargo test --test compiler_residual_completion remaining_complete_residual_bodies_execute_in_both_phases -- --exact --nocapture
```

The final exact Rust test passed in 3.12 seconds, executing all four rows
through both QuickJS and Node, first as ordinary applications and then as
complete QuickJS macro computations whose returned results execute in both
application engines. Log:
`/tmp/light-cljs-125-residual-completion-local-final.log`.
Fresh reference evidence:
`/tmp/light-cljs-125-residual-completion-reference-final/evidence.json`.
All four contracts passed release 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` with checked
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
Reference observations cover the ordinary contract; local exact tests supply
the application/macro phase evidence.

The generator defaults to proposals and fixture reproduction. `--catalogs`
checks the registered owned rows. Only coordinated `--write --catalogs` writes
those shared catalog rows, preserving unrelated generators' rows and order.

## Exact semantic omissions for focused owners

`compiler-residual-completion-omissions.json` retains complete source probes,
every original assertion and relevant setup form, original source hashes,
literal pinned expectations, and suggested scope. The original failure receipts remain intact; the repaired Partial probe is now
registered only after fresh successful application, macro, and both-pin execution.
Fresh original failure receipts verified exact source identity against this generated file:

- Both-pin receipts:
  `/tmp/light-cljs-125-residual-completion-omissions-reference/evidence.json`.
- Current local ordinary/macro and QuickJS/Node receipts:
  `/tmp/light-cljs-125-residual-completion-omissions-local.json`.
- Receiver diagnostics:
  `/tmp/light-cljs-125-object-equiv-local-receivers.json`.

These two semantic omissions received focused owners before light-cljs-125.
The variadic IFn omission is repaired below; light-cljs-150 still owns
persistent collection Object `.equiv`. Original reproductions were:

1. `core_test.cljs::test-cljs-2133` with complete original `Partial` setup
   returns **3** in both references. All four local engine/phase paths at `ce1487c`
   failed with **`2 is not ISeqable`**. The focused scope is variadic `IFn`
   `-invoke` rest binding and invocation. Probe SHA-256:
   `8c95f33171bb8525c6d059fc5d668c302abad04fa4a8a544b296ac5d91592754`.
2. `interop_test.cljs::test-obj-equiv` retains all seven original assertions
   under the normal `LITE_MODE false` configuration. Both references return
   **true**; all four local engine/phase paths fail at the first map `.equiv`
   call. Isolated current probes identify missing `.equiv` on maps, vectors,
   and sets. Keywords, symbols, lists, and lazy sequences pass. The focused
   scope is those public Object interop methods, preserving complete body and
   ordinary/macro execution. Probe SHA-256:
   `18ab06bef880d002aaa5584b75724664bc0cd0aacd0bf61894e6bcc3aee08d40`.

## Variadic IFn repair and complete-body integration

On 2026-10-11 from starting HEAD `ce1487c`, light-cljs-149 repaired variadic
`deftype`/`reify` callable methods. Both the `.call` and direct `-invoke` entries
now bind their source rest parameter to an indexed sequence or nil; fixed IFn
slots retain their previous receiver/recur behavior. A separate variadic call
entry handles unmatched arities in overload sets. Both pinned compilers lose
instance fields on this overloaded direct variadic path, while the matching
`apply` slot preserves them; the literal tests retain this observed distinction.

Saved variadic call/invoke functions expose their bounded sequence `apply`
entries. Seven supplemental contracts cover zero/one/multiple rest values,
fixed prefixes and overloads, actual borrowed receivers, saved callable values,
metadata copies, ordered effects, infinite inputs through saved sequence entries,
and passing a lazy value without premature realization. Callable-object `apply`
itself still uses the canonical eager argument-array path; no infinite eager
application is advertised as terminating. Ordinary variadic `fn` application
remains lazy. Macro supplemental tests compute `pr-str` before returning the
result, retaining the observation of undefined missing slots without trying to
export undefined as a supported macro form.

The unchanged complete `Partial` setup and `test-cljs-2133` body now return **3**
in Node/QuickJS applications and when computed by QuickJS macros, with probe
SHA-256 `8c95f33171bb8525c6d059fc5d668c302abad04fa4a8a544b296ac5d91592754`.
The generator adds this fifth residual fixture and only its assertion-free
body/phase/reference mapping; it creates no source-assertion witness or pass.

Reproduce both-pin evidence (eight contracts, all passed per pin):

```
python3 -B tests/reference/compiler-ifn-rest.py --output /tmp/light-cljs-149-reference-current
cargo test --test compiler_ifn_rest --test compiler_residual_completion
python3 -B tests/reference/compiler-residual-completion.py --catalogs
```

Checked evidence is retained at
`/tmp/light-cljs-149-reference-current/evidence.json`, with full source hashes,
local fixture/test hashes, identity and settings receipts for release **1.12.134**
and clean checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**,
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.
Focused tests also prove located invalid-form diagnostics, recovery after failed
runtime calls, and successful fresh/reused macro hosts after a macro throws.
Current regression verification passed `execution`, `analyzer`, `compiler_types`,
`compiler_reify`, `compiler_protocols`, `compiler_extensions`, `compiler_records`,
`compiler_macros`, `compiler_core_helpers`, and `compiler_deferred`. The execution
suite's 43 explicitly ignored reference tests were not counted as passes.
Both standalone JavaScript runtime/helper suites passed. The resident compiler
benchmark completed 423 checked samples (`--samples 9 --iterations 1`), retained
at `/tmp/light-cljs-149-compiler-bench.csv`; this is a performance smoke run,
not a quantified before/after speed claim. Catalog/source/support validation,
residual generators, inventory `--check`, focused Rust formatting and
`git diff --check` passed.

Other residual missing obligations and the unfinished light-cljs-125 full gate
remain open; this single repaired body does not establish full completion.

## Retained gaps and boundaries

The complete integer-predicate table also passes both references and fails
current compilation at unresolved `cljs.core/LongImpl`. Its original setup
includes `goog.math.Long` and `goog.math.Integer` representations, all fourteen
rows, eight predicate columns, and full loops. It remains unwitnessed pending
an explicit host-input boundary review. This does not establish a defect in
native JavaScript-number predicates or assign another feature owner. No table
row or input representation is replaced. Its exact probe and receipts are
retained alongside the two semantic omissions above.

`test-cljs-2549` reads upstream explicitly private `cljs.core/tapset`, declared
with `:private true`. Current tap support stores listeners differently. No
invented `tapset` setup or substituted public assertion covers those two
original sites; they remain unwitnessed pending their documented scope basis.

Both generative parse bodies request **100,000** `test.check` trials. Their
complete generator/property/shrinker dependencies are not supplied by the
current local adapter. They remain unwitnessed; no sampling, trial reduction,
or replacement numeric inputs are used. The generative string body similarly
retains its original generator dependency and remains uncovered.

The remaining assertion-free inference body depends on its complete upstream
JVM inference helper macros and `cljs.pprint` predicates. It is not replaced
with precomputed predicate outcomes or a compiler-only pass.

The unchanged `invoke_test.cljs` module was separately compiled in isolated
source roots for both references. Both throw a `TypeError` during module
initialization at their emitted `declared_fn.call` site, before the later record
forms can execute. The complete module declares that function but never defines
it. Direct initialization receipts and checked compiler outputs are retained at
`/tmp/light-cljs-125-invoke-program-isolated-reference/initialization-errors.json`
and its sibling release/checkout directories. This establishes the upstream
runtime limitation; it does not claim local execution coverage or rewrite
the source-program obligation as complete.
