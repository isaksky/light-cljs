# Original eval source diagnostic

This unregistered diagnostic executes the complete original `cljs.eval-test/test-eval`
body and preserves all 14 expressions, setup declarations and expected literals.
It supplies no shared-ledger credit and changes no compiler, runtime, source
catalog, table, stored receipt or original expectation.

The local adapter retains every declaration after the namespace and the entire
`deftest` body byte-for-byte. Its `deftest` macro creates a function; its `is`
macro wraps the original expression in a reporting thunk that catches ordinary
errors and continues. `EvalSession` installs the existing Rust compiler service.
The adapter never replaces eval with direct calls or a second evaluator.
Both a fresh session and one warmed with an ordinary root and source macro run
all 14 original assertion expressions in their original order.

The reference adapter compiles the complete unchanged original file with the
pin's genuine `cljs.test` macros. It loads the pin's `cljs.js`, which installs
`cljs.core/*eval*` as `eval-impl`. The upstream self-parity loader supplies ordinary
Node JavaScript evaluation and synchronous source-loading callbacks. A real
`cljs.js/eval` namespace form initializes the compiler namespace, then the host
binds its compiler state and `cljs.eval-test` namespace and runs only this test.
The genuine `cljs.js/eval-impl` uses `:def-emits-var true` and retains actual
Function/Var constants through its standard emitter methods. The bootstrap
compilation separately uses the checked ordinary reference settings
`:optimizations :none`, `:static-fns false`, `:def-emits-var false`.

At HEAD `8be52ad6d2e37e0c9e9a7a64a61244f14f7094d9`, both reference pins report
`Ran 1 tests containing 14 assertions. 0 failures, 0 errors.`

| Original assertion | Local fresh/reused result | Boundary observed |
| --- | --- | --- |
| 1, 2, 3, 5 | true | Finite form input reaches installed compiler eval |
| 4 | false | Original expectation requests metadata of the Var returned by eval of def |
| 6, 7, 8 | caught error | Live Function at `$[0]` is outside form transport |
| 9 | caught error | Live Function at `$[2][0]` is outside form transport |
| 10 | caught error | Original lucky-number definition did not happen because assertion 9 failed |
| 11, 13 | caught error | Live Function at `$[0]` is outside form transport |
| 12 | caught error | Bare Function at `$` is outside form transport |
| 14 | caught error | Live Var at `$[0]` is outside form transport |

The group boundary excludes upstream self-hosted compiler and unchanged
application compatibility. This original file explicitly requires an environment
that installs `cljs.core/*eval*`; its Function/Var constants and Var-returning
definitions exercise the genuine upstream self-hosted profile. The reference
result validates those original expectations in that profile. It does not by
itself establish that light-cljs promises this profile or justify a new
production gap.

[docs/evaluation.md](../../../docs/evaluation.md) describes `eval_form` as taking
compiler-owned Form data and the value/data bridge as finite compiler data;
arbitrary functions/atoms/objects remain ValueIDs. `compiler_data::Datum` has no
Function/Var runtime-root variant. The installed eval service calls `call.data`
before analyzing a Form. The same documentation explicitly says definitions
return their CLJS initializer value and retires native Var return conventions.
These are narrower than the upstream self-hosted Function/Var and
`:def-emits-var true` profile. Source evaluation still supports ordinary source
functions and finite quoted eval forms: assertions 1, 2, 3 and 5 pass through
the installed service. There is no broad service retirement or blanket
exclusion claim here. Classify any prospective original ledger disposition
against its exact group/scope contract; this diagnostic leaves every original
expectation and shared disposition unchanged.

Current artifacts are below `/tmp/light-cljs-125-original-eval-after166`:

- `local-final/evidence.json`: `d7abf0fa0c429f1e5c6a67067814041b1eb57784e9b27f9a2f99b3512f70174d`
- `release-initialized-original/evidence.json`: `ec539fbe5488afe72cbf9db56bb40cb596364ba81812e3968b61520e5d90305e`
- `checkout-initialized-original/evidence.json`: `579541d4bd7d3e9fef12dfac92075ac389d2b0c5e6c038225a681401413696b8`

After formatting the owned diagnostic driver with the crate's 2024 edition,
the exact installed-service audit was rerun at
`local-current-formatted/evidence.json`, SHA-256
`8add6dc0e93b186427768d826da3c6f9e2a8075a706c88beed8c3fb9e829459a`.
Both lifecycle contexts retain four passing original assertions, one false
result and nine caught failures. The earlier receipt remains intact. Neither
local receipt supplies shared-ledger semantic credit.

Earlier reference attempts retain their build/run logs. They respectively exposed
an incorrect working directory, source-loader dependency metadata absent from
the fresh compiler state, and the missing compiler namespace initialization.
Only the final initialized hosts above establish the original reference result.

Run each into a fresh artifact directory:

```sh
python3 -B tests/reference/compiler-language/original_eval_source_probe.py --output /tmp/original-eval-local
python3 -B tests/reference/compiler-language/original_eval_source_probe.py --reference release --output /tmp/original-eval-release
python3 -B tests/reference/compiler-language/original_eval_source_probe.py --reference checkout --output /tmp/original-eval-checkout
```

These commands build only the dedicated ignored local diagnostic and one
original upstream test per reference. They do not run cancellation/limit suites
or investigate native crashes.
