# Exact original closure phase boundary

The unchanged portable original `sci-record-method-closure` has SHA-256
`dd9285ec7467d76b36e25575876a2a404469faa9b81a6b9f608b1890668c27f8`:

```clojure
(defprotocol P (p [x])) (let [kept [40]] (defrecord R [x] P (p [this] (fn [] [x (first kept) (= this this)])))) ((p (->R 2)))
```

The original SCI result `[2 40 true]` remains immutable and unresolved. Both
required pins and current application observe the typed vector `[2 nil true]`,
with empty trace/stdout. The retained emitted pin method reads `user.kept` while
the enclosing original let installs a lexical `kept_...`; an absent namespace
property becomes undefined and `first` returns nil. Neither pin result nor a
compiler warning grants agreement with the original SCI expectation.

Actual MacroHost rejects `unavailable definition: user/->R` when the original
setup remains top level and its last expression is demanded by a macro. Executing
the entire unchanged contiguous original inside the macro removes that setup
ownership ambiguity but still rejects `unavailable definition: user/kept`.
Fresh and reused controls retain both located diagnostics. They supply no fake
root, rename, removed let, shortened body or original semantic credit.

This is a concrete application/phase difference, but a distinct required phase
semantic defect is not established. `docs/evaluator-migration.md` explicitly
retains definition-site resolution, source visibility and unavailable-definition
errors, with global storage differing by phase. Pellet 158 is a closed, narrow
`exists?` exception: it allows safe nonforcing existence observations and
explicitly preserves normal unresolved-Var diagnostics. This body calls `first`
on an ordinary root read, not `exists?`. The group asks for executing local tests
for deliberate macro/source/phase differences, separately from public pinned
semantics. No blanket record exclusion or source disposition follows from this
boundary; the original remains pending independent reconciliation.

Distinct finite source controls strengthen the attribution. With an explicitly
declared namespace root `(def kept [99])`, the complete original body inside
actual MacroHost returns `[2 99 true]`; both pins' corresponding application
setup also returns this vector. A normal `(let [kept [40]] (defn saved [] (first
kept)))` owner returns 40 in actual MacroHost and both pinned applications.
Both controls execute fresh/reused MacroHosts and Node/QuickJS application
observations; reused controls succeed after the retained original failures.
A plain `(declare absent)` macro read still fails with its located diagnostic.
These distinct controls never substitute for the original source or expectation.

Raw provenance and failed controls are retained:

- `/tmp/light-cljs-125-record-method-dialect-pending-closure-archive/handoff.json`
- `/tmp/light-cljs-125-record-method-complete-original-macro-control-v1.log`
- `/tmp/light-cljs-125-record-method-closure-controls-v1.log`: the first diagnostic
  driver expected an untyped JS array instead of the actual typed vector packet;
  the failure was in its observation assertion, with the real packet retained.
- `/tmp/light-cljs-125-record-method-closure-controls-v2.log` and the matching
  artifact directory: six located rejections and eight positive raw controls.
- `/tmp/light-cljs-125-record-method-closure-pin-controls-v1/evidence.json`:
  four independently fresh pin/setup controls with complete emitted artifacts.
- `/tmp/light-cljs-125-record-method-closure-boundary-review.json`: exact source,
  current compiler/driver/binary hashes, controls, group/158 basis and pending
  reconciliation proposal.

If an owner explicitly requires generated undeclared namespace properties to
have canonical undefined reads during actual macro execution, focused acceptance
would retain the complete original application and macro sources and verify
`[2 nil true]` across fresh/reused phases and both application engines. It must
keep the original SCI `[2 40 true]` unresolved, retain ordinary missing-root and
future-definition located diagnostics, and distinguish generated method reads
from source-level unresolved Vars. Declaration owners, captures, ordered effects,
root cycles and reuse must keep their existing behavior. No broad missing-root
fallback or fake namespace root would satisfy that narrower requirement. This
conditional proposal is not a confirmed gap, implementation or queue action.

```sh
LIGHT_SCI_CLOSURE_CONTROL_ROOT=/tmp/light-cljs-closure-controls-fresh cargo test --locked --test compiler_sci_record_method_closure_controls original_closure_and_ownership_controls -- --ignored --exact --nocapture
```
