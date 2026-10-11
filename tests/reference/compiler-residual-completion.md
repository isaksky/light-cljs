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
The variadic IFn and persistent collection Object `.equiv` omissions are repaired
below. Original reproductions were:

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

## Persistent collection Object equiv repair and complete-body integration

On 2026-10-11 from starting HEAD `f83237f`, light-cljs-150 added public
Object `.equiv` methods to the persistent map/vector/set prototypes. They
consult the actual receiver's `IEquiv` method before default structural
comparison, preserving overridden equality even for identical receivers.
Default `.equiv` also compares the contents of an identical collection, including nonreflexive `NaN` contents; language `=` retains its identity shortcut. Saved functions dispatch through
the borrowed receiver, and metadata copies/public constructors inherit the
same methods. The compiler/adapter does not rewrite `.equiv` to `=`.

The unchanged complete non-lite `interop_test.cljs::test-obj-equiv` probe
retains all seven original assertions and source SHA-256
`18ab06bef880d002aaa5584b75724664bc0cd0aacd0bf61894e6bcc3aee08d40`.
It now returns **true** through Node/QuickJS applications and when independently
computed by QuickJS macros before execution in both application engines.
Nine supplemental literal contracts cover map kinds, separately constructed
values, empty/equal/unequal values, vector/list/lazy-sequence compatibility,
metadata, positional constructors, saved/borrowed methods, custom equality,
hash/key membership, identity and ordered lazy effects and nonreflexive contents. A separate test checks
application failure/reuse and fresh/reused macro-host recovery.

Reproduce the exact source, mappings and both-pin evidence:

```
python3 -B tests/reference/compiler-object-equiv.py --catalogs --run --output /tmp/light-cljs-150-reference-final
cargo test --test compiler_object_equiv -- --nocapture
python3 -B tests/reference/compiler-residual-completion.py --catalogs
```

Both pins pass all ten contracts, with verified release **1.12.134**, clean
checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, and
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.
Complete local source and fixture hashes accompany the fresh reference receipts.
The generator registers seven original assertion mappings only after proof.
The source case also retains its complete body and exact hashed
application/macro execution test mapping. Assertion-bearing bodies have no
independent `body::` obligation in this ledger, so none is fabricated.

The runtime/helper suites and affected compiler interop, equality, construction,
Object/protocol/extension, record, macro, sequence, collection and execution
regressions passed. Focused logs and reference receipts are under
`/tmp/light-cljs-150-*`. The compiler performance smoke run uses nine samples
and one iteration with checked outputs; it is not a before/after speed claim.
Ignored reference-only tests in normal Rust suites remain explicitly ignored;
the full original body and supplemental oracles were freshly run in both pins.

Two separate demonstrated defects received focused live owners after duplicate
searches. light-cljs-151 owns overridden `IEquiv` on existing sequence/name
Object `.equiv` methods: the exact List probe returns `[false true]` in both
pins and `[true true]` locally. light-cljs-152 owns a false extension-declaration
cycle while the core `and` macro expands inside persistent collection `IEquiv`
extensions. Its original guarded source and both-pin successful receipts are
preserved at `/tmp/light-cljs-150-reference-final-guarded`; the final supplemental
collection-dispatch control uses the equivalent `if` guard. These are separate
from the seven unchanged upstream assertions and were not implemented here.
The full light-cljs-125 language gate remains incomplete and unchanged.

## Existing sequence and name Object equiv dispatch

On 2026-10-11 from starting HEAD `320307f`, light-cljs-151 routes the existing
sequence, Keyword, Symbol, and LazySeq Object `.equiv` methods through the
actual receiver's `IEquiv` implementation. The LazySeq public prototype has its
own method and is checked independently of the shared sequence prototype.
Default sequence comparison traverses contents even for identical receivers,
including nonreflexive `NaN` elements; language `=` retains its identity shortcut.
Hashing and key membership retain their existing implementations.

`compiler_sequence_object_equiv.tsv` preserves the exact List reproduction plus
18 supplemental contracts. They cover default equal/unequal and empty values,
metadata copies, hashes, key membership, saved methods captured before extension,
borrowed receivers/reify equality, and overridden equality on List, EmptyList,
Cons, IndexedSeq, ChunkedSeq, ChunkedCons, LazySeq, Repeat, PersistentQueue,
NodeSeq, PersistentTreeMapSeq, IntegerRange, Range, Keyword, and Symbol.
Every contract executes as a Node/QuickJS application and as an independent
QuickJS macro computation consumed by both application engines. Separate tests
verify failure/recovery in reused application sessions and macro hosts.

The exact full non-lite upstream Object equiv body and all seven assertions
remain unchanged, with original body SHA-256
`9d52796012fca3fe4fdf4adc8d3672db5d68e9b0ce110c54fa2e41e061df0ca2`
and adapted source SHA-256
`18ab06bef880d002aaa5584b75724664bc0cd0aacd0bf61894e6bcc3aee08d40`.
The reference script retains the full original body, exact source, provenance,
and source/test/runtime hashes. It never replaces `.equiv` with `=`.

Reproduce the comparison and focused execution:

```sh
python3 -B tests/reference/compiler-sequence-object-equiv.py --output /tmp/light-cljs-151-reference-final-receipt
cargo test --locked --test compiler_sequence_object_equiv --test compiler_object_equiv -- --nocapture
```

Both references pass all 20 cases, with verified release **1.12.134**, clean
checkout **a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, and
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.
The final reference receipt is
`/tmp/light-cljs-151-reference-final-receipt/evidence.json`, SHA-256
`757ea37f221de0981b3cac4a1e36c8225eefc4d177407f33c59d063c72fddf37`.
Its recorded source hashes match the final runtime and fixtures.

The final library plus 18 affected integration suites pass **211 tests**, zero
failures, with one explicitly ignored general reference-equality test; the
focused reference comparison above was executed separately. These suites cover
Object/equality, sequences/deferred values, interop, protocols/extensions,
public constructors, types/records/reify, macros/bootstrap and session reuse.
Both JavaScript runtime/helper suites pass. Logs are retained under
`/tmp/light-cljs-151-*`. The validated compiler performance smoke report at
`/tmp/light-cljs-151-compiler-bench-final` uses nine samples and one iteration;
this is a checked-output smoke run, not a before/after performance claim.
The earlier attempt was rejected by the runner because test source changed
during its snapshot and is not accepted as verification.

A separate independently reproduced Atom Object dispatch defect has live owner
light-cljs-153 after duplicate searches: both pins return `[false true]` for an
Atom extension rejecting itself, while local QuickJS returns `[true true]`.
Its exact source and evidence are retained at
`/tmp/light-cljs-151-atom-equiv-probe/evidence.json` and
`/tmp/light-cljs-151-atom-equiv-local.log`. Atom behavior is not implemented in
this sequence/name change. light-cljs-152's core-macro declaration cycle and
light-cljs-125's full-language gate remain unfinished with their original scope.

## Core macros and collection extension demand

On 2026-10-11 from starting HEAD `f4259e3`, light-cljs-152 runs compiler-owned
core macro computations at their bootstrap declaration frontier. Collection
operations used to build expansions no longer demand user extensions, including
the declaration whose method is currently being expanded. The existing frontier
guard restores installed user extensions and executed mutations afterward,
including failure paths. Source macros keep normal user declaration demand;
compile, root and bundle cycle detection are unchanged.

`tests/fixtures/compiler_core_extension_demand.tsv` retains the original `and`
source, including all three persistent collection extensions and saved receiver
calls. With its `pr-str` wrapper, its SHA-256 is
`d79da2669a225d77040f0de18af6438b232c45b604eed0cc1133c650588905fd`.
Fresh/reused MacroHosts execute it in Node/QuickJS applications and as actual
QuickJS macro computation. Separate tests cover ordered equality replacement,
failed extension-demand recovery and a genuine computed-target declaration cycle.

Reproduce the exact reference comparison and focused checks:

```sh
python3 -B tests/reference/compiler-core-extension-demand.py --output /tmp/light-cljs-152-reference-final
cargo test --locked --test compiler_core_extension_demand
```

Both reference pins pass: release **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, with `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`. The receipt at
`/tmp/light-cljs-152-reference-final/evidence.json` has SHA-256
`bc58f3910f965b7a8af3d84ebf6bd9ec79d382df8b3b3d09ab5d0c8951bb9fe0`;
its implementation/fixture/test hashes match the final files.

The final library plus ten affected integration suites pass **163 tests**, zero
failures and one explicitly ignored general reference-equality test. These cover
macros/bootstrap, protocols/extensions, equality and collection/sequence Object
methods. The focused two-pin comparison above was run separately. Both JavaScript
runtime/helper suites pass. Logs use `/tmp/light-cljs-152-*`; the validated
compiler benchmark report `/tmp/light-cljs-152-compiler-bench-final` uses nine
samples and one iteration, a checked-output smoke run rather than a comparative
performance claim. No runtime equality behavior or full-language evidence gate
was changed. Atom dispatch remains owned by light-cljs-153 and full-language
completion remains owned by light-cljs-125.

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

## Complete residual source contexts, 2026-10-11

The continuation from starting HEAD `83070de` adds thirteen complete original
bodies and **22 original assertion sites** to reproducible proposals. Source
witness registration remains coordinated by the main light-cljs-125 owner.
Every original source span is checked against the pinned checkout before
fixture generation. No production semantics change in these adapters.

Four additional rows in `compiler-residual-completion.tsv` retain the complete
`cljs-2866-test` loop/reset workflow, `cljs-3068-test` including original
`foo-var-3068 (merge)` setup and conditional expression, returned
`String/.toUpperCase` method workflow, and both original EDN/read-string
comparisons. Their five assertions execute. The qualified method's original
global String alias binds the identical `js/String` constructor; EDN and reader
aliases qualify to their original canonical libraries. The EDN source fixture
retains both imports. No compiler-only acceptance substitutes for execution.

`compiler-residual-source-context.py` adds five supported namespace contexts
and nine complete bodies with seventeen original assertion sites:

- Keyword reader namespace and original ordinary/macro namespace aliases.
- All four original syntax-quote assertions with the original namespace and
  `cljs.test` alias. The local empty namespace marker supplies only this
  reader context; no test function is implemented or called. References use
  the actual `cljs.test` library.
- Both `cljs.ns-test.foo` bodies, including `kw`, quoted `qkw`, and the original
  `made.up.lib` `:as-alias` clause.
- The unchanged independent `test-cljs-2283` body.
- All four remaining original macro-test bodies, including macro arglist
  metadata and the sorted-map macro result. Every required `.clj` macro module
  is byte-identical in the local `.cljs` source-macro module and in the
  canonical `.clj` reference tree. Local ordinary `:require` and canonical
  `:require-macros` are explicitly separate; original `:use-macros :only`
  becomes the corresponding `:refer` import.

Support forms retain their original top-level namespace positions. Assertion
adapters change `deftest/is` to executing `do/assert`, preserve original
operands/messages, and return true after the complete assertion-bearing body.
The dedicated Rust test copies each isolated source root, executes its complete
ordinary program, then computes the complete original body inside an actual
QuickJS macro at its original namespace before running the expansion. Both
Node and QuickJS application engines compare the literal result `true`.
Unsupported source contexts are absent from its passing TSV and retained
separately in `unwitnessed-source-cases`.

Final focused verification passed both tests: all nine completion fixture rows
in **5.78 s**, and all five supported namespace contexts in **3.65 s**, zero
failures or ignored tests. The log is
`/tmp/light-cljs-125-residual-source-final-tests.log`. Both reference pins pass
all four new standalone bodies, all seven initial ordinary namespace contexts,
and the additional complete macro source context. Identities and settings are
verified: release **1.12.134**, checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`. These ordinary reference results
supply compatibility evidence; the local tests supply the deliberate macro
phase execution evidence.

Reference receipts and SHA-256 values:

| Receipt | SHA-256 |
| --- | --- |
| `/tmp/light-cljs-125-residual-source-additions-reference/evidence.json` | `8f89d695fc7ce5c4cb293fad33217cb49ffb3edd4983e0382dbf9bc9774a6e19` |
| `/tmp/light-cljs-125-residual-source-context-reference-final/evidence.json` | `998c53cb1d19bd3e1e1ad06229d83aff8ba5b530e726a8120043dabab8b3bc3c` |
| `/tmp/light-cljs-125-residual-macro-source-reference/evidence.json` | `f9c32f12f55d9c0bbbe0887538bc4b0bd34c9d71ff05307c44df6199a6bb9f20` |

The first namespace-reference attempt stopped when a source change invalidated
its source-tree hash; it is discarded. The final receipt above compares the
frozen exact source files. Later proposal/test eligibility changes preserve
those observed source bytes. Reproduce the source generation and execution:

```sh
python3 -B tests/reference/compiler-residual-completion.py
python3 -B tests/reference/compiler-residual-completion.py --run-additions --output /tmp/light-cljs-125-residual-source-additions-reference-current
python3 -B tests/reference/compiler-residual-source-context.py
python3 -B tests/reference/compiler-residual-source-context.py --run --output /tmp/light-cljs-125-residual-source-context-reference-current
cargo test --locked --test compiler_residual_completion --test compiler_residual_source_context -- --nocapture
```

### New exact source gaps

Both copies of the complete upstream `foo.ns-shadow-test` source retain the
required namespace `baz` and same-spelled function `baz`, its zero/one-argument
recursion, `bar`, qualified call from `quux`, `id`, and function `foo`. Both pins
return true for all three original assertions. Ordinary local Node and QuickJS
throw `cannot read property 'call' of undefined`; actual macro compilation also
fails. The canonical complete adapted program SHA-256 is
`7a1d1a674faadc0982f2c3d787ffe7e8466391699dda67068c99b68cffadb479`.
No dependency or function is renamed to make it pass.

The complete original `cljs.top-level` setup retains both top-level
`let`/`defn` closures and both original assertions. Both pins and both local
ordinary engines return true. Actual QuickJS macro computation fails with
`unavailable definition: cljs.top-level/bar`. Its adapted ordinary program
SHA-256 is
`ecf36324188c46de092ddad498b959db3af3339a81a80fc97ee720b55d3ba5c5`.
Moving those setup forms into the macro would hide the namespace declaration
demand gap and is not proposed.

`--audit-local` preserves exact emitted JavaScript plus ordinary/macro source
hashes, binary identity, compiler diagnostics and both engine outputs. The
observation-only audit does not assert a failing execution as a pass. Its final
receipt is `/tmp/light-cljs-125-residual-source-local-final/evidence.json`,
SHA-256 `79ba7b872bc570dacd62f9b0d1dc305a4dd688c6b1d6f77aafa54125f38afcc3`.
Both gaps were sent to the main owner with their complete sources and both-pin
proof for focused ownership. No passing witness is proposed for these three
complete sources.

### Reviewed residual boundaries

`compiler-residual-source-boundary-review.json` retains every reviewed body,
all relevant setup forms, original namespace spans and source hashes, the
complete JVM inference helper, and the complete invocation program. It records
review bases without modifying or excluding any ledger row.

The integer table's fourteen rows and eight predicate columns remain intact:
eight native JavaScript Number rows and six `goog.math.Long`/`Integer` rows.
`LongImpl` is explicitly documented upstream as `INTERNAL: do not use`.
`int?` and its sign variants separately recognize these Closure objects;
`integer?` describes native JavaScript numbers. The adopted JavaScript-number
basis and excluded Closure ecosystem supply no Long/Integer implementation.
These inputs are neither native Number values nor the separately allowed
explicit JS BigInt interop operation. The complete original source assertion
remains unresolved. This review does not truncate JS rows, coerce host objects,
exclude the whole predicate body, or claim that independently passing native
number oracles cover this table.

The tap assertions observe explicitly private `tapset` storage and its atom/set
representation. Public tap registration/delivery remains intended; an invented
private storage adapter would not witness those source sites. The assertion-free
inference body uses the complete JVM helper to instrument emitted `truth_`
coercion calls; its purpose is compiler-emission shape rather than predicate
return values. Private emitted helper shape, `.clj` companions and general
pprint have explicit group boundary bases; public predicate APIs remain intended.
The two generative parse bodies retain **100000** trials and their full
`test.check` dependency requirements, and the generative string body retains
its complete original reference implementation. No sampling, fake generator,
trial reduction or unconditional success is supplied.

The exact eval module states it requires an embedding that installs `*eval*`.
All fourteen original assertions and live function/Var-containing setup stay
unwitnessed embedding contracts; default standalone emitted JS/reference builds
cannot supply this hook. The unchanged invocation module still throws in both
canonical initializations before its later record forms. Its undefined declared
function is preserved. A canonical throw supplies neither a local execution
pass nor an automatic source-program exclusion. The complete source gate and
all remaining acceptance under light-cljs-125 remain unfinished.

### Namespace shadow resolution repair — 2026-10-11

`light-cljs-154` repairs the qualified call in both complete original
`foo.ns-shadow-test` contexts. `quux` binds the lexical parameter `foo`, but
`foo.ns-shadow-test/bar` names the namespace Var. Analysis formerly treated
that qualified symbol as dotted access through `foo`. Dotted lexical property
resolution now applies only to unqualified symbols, matching the qualified
symbol branch before dotted access in the pinned upstream `resolve-var`.
The required `baz` namespace, same-spelled global `baz` function, every original
setup form, zero/one-argument methods, recursive call, and six original
assertion sites remain intact.

Both complete bodies pass ordinary Node/QuickJS execution and actual QuickJS
macro computation in fresh and reused MacroHosts. Their two case records,
six assertion witnesses, and complete body/phase mappings are now registered.
The source-context catalog has seven supported contexts, eleven complete
bodies, and twenty-three original sites. The complete top-level `let`/`defn`
context stays unwitnessed under `light-cljs-155`; none of its source or
expectations was changed. The full language gate remains unfinished.

The independent collision fixture adds required namespace and alias names
colliding with ordinary global functions and lexical functions, dotted aliases
whose prefix is lexical, qualified sibling calls in independent namespaces,
zero/one-argument recursive calls, ordered initialization and redefinition,
and saved earlier function values. Both pinned ordinary references return
`[3 7 1 7 7 17 [9 11 13 70 7 19 70] 90 [:before :after]]`.
Local macro tests explicitly demand the earlier definitions before replacement,
then verify the later definitions and separate application effects; a macro-only
atom mutation cannot leak into either application engine. Unknown namespace and
Var failures retain their source path and location, including an enclosing
macro declaration location, and the same host subsequently compiles and
executes the repaired source.

Reproduce the focused semantic checks and oracles:

```sh
cargo test --locked --test compiler_namespace_shadowing --test compiler_residual_source_context --test namespaces --test compiler_namespace_completion --test compiler_macros --test execution --test compiler_residual_completion
python3 -B tests/reference/compiler-residual-source-context.py --catalogs
python3 -B tests/reference/compiler-residual-source-context.py --run --case cljs-shadow --output /tmp/light-cljs-154-cljs-shadow-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case foo-shadow --output /tmp/light-cljs-154-foo-shadow-reference-current
python3 -B tests/reference/compiler-namespace-shadowing.py --output /tmp/light-cljs-154-shadow-contract-reference-final
python3 -B tests/reference/compiler-language/extract_inventory.py --check
node tests/support/js_runtime.test.js
node tests/support/js_core_helpers.test.js
```

Both pins are freshly verified: release **1.12.134**, clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, `:optimizations :none`,
`:static-fns false`, `:def-emits-var false`. Final ordinary reference receipts:

| Receipt | SHA-256 |
| --- | --- |
| `/tmp/light-cljs-154-cljs-shadow-reference-current/evidence.json` | `839a9fc3a89e3ee528f0d2d13f8e354bb1493ac7ebff2dd78f2d199654ad3e4a` |
| `/tmp/light-cljs-154-foo-shadow-reference-current/evidence.json` | `171ac78659005e535f8dcd021658f7dde5ac2878ae12a7bf662072a4303b7ca4` |
| `/tmp/light-cljs-154-shadow-contract-reference-final/evidence.json` | `0e8bbe25449f5105e1a68d88c71991c49d86ac503970fac57fd2dbd0dc302854` |

The focused seven-target Rust run passes **199 tests**, zero failures,
**43 explicitly ignored reference-only tests**. The final additional alias
and recursive fixture run passes both focused tests, and its final fresh
reference receipt above includes the final exact source. Both direct Node
runtime suites pass. Logs are `/tmp/light-cljs-154-regressions.log`,
`/tmp/light-cljs-154-shadowing-final.log`, and `/tmp/light-cljs-154-js-*.log`.
The inventory refresh changes only the analyzer source hash; generated
obligations, original expectations and acceptance rules remain unchanged.

The broader Python discovery attempt initially rejected that analyzer hash
before regeneration, and the library receipt validator also rejects the older
191 receipts' `compiler_source_sha256.src/analyzer.rs`. A read-only diagnostic
using only the baseline HEAD analyzer hash verifies all 191 older receipts,
confirming intentional current-source invalidation. That diagnostic is not
fresh library execution or a current receipt pass. Fresh library replay and
the comprehensive strict gate remain with the already live `light-cljs-125`;
no stored receipt was relabeled or weakened to pass. Repository-wide
`cargo fmt --check` reports extensive pre-existing formatting differences;
only the edited Rust tests are formatted, and unrelated formatting is retained.

The compiler resident smoke completes **423 checked samples** at nine samples
and one iteration, retained in `/tmp/light-cljs-154-compiler-bench.csv`.
The first full performance attempt at `/tmp/light-cljs-154-compiler-performance`
correctly rejects a changed fixture during measurement; it is not a valid
report. The frozen-source rerun uses the normal runner below. No before/after
performance claim is made.

```sh
python3 benchmarks/run_compiler.py --output /tmp/light-cljs-154-compiler-performance-final --samples 9 --iterations 1
```

The frozen normal performance rerun succeeds at
`/tmp/light-cljs-154-compiler-performance-final`: **180 fresh-process rows**,
**423 resident rows**, **423 allocation rows**, and checked resident operation
outputs, with exact source archive, binaries, emitted programs and hashes.
The valid frozen report is separate from the discarded changed-source attempt.
Final Python discovery runs **117 tests: 111 pass, six errors** from the
intentionally stale library compiler-source receipts above, zero assertion
failures. The refreshed inventory check and source-context catalog check pass.
The final discovery log is `/tmp/light-cljs-154-ledger-tests-current.log`.

### Top-level lexical declaration source restoration — 2026-10-11

`light-cljs-155` restores the complete original `cljs.top-level` context to
its executing TSV and source-context catalogs. The two original top-level
`let` forms remain unchanged: `bar` closes over `foo = 1`, and `baz` closes
over a separate `foo = 2`. Both original assertions and their source-span
provenance are retained. The adapter now registers eight supported contexts,
twelve complete bodies, and twenty-five original assertion sites. No pending
source-context exclusion remains; the comprehensive language gate under
`light-cljs-125` is separate and remains unfinished.

The complete original ordinary contract passes both freshly checked pinned
references: release **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, with
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
Both return `true`, with empty trace and stdout. The source programs themselves
are unchanged; regeneration updates registration and test/fixture fingerprints.

```sh
python3 -B tests/reference/compiler-residual-source-context.py --write --catalogs
python3 -B tests/reference/compiler-residual-source-context.py --catalogs
python3 -B tests/reference/compiler-residual-source-context.py --run --case top-level --output /tmp/light-cljs-155-top-level-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --audit-local --case top-level --output /tmp/light-cljs-155-top-level-local-current
cargo test --locked --test compiler_residual_source_context
```

The ordinary both-pin receipt is
`/tmp/light-cljs-155-top-level-reference-current/evidence.json`, SHA-256
`42bda852e44d0f4e9427625167a6d9f3ac6035fd839eb4e9666cd6cdf28b39cd`.
The local audit now observes supported contexts as well as unresolved ones,
and `--case` selects the same exact source for local and reference execution.
This prevents a restored source from disappearing silently from the local
observation run. Catalog validation passes after regeneration.

The restored complete context also passes the local observation audit in
ordinary and macro phases: both compilations succeed, Node prints `true`, and
QuickJS returns the printed string `"true"`. Its receipt is
`/tmp/light-cljs-155-top-level-local-current/evidence.json`, SHA-256
`661b346dc749150a052ab5ef6cfae7741013ebcc37751a9dd3c967d0b1543f0f`.
The full `compiler_residual_source_context` Rust test passes all eight contexts
in ordinary and actual macro execution, using both fresh and reused MacroHosts
and both Node and QuickJS application engines. The log is
`/tmp/light-cljs-155-source-context-rust.log`.

The implementation catalogs the complete analyzed owning expression for nested
`def`/`defn` products, executes that owner lazily once, and shares its lexical
captures. It retains analyzed IR so macro calls are not expanded again. Earlier
products and previous source generations remain readable during installation;
missing conditional products fall back to earlier definitions, and `defonce`
checks actual conditional installation without forcing unrelated plain roots.
Dynamic bindings and executed `set!` preserve their usual phase-local behavior.
Unfinished forward reads still diagnose a genuine initialization cycle.

Final local audit after the generation/binding corrections:
`/tmp/light-cljs-155-top-level-local-final/evidence.json`, SHA-256
`bfcc4f19e248fe41b0b2513d51f5871c7d7e4c6f60c7c16490d7d227bd321e19`.
The focused final Rust run passes 154 tests (112 macro, 10 namespace completion,
one eight-context source test, 21 session, and 10 evaluator), with one explicitly
ignored evaluator reference test. The existing compiler-bootstrap cancellation
regression now additionally cancels inside an owning group's initializer,
verifies terminal cancellation despite a language catch, and reuses the host
successfully. That focused unit test passes. Logs:
`/tmp/light-cljs-155-final-regressions.log` and
`/tmp/light-cljs-155-final-demand.log`. The direct canonical runtime Node test
also passes (`/tmp/light-cljs-155-runtime-tests.log`).

A separate timer-driven stress probe observed one native QuickJS crash during
bootstrap form import, before owner execution. Ten consecutive default-stack
reruns and two further individual default-stack reruns succeeded; this is not
proof of a repaired race. `light-cljs-156` retains the exact observed native
stack, reproduction lead and required repair/validation as a distinct engine
issue. The deterministic initializer cancellation test above does not mask or
claim to resolve that asynchronous import failure.

Normal checked performance completes successfully:

```sh
python3 benchmarks/run_compiler.py --output /tmp/light-cljs-155-compiler-performance --samples 9 --iterations 1
cargo bench --locked --bench evaluator -- --iterations 20
cargo bench --locked --bench compiler -- --application-only --samples 9 --iterations 1 --dump /tmp/light-cljs-155-application-performance
```

The normal report retains 180 fresh-process, 423 resident, and 423 allocation
rows with checked results, exact source archive, binaries and emitted programs.
Manifest SHA-256:
`2547acbc8e82184a5d29d64cd84a3467470c2e576fd92b46f84eba45b5c48a87`.
The evaluator run validates all three scenarios at 20 iterations; the additional
application run validates 270 samples. Their logs are
`/tmp/light-cljs-155-performance.tsv` and
`/tmp/light-cljs-155-application-performance.log`. No before/after speed claim is
made. The static inventory's analyzer fingerprint is regenerated and checked;
this does not grant execution coverage. The full zero-missing final gate remains
owned by `light-cljs-125`.

The final execution/namespace run adds 83 passing tests (53 execution, 28
namespace, and two namespace-shadowing), with 43 explicitly ignored external
reference cases. Combined with the focused run and cancellation unit test,
**238 Rust tests pass**, with **44 explicitly ignored cases**. See
`/tmp/light-cljs-155-final-execution.log` for the final run.

After refreshing the static analyzer fingerprint, Python discovery runs 117
tests: **111 pass, six errors, zero assertion failures**. All six errors reject
stale library source/driver/inspection receipts, the previously retained
`light-cljs-125` refresh obligation; validators are unchanged. The initial run
before fingerprint regeneration also reported local-surface drift and is not
credited as the final result. Final log:
`/tmp/light-cljs-155-ledger-tests-final.log`. Source-context catalog validation,
static inventory `--check`, and `git diff --check` pass.

## Namespace and runtime reader continuation, 2026-10-11

Starting from clean HEAD `95abbfd`, the source-context generator retains five
additional complete original bodies and **20 original assertion sites**. The
three namespace bodies `test-ns`, `test-cljs-1508`, and `test-cljs-3399` retain
fifteen sites, the reserved JavaScript keyword namespace `static.core-test`
retains its one original site, and the complete tagged runtime reader body
retains all four original inputs and assertions. Generation verifies each
complete source span and setup against the original pinned checkout. Public
API/arity eligibility is not inferred from these source witnesses.

The namespace source keeps its original `+` definition, core excludes and
`mapv` rename, set alias/refer/rename, referred `baz`, `:use` of `quux`, and
`fake.ns` `:as-alias` context. The local source macro namespace uses `cljs.core`
for the original `clojure.core` JVM macro aliases; the reference header retains
`clojure.core` `:require-macros`. The complete already-proven `ns-test.foo`
support module retains `baz`, `kw`, and quoted `qkw`; `ns-test.bar` remains
byte-identical. Only unused `goog`/`cljs.analyzer` imports and the adapted
`cljs.test` harness imports are omitted. The separate `goog/isArrayLike` body
is not selected or covered.

`test-cljs-3399` has a second original `is` inside the outer assertion's
message. The adapter evaluates the outer condition, then its complete message,
then performs the outer assertion. Both original sites therefore execute on
the successful path. Using an ordinary lazy assertion message would skip that
second site; this adapter does not do so. Alias spellings remain unchanged
through these bodies, including `s/union` and `lang/for`.

The tagged runtime body uses the three exact handlers from the unchanged full
`data_readers.cljc` map: `cljs/tag` maps to identity, `cljs/inc` to inc, and
`cljs/union` to set union. Its provenance retains that entire original map and
hash. The explicit embedding installs these handlers through the public reader
registry independently in each execution phase, then executes every original
`read-string` input and assertion. The separate compiled source-tag body and
unused custom test handlers receive no witness from this runtime body.

The focused Rust test passed all eleven context fixtures in **14.83 s**, zero
failures or ignored tests. Each executes in Node and QuickJS applications and
inside actual QuickJS macros, with fresh and reused MacroHosts. Log:
`/tmp/light-cljs-125-remaining-reader-namespace-local.log`. All three new context
fixtures independently pass both release **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**, with checked `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false`:

- `/tmp/light-cljs-125-remaining-ns-test-reference-final/evidence.json`, SHA-256
  `2874cf949d4e1912ba1144eceeaa5b7c0f40a1adda73bd8c001925b177003eeb`.
- `/tmp/light-cljs-125-static-namespace-reference/evidence.json`, SHA-256
  `27bf39de6e5113bc638c96d25c9ab5726b1cfb96a283975344e36653370c3e26`.
- `/tmp/light-cljs-125-tagged-runtime-reference/evidence.json`, SHA-256
  `49c43269d9052da8aed98c6502082e56e992ee57db97b9662a0c80e1344f3311`.

The supported proposal now contains eleven context cases, seventeen complete
bodies and forty-five original sites, including the earlier proved bodies.
No shared catalog writes were performed by this source adapter continuation;
registration remains coordinated by the main owner. Reproduce the new proofs:

```sh
python3 -B tests/reference/compiler-residual-source-context.py
python3 -B tests/reference/compiler-residual-source-context.py --run --case ns-test --output /tmp/light-cljs-125-ns-test-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case static --output /tmp/light-cljs-125-static-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case tagged-runtime --output /tmp/light-cljs-125-tagged-runtime-reference-current
cargo test --locked --test compiler_residual_source_context -- --nocapture
```

### Exact renamed-core existence gap

The complete original `test-cljs-2292` remains an independent uncovered source
probe. Its header keeps the original core `mapv` to `core-mapv` rename, all
relevant original namespace imports, and complete original setup. The actual
body checks that `(exists? mapv)` is false while `(exists? core-mapv)` is true.
Both reference pins and both ordinary local engines pass both assertions.
Actual QuickJS macro compilation fails while forcing the unavailable
`cljs.ns-test/mapv` definition. This source is absent from the passing TSV and
catalog proposals; both original sites remain in `unwitnessed-source-cases`.
It is not renamed, guarded or given a boundary exclusion.

Original module SHA-256:
`21243db745474306fdfb034dd99b4ba5cee1a858493e4e8182fc2542968d1f1b`.
Original complete body SHA-256:
`419c0dbcddcffa2538e2b17d086a14040f6fba4e97d1e7df910966b6e189dcc4`.
Complete adapted ordinary program SHA-256:
`62f941cd8dcba802c664c068ee0d6c1901bb98f62485edd533e4f4e9569b02d3`.
The exact sources, emitted JavaScript, Node/QuickJS observations and located
macro error are retained under `/tmp/light-cljs-125-ns-exists-source-local`.
Its receipt SHA-256 is
`0f66142dd08f122ae60897687dab6b06a7a58639676ff7423efd73d993e165b5`.
The independent both-pin receipt is
`/tmp/light-cljs-125-ns-exists-source-reference/evidence.json`, SHA-256
`d01e5277083afae0fad1872b5180e41ee5e09d72cf0a5d0fb564419ae3b78c9e`.
The main owner received these exact sources and receipts for focused ownership.

No production implementation, queue or Git state was changed. The existing
cancellation/limits work was not investigated or executed. The full language
gate remains incomplete; this continuation does not rerun or claim that gate.

### Repaired existence source and remaining complete readers — 2026-10-11

At production HEAD `a8e8a43`, light-cljs-158 has already promoted the exact
`ns-exists` context above. Fresh release **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** execute both original assertions
with the checked `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false` settings. Receipt:
`/tmp/light-cljs-125-fixed-ns-exists-reference/evidence.json`, SHA-256
`f4aaeb9a99f7ec753893e8fcd4e2011a5b0c5ad1bf3c6d1f333c70a3ff13b5ea`.
`complete_residual_namespace_bodies_execute_in_both_phases` passed all twelve
contexts in **17.20 s**, zero failures or ignored tests, including Node and
QuickJS applications and actual macro execution with fresh and reused
MacroHosts. Log: `/tmp/light-cljs-125-fixed-ns-exists-local.log`.

The passing TSV and Rust test remain unchanged by this continuation:
**12 contexts, 18 complete bodies, 47 original sites**. TSV SHA-256 remains
`3f3fbb3ffadf6c02cf6334da6a353592474909fc46b00300367764cc4a2b275e`.
Two new complete contexts retain **6 assertion-bearing bodies / 8 sites** in
`unwitnessed-source-cases`. They receive no passing case, assertion witness or
phase mapping. The generator verifies each original byte span and preserves
all setup, literal expectations and messages.

The `edn-stream` context retains the complete original `test-reader` helper
and `test-read` body from `src/test/cljs/clojure/edn_test.cljs`, including all
three `edn/read` versus `reader/read` assertions. Locally the `reader-types`
alias selects the existing first-party `cljs.reader/string-push-back-reader`
constructor; references retain `cljs.tools.reader.reader-types`. This is an
explicit stream-constructor namespace adaptation, without a fixture stream
implementation, replacement `read-string` call or general reader-types claim.
Both pins pass the full body. Node and QuickJS applications reach the third
assertion and fail with `Invalid arity: 4`; actual macro compilation fails
with the same error. The local `clojure.edn/read` facade provides only zero,
one and two arguments. This is an intended public EDN stream contract requiring
focused implementation; neither the original assertion nor its expectation
is weakened.

- Original module SHA-256:
  `7034a216f6aba2dd277f876899b2c235ebb54812d405af34087885eca4959abe`.
- Original `test-read` body SHA-256:
  `7d7a5a4c39247074c088d8a6859265dde0a271c0b5b496c7815e72d9399f0763`.
- Original helper SHA-256:
  `e89a676d30b2aec6c74fbe7c6485f8e767261c1129dcf23e746229a3f0f7e9ec`.
- Complete adapted program SHA-256:
  `9a9c0baea6512d9de2dcf44aeba59f2d3874a3a0f0f7787e83eddd7776fe4f2b`.
- Both-pin receipt `/tmp/light-cljs-125-edn-stream-reference/evidence.json`,
  SHA-256 `8bf84f3ab26e0d66b0de857addbe04f9b4bef46b8328d0e5f9fbc6a9d26d1131`.
- Local receipt `/tmp/light-cljs-125-edn-stream-local/evidence.json`, SHA-256
  `abce3981a19c443b78d45abe6cbbcd307c58ff2e1072ba3077833f4c61afee5b`.

The `record-literals` context retains the entire `Foo` setup and all five
top-level assertions from `src/test/cljs/data_readers_test/records.cljc`.
Ordinary, quoted and doubly quoted record tags, symbol-valued fields inside
lexical shadowing, and both original assertion messages remain. Both pins pass
using the original self `:require-macros` clause and a byte-identical complete
CLJC companion. The local header omits that JVM companion clause under the
existing companion boundary; ordinary and macro source reading both reject
the first `#data_readers_test.records.Foo` tag with `UnknownTag`. No synthetic
data reader, map substitute, constructor expansion or runtime pass is supplied.
This remains a source-contract reconciliation candidate: independently decide
the ordinary record-reader obligation and the excluded JVM companion facility
before treating the failure as a demonstrated feature omission or exclusion.

- Original module SHA-256:
  `b642ad4397ab1723da07245ac6e36cc24a22af44fbddf823ee149a8e7c3a87f2`.
- Complete adapted program SHA-256:
  `d5674c46d5a5935d4be35d238ae047e41786d12529860186670c116bd8a4a7ee`.
- Both-pin receipt `/tmp/light-cljs-125-record-literals-reference/evidence.json`,
  SHA-256 `24d3f017dac7ecdb47867c4510458c9bb326931792098d683c779e54fef73035`.
- Local receipt `/tmp/light-cljs-125-record-literals-local/evidence.json`,
  SHA-256 `a8c2397f0c24720ab22f8f936208edc4a9eb7988580a0fdc18ac14960a19aedb`.

Reproduce the retained candidates without registering passing witnesses:

```sh
python3 -B tests/reference/compiler-residual-source-context.py
python3 -B tests/reference/compiler-residual-source-context.py --run --case edn-stream --output /tmp/light-cljs-125-edn-stream-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --audit-local --case edn-stream --output /tmp/light-cljs-125-edn-stream-local-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case record-literals --output /tmp/light-cljs-125-record-literals-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --audit-local --case record-literals --output /tmp/light-cljs-125-record-literals-local-current
```

Only the source-context generator, proposals, retained source fixtures and this
documentation changed. Shared catalogs remain owned by the main worker. No
production, queue, staging or commit changes were made; no full gate,
cancellation or limits suite was run, and light-cljs-156 was not investigated.

## EDN stream closure after the retained omission (2026-10-11)

The earlier four-argument failure above is historical. `clojure.edn/read` now
exports the canonical one/two/four stream arities through the strict first-party
runtime, retaining the supported explicit zero-argument embedding. The original
`test-reader` helper, complete `test-read` body, all three assertions, their
expectations and the existing constructor alias adaptation remain unchanged.
The passing context TSV now contains 13 contexts, 19 complete bodies and 50
original assertion sites. Only the separate record-literal candidate remains
unwitnessed; this focused completion makes no broad zero-missing claim.

The complete context passes both identity/settings-checked pins and literal
Node/QuickJS application observations. The Rust source-context suite executes
all 13 contexts in applications and actual macro computation, with fresh and
reused MacroHosts. `tests/compiler_edn_stream.rs` additionally runs literal
cursor/EOF/options/provider observations in all four combinations and both
engines, then checks located macro failures, unsupported expansion-result
values and recovery in the same host. Runtime stream errors and the macro
expansion-result boundary remain separate diagnostics.

`tests/reference/compiler-edn-stream-contract.json` binds the canonical public
read declaration and its 1/2/4 arities, current 0/1/2/4 source facade, current
runtime, unchanged complete helper/body provenance, phase test and every
support fixture to exact hashes. The public API witness now points to this
current strict stream contract and the complete original reference case.
Hashes and registrations alone grant no execution credit.

Reproduce the original body and local source observations:

```sh
cargo build --locked --bin light-cljs
python3 -B tests/reference/compiler-residual-source-context.py --catalogs --case edn-stream --run --audit-local --output /tmp/light-cljs-159-edn-stream-current
cargo test --locked --test compiler_edn --test compiler_edn_stream --test compiler_reader --test compiler_residual_source_context
python3 -B tests/reference/compiler-edn-stream.py --output /tmp/light-cljs-159-edn-eof-reference
```

The separate parity fixture checks one-argument nil EOF, mandatory two-argument
EOF errors, four-argument error-flag truthiness including 0/empty string, exact
nil/false/string/keyword EOF values overriding `:eof`, and ordered local reader
callbacks. Both pins pass. Strict unsupported syntax, host capabilities and
ambient-tag exclusions stay in the independently executed local fixture.

Checked performance samples are retained in
`benchmarks/results/edn-stream-2026-10-11/output-startup.json`: nine fresh Node
processes verify the complete integrated value/effects/stdout on every sample,
plus primitive/core-call values and runtime multiplicity. The emitted integrated
program is 346,575 bytes (82,502 gzip); median parse/initialize/execute/total are
4.602/3.388/8.091/16.165 ms. This is a current checked sample, not a comparative
speedup claim or an EDN throughput benchmark.

All 191 complete SCI library cases were observed in Node and QuickJS against
finalized compiler/library hashes. Earlier batches whose inputs changed were
rerun. The new active receipts retain the exact source/identity/settings-checked
release/checkout observations and every prior receipt. Classification remains
163 original agreements, 6 verified pin differences and 22 unresolved original
contracts; no mismatch is promoted or expectation changed. In particular, the
separate string/set omissions retain their live owners. Evidence:
`/tmp/light-cljs-159-library-current/evidence.json`; current source and evidence
hashes are retained in `tests/reference/compiler-edn-stream-verification.json`.
The six focused arity/phase accounting regressions and all fourteen library
accounting regressions pass. Focused Clippy succeeds;
strict `-D warnings` reports 53 existing diagnostics in ten unchanged source
files and is not represented as a clean lint run.

## Complete namespace evidence continuation at 9964c3c (2026-10-11)

The source-context suite now executes **17 contexts, 31 complete original
bodies and 118 original assertion sites** through Node and QuickJS applications
and actual macro computations, each with fresh and reused MacroHosts. Its exact
Rust test passes with zero ignored checks. This is focused source/phase evidence;
full zero-missing language acceptance remains incomplete.

Three new ordinary namespace contexts preserve **11 complete bodies / 67
original sites** that previously had historical flattened assertion witnesses:

| Context | Complete original evidence | Program SHA-256 |
| --- | --- | --- |
| `apply` | Six active bodies / 53 sites, original `fn-returning-this` and `meta-f` setup, both full six-function `doseq` lists, 30-argument paths, callable-data arity errors and eager original assertion-message computations | `c57862a28018871e133538c6319a8d3737878c8ff0662a585cd1ca3aadb9ddf7` |
| `binding` | Four contiguous bodies / six sites, every original local setup form and byte-identical `binding-test-other-ns` dependency, cross-namespace binding/redefs, restoration and parallel initializers | `6b0df864fd46e8c361e7d8cf898966a137cceeeab2ea4b6423bec79a18da48ad` |
| `letfn` | One complete body / eight sites, lexical mutually recursive `ev?`/`od?` definitions and every original call | `fd59d96c3cc22b0f9de3afd31012c7096d8939919c2d6e996e9d1b788962b4bd` |

Both identity/settings-checked pins pass all three complete contexts. Direct
local source probes also return true in both engines/application and actual
macro phases. These add missing complete-body source/phase contexts without
duplicating their existing assertion witnesses or granting new assertion counts.
The independent binding type-hint inference body and the two upstream discarded
apply bodies are not selected or counted. Their original source remains intact.

The closed EDN stream repair is independently reverified here. The complete
original helper and all three original `test-read` assertions remain unchanged;
program SHA-256 is
`9a9c0baea6512d9de2dcf44aeba59f2d3874a3a0f0f7787e83eddd7776fe4f2b`.
Both pins pass, direct Node/QuickJS source observations return true in both
phases, and the suite executes it in fresh/reused hosts. The existing stream
constructor namespace adaptation remains explicit. No production repair or
weakened original assertion was needed.

The new `tagged-source` context supplies supplemental proof for the genuinely
unregistered complete `test-identity-custom-literal` body and its original
`#cljs/tag [1 2 3]` assertion. Program SHA-256 is
`c930ca1c14e49238919acb92cbac85c0eae2c8f3674df99d6d277b48e95a777e`;
original body SHA-256 is
`e44f87cf162a70c7deba9146fac8bf6db796ee6be1770dc54f6d955ea495fed3`.
The Rust embedding installs the exact original `cljs/tag=clojure.core/identity`
operation through `register_compiler_data_reader` before reading the unchanged
source in each fresh/reused host. All application/macro and Node/QuickJS paths
pass. Both pins pass with the exact original tag entry selected in the reference
`data_readers.cljc`; the entire original configuration remains in provenance.
The initial reference attempt that supplied every configuration entry failed
with `Var$Unbound` because unused custom handlers require their own JVM companion
namespaces. That failed receipt is retained separately and is not a pass.

The ordinary CLI has no compiler-data-reader installation argument. The source
report now uses the explicit configured-reader adapter described below. The
reviewed `embedding-source-cases` row remains durable provenance; its exact
case and sole assertion witness are registered in the full catalogs. Ordinary
CLI `UnknownTag` remains an independent capability boundary.

The record-literal reconciliation now includes an independent companion
control. Both checked pins pass the unchanged `Foo` setup and all five original
top-level assertions with the original self `:require-macros` clause and complete
byte-identical CLJC companion. Both checked pins reject the same complete source
with the local header and no JVM companion, with `ClassNotFoundException` for
`data_readers_test.records.Foo`, before any assertion executes. Local ordinary
and actual-macro source reading still reject the first original tag with located
`UnknownTag`. No constructor expansion, map replacement or invented data reader
is supplied. The exact upstream literal oracle depends on the JVM record class
provided by the explicitly excluded companion facility; this is scope evidence,
not a blanket exclusion of record values, constructors or ordinary tagged-reader
capabilities. All five original source sites remain intended/unwitnessed pending
the adopted JVM-free constructor-reader contract, with no passing witness or
source-site retirement. The complete review and receipt hashes are durable in
the retained proposal's `source_contract_review`.

Fresh receipts:

- `/tmp/light-cljs-125-source-next-reference/evidence.json`, SHA-256
  `1febb5f675bf4222000af4130507da8f271dc76b836d5910b02d9cf9787c5bcc`:
  both pins pass EDN, apply, binding, letfn and the original record companion
  context; the no-companion record control fails independently in both pins.
- `/tmp/light-cljs-125-record-companion-control-current/evidence.json`, SHA-256
  `b6ed1388e0b30caf0bbf31201e4b7c405cac2983e10dc030ff98d97d1b5a7637`: fresh reproduction of the checked
  original-companion pass and no-companion failure in both pins.
- `/tmp/light-cljs-125-source-next-local/evidence.json`, SHA-256
  `15224ccb15e53dfae48c664ee4ba071a06c22dcc3af9679a5db97fd6cdea154c`:
  direct both-engine/phase observations; record sources retain located rejection.
- `/tmp/light-cljs-125-tagged-source-next-reference-corrected/evidence.json`,
  SHA-256 `f963b97774374d1eb523479ddf4c77c844f9eb12f42a0d002310f6469b82611f`:
  both pins pass the complete source-tag assertion with the exact original entry.
- `/tmp/light-cljs-125-tagged-source-next-reference/evidence.json` retains the
  failed unbound-unused-handler attempt; it receives no passing credit.
- `/tmp/light-cljs-125-source-next-rust17.log`, SHA-256
  `432e997009bae8facbcfaa885042188ee2e34ed459eb4c994239d457bbf1ea06`:
  one exact source-context test passes all 17 contexts, zero ignored.

Reproduction:

```sh
python3 -B tests/reference/compiler-residual-source-context.py
python3 -B tests/reference/compiler-residual-source-context.py --run --case apply --output /tmp/light-cljs-125-apply-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case binding --output /tmp/light-cljs-125-binding-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case letfn --output /tmp/light-cljs-125-letfn-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --run --case tagged-source --output /tmp/light-cljs-125-tagged-source-reference-current
python3 -B tests/reference/compiler-residual-source-context.py --audit-record-companion --output /tmp/light-cljs-125-record-companion-control-current
cargo test --locked --test compiler_residual_source_context complete_residual_namespace_bodies_execute_in_both_phases -- --exact --nocapture
```

The generator's byte-span/source-tree checks, scoped Rust formatting and owned
file diff checks pass. Shared full case/witness/phase catalogs remain owned by
the main integration worker. No queue, staging, commit, production, cancellation
or limits actions occurred, and the preserved light-cljs-156 native-crash work
was neither investigated nor changed.

### Public declaration anchors for complete contexts

The three new ordinary contexts are independently anchored in the operation
ledger, so their complete sources remain executable report fixtures while the
67 historical assertion witnesses stay byte-identical. The generator proposes
three exact public operation rows and their matching phase rows:

| Complete context | Reviewed obligation | Exact executed source call/signature |
| --- | --- | --- |
| `apply` | `api::src/main/cljs/cljs/core.cljs::apply::arity-3` | Fixed `[f x y args]`, four arguments: `(apply f 0 1 (range 2 10))`, executed for every original function in each complete six-candidate `doseq` |
| `binding` | `api::src/main/clojure/cljs/core.cljc::binding::arity-1` | Actual `defmacro` `[bindings & body]`, exercised by the original cross-namespace binding assertion and parallel-initializer body |
| `letfn` | `api::src/main/clojure/cljs/core.cljc::letfn~2::arity-1` | Actual `defmacro` `[fnspecs & body]`, exercised by the complete mutually recursive `ev?`/`od?` vector and all eight original assertion forms |

`apply::arity-3` is the third declaration branch, not a three-argument call.
The distinct imported `letfn` declaration has an unresolved signature and is
not mapped. Each mapping retains the exact pinned declaration body/signature,
original executing test-body/call provenance, current whole-source Rust test,
fixture/support hashes, full reference-source hash and literal true/empty
trace/stdout contract hash. The fixture assertion witnesses are not replaced by
operation assertions; the operation rows use their independently verified named
application/macro test and both-pin full-source oracle.

All three selected operation/phase IDs previously had no mapping. If a future
reconciliation replaces another operation or phase receipt, the generator
preserves that exact row/hash in `superseded-context-operation-mappings` before
replacing it. Historical source-assertion witnesses remain outside this ownership.

The generated operation/phase rows and exact review are durable in
`compiler-residual-source-context-proposals.json`. The main integration worker
installs them through the existing coordinated catalog workflow:

```sh
python3 -B tests/reference/compiler-residual-source-context.py --write --catalogs
python3 -B tests/reference/compiler-residual-source-context.py --catalogs
```

These commands are catalog integration/freshness checks, not execution passes.
The complete local and both-pin source receipts above establish the corresponding
executions. The configured source-tag embedding and unresolved record literal
contexts remain outside these generic operation anchors.

Read-only candidate overlays verify the integration before any shared writes:
`/tmp/light-cljs-125-source-anchor-contract-validation.json` records a passing
`report.load_contract('intended-language')` with 4,728 assertion rows, 3,763
witnesses and 1,026 fixtures. `/tmp/light-cljs-125-source-anchor-validation.json`
records passing public-operation and phase loaders for all three proposed IDs,
including current test/support hashes and exact source/literal oracle matching.
These are structural integrity checks; the semantic executions remain the
full-source receipts above.


## Twelve original assertions through four audited source embeddings

`compiler-residual-source-context.py --write --catalogs` now registers the
configured `cljs/tag` assertion, all six original import assertions, all three
original `goog/isArrayLike` assertions and both original `Error->map`
assertions. Historical witnesses remain byte-identical. These registrations add
exactly twelve original source assertions; they add no public API/body/program
phase mappings. All five record literal sites and fourteen self-host eval sites
remain intended and pending.

`hosted_source_dispatch.py` validates the exact profile before the report can
execute any of these four cases. The report routes them through the standalone
Rust audits and retains eight raw observations per profile: fresh/reused
application/actual `MacroHost` execution in Node and QuickJS. Every frame must
match literal true, empty trace and empty stdout. Compiler/producer/source,
build, execution binary, original body/assertion, reader configuration and real
dependency hashes are checked in the current invocation. The report never reads
a stored receipt to grant source credit. Missing profiles, source/dependency or
expected-value drift, partial/reordered witnesses and missing raw frames fail
closed; a known profile cannot fall through to generic CLI execution.

The configured tag profile retains the exact `#cljs/tag` source and original
reader configuration. Its compiler embedding installs identity before reading.
The generic CLI still rejects that tag without the configured capability.

The import profile uses the thirteen genuine transitive Closure dependency
files independently obtained from both checked builds and matched to the Maven
source archive. Long, Integer, Vec2 and Vec3 retain their genuine constructors,
methods and original six assertions. No fake constructor or reduced input is
supplied, and Closure build-system parity is not claimed.

The goog profile preserves the full original `test-cljs-1677` let and three
call spellings. Its compiled provider delegates to real pinned
`js/goog.isArrayLike`; real Closure base initialization executes before the
application and inside the actual macro. The earlier bare foreign-function root
macro rejection and failed/corrected loader controls remain separate evidence.
No default goog namespace capability is inferred.

The REPL profile retains both complete original TypeError/ExceptionInfo bodies,
original expected type symbols and exact original `Error->map` definition and
docstring. That function delegates to real core `Throwable->map`. Both checked
references execute canonical upstream `cljs.repl`; the local dependency slice
establishes this function only, without claiming the entire REPL namespace or
its unused spec/documentation/macro dependencies.

Standalone review receipts remain immutable at
`/tmp/light-cljs-125-configured-source-adapter-final/evidence.json`,
`/tmp/light-cljs-125-hosted-import-adapter-v1/evidence.json` and
`/tmp/light-cljs-125-hosted-namespace-adapter-v1/evidence.json`. The exact source
and dependency integration review is retained at
`/tmp/light-cljs-125-hosted-namespace-adapter-integration-review.json`.

Focused guards:

```sh
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_configured_source_context.py -v
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_hosted_import_source_context.py -v
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_hosted_namespace_source_context.py -v
python3 -B -m unittest discover -s tests/reference/compiler-language -p test_hosted_source_dispatch.py -v
```

Fresh standalone artifacts can be reproduced with the three corresponding
helper scripts and a new `--output` directory. The ordinary report runs these
adapters when their original assertion IDs are selected, retaining the full
current raw evidence under `hosted-source` in its fresh artifact directory.


Current ordinary report integration verification:
`/tmp/light-cljs-125-hosted-report-integrated-v1/report.json`, SHA-256
`53375c010c99b02d0acb461d338ce364b293f2bcccb99211c75bf506d26d691c`,
was produced by selecting all twelve original hosted assertion IDs with
`--run --reference both --timeout 180 --reference-timeout 300` and a fresh
artifact directory. The invocation exited zero and marks exactly those twelve
original assertions covered. Each of the four profiles retains eight current
raw observations and both checked reference results. The required API selection
also executes its ordinary witness cases and boundary checks. Its overall
900 covered / 3,909 missing / 466 unrelated count is a focused report including
all independent API obligations, not a full completion claim.

The separate generative audit remains unregistered. Both complete original
parse bodies pass each canonical pin and independent real-provider Node
controls. Its first actual QuickJS application frame returns true with exactly
100,000 callbacks for each body; the subsequent macro frame rejects the draft
outer binding form before execution. The eight required frames are incomplete,
so no source credit is granted. Exact dependency bundles, failures, the unused
corrected wrapper, raw observation and diagnostic patch are preserved under
`/tmp/light-cljs-125-generative-audit-v1`; `handoff-final.json` records normal
process completion with exit 101. The unreferenced
`tests/compiler_hosted_generative_source_context.rs` is diagnostic work and
must remain outside the verified hosted-source integration commit.
