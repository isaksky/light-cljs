# Compiled loop and recurrence

The contract is ClojureScript commit
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, compared with Maven **1.12.134**
and the checkout runtime **0.0.135109660**. Reference runs verified the revision,
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false` before and
after compilation. Tools: Node **v24.18.0**, Clojure **1.12.4**, Java **25.0.2**.

## Implementation and source contract

Each function method and loop owns a stable recurrence target. Tail eligibility
is separate from value/discard/return emission context: a discarded loop can
recur, while a value-position let/IIFE inside an argument cannot recur outward.
Initializers evaluate sequentially. Recur evaluates all arguments left to right
before changing any state. Labeled iteration and fresh lexical constants preserve
swaps, nested targets, and closures retained from successive iterations. Rest
recurrence passes one sequence value without repacking it. Ordinary self-calls
remain ordinary JavaScript calls.

Supporting case and synchronous try/catch/finally/throw emission preserves tail
control flow. Try hides the enclosing recurrence target throughout its body,
handlers and finalizer; a function or loop established inside any of those
regions can recur normally. Method recurrence updates supplied `js-arguments`
slots to reproduce upstream non-strict formal-parameter aliasing, despite the
strict generated unit.

Basic declared protocols and fixed/overloaded reify methods supply a real method
receiver context. Ordinary protocol methods accept the receiver explicitly or
omit it. An explicit receiver expression evaluates for its effects; the source
receiver binding continues to observe `this`. Nested loops and functions use
their own parameter counts and conventions.

IFn has two upstream adapters: `.call` includes a hidden receiver parameter,
while the direct invoke entry drops it. A source method must satisfy both, so
its recur argument count is exactly its non-receiver parameter count. Explicit
receiver and too-few argument forms are rejected by both pinned references.
The pinned `.call` adapter also clears the hidden field-access receiver after
recurrence: a later captured outer-local read throws TypeError. The direct
invoke entry retains its captures. Both behaviors, including the trace before
the exception, are retained and tested rather than treated as interchangeable.

The relevant complete test bodies and implementation sections were read:

- `src/test/cljs/cljs/recur_test.cljs`: `Search1` through `Search4` and
  `cljs-2085-test` distinguish explicit/implicit receivers and nested targets.
- `src/test/cljs/cljs/primitives_test.cljs`: `test-arithmetic`, especially
  assertion 135's closures retained by a loop.
- `src/test/cljs/cljs/core_test.cljs`: complete `threading-loop-recur`,
  `cljs-739`/`test-739`, `case-recur`/`test-812`, and `test-cljs-3309` contexts.
- `src/test/clojure/cljs/analyzer_tests.clj`: loop/recur analysis forms,
  `test-cljs-2475`, `test-cljs-2476`, and `test-cljs-3031`.
- `src/main/clojure/cljs/analyzer.cljc`: `analyze-let`, function method frames,
  `parse 'recur`, and the recurrence-disallowing boundaries.
- `src/main/clojure/cljs/compiler.cljc`: function method, loop and recur emitters.
- `src/main/clojure/cljs/core.cljc`: `adapt-proto-params`,
  `adapt-ifn-params`, `adapt-ifn-invoke-params`, and `add-ifn-methods`.

## Assertion accounting and limits

Seven assertion-faithful witnesses pass strict execution on generated JavaScript,
Maven and the checkout: all five `threading-loop-recur` assertions,
`test-arithmetic::assertion-135`, and `test-812::assertion-001`. The latter keeps
the original `case-recur` definition; the closure witness retains actual `map`,
function creation and invocation. Mappings, original assertion hashes and
adaptation rationales are in `compiler-language/witnesses.jsonl` and
`fixture-origins.tsv`. The source inventory and complete body hashes are unchanged.

The **53 independent recurrence probes** compare full values, exception
categories, ordered effects and stdout. They cover 200,000 iterations, overloads,
variadic rest identity/nil, destructuring, simultaneous swaps, initializer effects,
function/let/loop captures, nested targets, all three emission contexts, receiver
expressions, IFn entry differences, local recurrence within exception regions,
argument aliases and stdout. Direct Node assertions separately execute a million
function iterations and inspect retained closure arrays without the shared encoder.
Namespace tests cover aliases, referred protocol methods and separate recurrence
targets. Ten added negative boundary rows cover tail/target/count/cross-try/IFn
rules; all **153** current boundary classifications match both references.

This is recurrence coverage, not completion of the entire recursion inventory.
The original Search assertions still require their actual record constructors;
independent reify probes do not claim those record assertions. Local `letfn` is now
covered separately by [its audit](compiler-letfn.md). Full record/type construction, extension APIs, mixed IFn/custom protocol objects,
printing/dynamic-binding contexts of `test-739`, comprehension contexts, and
numeric-warning analysis remain separately visible in the language matrix.
Collection-taking `map` supports the closure witness; this does not claim its
transducer arity or the full chunked lazy-sequence contract.

## Verification

The focused Rust suites passed **144 tests** with **14** explicitly ignored
reference tests in the ordinary invocation. The recurrence differential and
analyzer-boundary reference tests passed separately; the strict witness report
has **7 covered, 0 missing, 0 version differences**. Standalone Node runtime
assertions, Clippy, formatting, inventory regeneration and whitespace checks passed.

```sh
cargo test --locked --test analyzer --test boundaries --test cli \
  --test compiler_macros --test compiler_reader --test execution \
  --test namespaces --test provenance --test compiler_language
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  compiled_recur_matches_pinned_values_effects_and_stdout -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  analyzer_boundary_classifications_match_reference -- --ignored --nocapture

python3 - <<'PY'
import subprocess
ids = [f'core_test.cljs::threading-loop-recur::assertion-{i:03}' for i in range(1, 6)]
ids += ['primitives_test.cljs::test-arithmetic::assertion-135',
        'core_test.cljs::test-812::assertion-001']
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for identity in ids:
    command += ['--id', identity]
raise SystemExit(subprocess.call(command))
PY
```

## Output and initialization observation

A constant still emits **104 bytes** without the runtime. `(let* [x 42] x)`
emits **133 bytes**, and `(loop* [x 42] x)` emits **210 bytes** (152 gzip bytes).
The loop adds 77 bytes and no runtime prelude. A numeric function with `pos?`
and `dec` emits 54,314 bytes; replacing its decrement result with recur emits
54,537 bytes: **223 additional bytes**, or **64 gzip bytes**. Numeric operations
already require the runtime in both cases.

On this Mac and Node v24.18.0, median warm evaluations of precompiled `vm.Script`
units (seven batches of 1,000 after 100 warmups) were approximately 0.47 microseconds
for the let unit, 0.45 microseconds for the loop unit, and 70 microseconds for
initializing the arithmetic runtime and function. These are local initialization
observations, not browser performance claims or a full compiler benchmark.
