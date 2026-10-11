# Compiled definitions, Vars and synchronous binding

Contract: ClojureScript checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`
and Maven **1.12.134**. Every differential run verifies source origin/revision,
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.

## Source and assertion evidence

The complete `binding_test.cljs`, `binding_test_other_ns.cljs` and
`var_test.cljs` bodies were read, together with the Var, defonce and resolve
regressions in `core_test.cljs`. The executable inventory now retains
`var_test.cljs` as a twelfth complete source file, including both assertions in
the function's `:test` metadata. This adds four sites to the inventory without
changing the prior assertions or their expectations.

The selected witnesses retain **24 original assertion sites**: all 12 binding
assertions, all four Var-test assertions (including the metadata callback), and
eight core assertions from `test-var?`, `test-var-arglists`, `test-cljs-1187`,
`test-resolve` and `test-cljs-3333`. Whole namespace bodies keep their support
definitions, aliases, dynamic tags and evaluation order. Test wrappers become
callable functions/`do`, `is` becomes throwing `assert`, and `testing` becomes
`do`. The `test-cljs-1187` witness retains its original three helper functions,
`print`, `with-out-str`, `case`, explicit Vars and `apply`.
`compiler-language/witnesses.jsonl` pins every assertion hash and explains the
adaptation; `fixture-origins.tsv` retains source revision/path/test anchors.

Implementation evidence also includes `core.cljc`'s `declare`, `defonce`,
`binding`, `with-redefs`, `resolve`, `exists?` and `with-out-str`; `core.cljs`'s
`Var`, `test`, `print` and print roots; analyzer `var-meta`, `var-ast`,
`confirm-bindings`, and `parse def/var/set!`; and compiler `emit* :def`.
The complete analyzer bodies `test-constants`, `var-args-error-test`,
`test-cljs-1871`, `test-cljs-1992`, `test-cljs-2101`, `test-cljs-2139`,
`test-cljs-2182` and definition-related `analyze-ops` sections were inspected.
Seven invalid-source probes compare constant assignment/redefinition, local
mutation, malformed Vars and unquoted resolve against both pins. Analyzer
internal AST and warning assertions are not counted as semantic passes.

## Implemented behavior

Ordinary globals remain separate JavaScript storage slots. `def` retains its
initializer result, accepts a docstring and definition metadata, and registers
its name before analyzing initialization. Reserved definition source filename/line/column metadata is authoritative even
when user metadata supplies those keys; see [source metadata evidence](compiler-source-metadata.md).
`declare` permits forward calls and
preserves existing definitions. Nonempty tail declarations yield undefined;
empty declarations yield nil. Required value positions for uninitialized
definitions/declarations remain rejected because the pins emit invalid JavaScript
there. Ordinary undeclared reads retain the compiler's located strict diagnostic.

`defonce` tests JavaScript undefined, including after a previous initializer
returned undefined. Nil and false are initialized roots and suppress a subsequent
initializer. `binding` and `with-redefs` first save every original root, evaluate
all initializers, install values, and restore in reverse order in `finally`.
Initializers that throw do not enter the body/restoration region; any mutation
performed by such an initializer remains, matching upstream. Duplicate targets,
body mutation, nested scopes, arbitrary thrown values and cross-namespace reads
are exercised. `set!` supports resolved globals, core roots and host properties;
local mutation and constant mutation/redefinition are rejected.

Scopes are synchronous. Captured locals preserve snapshots, live global reads
see current temporary roots, and Promise callbacks run after restoration. Both
pins verify this behavior. Explicit boolean tags on globals select JavaScript
truth tests, including the upstream empty-string regression; untagged dynamic
globals use ClojureScript truthiness.

Explicit Vars are distinct callable objects with symbol-based equality/hash,
live dereference/invocation, immutable metadata copies, public constructor/fields
and `isMacro`. Definition metadata is captured at the Var expression; its getter
continues observing later redefinitions. `resolve` accepts a quoted symbol,
resolves aliases and locals, and checks existence at runtime. A name unresolved
at analysis can become defined later; its original metadata remains nil.
Core Var metadata is generated from the pinned core analyzer definitions for
only the runtime's supported names. The checked data table ships with Rust and
only requested metadata is emitted into JavaScript. `test` follows function test
properties or captured Var `:test` functions. The pins' tail `defn` returns before
its later test-property assignment; this context distinction is preserved.

The supporting `print`/`with-out-str` path executes original Var regression code.
Print roots can be rebound, capture nests, and failures restore the outer sink
and readable flag. This is not a claim to complete unrelated printing protocols
or every core library helper.

## Verification

```sh
cargo test --locked --tests --quiet
node tests/support/js_runtime.test.js
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
python3 -B tests/reference/compiler-language/extract_inventory.py --check
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:local/root "research/clojurescript"}}}' \
  -M tests/reference/generate_core_var_metadata.clj --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution compiled_definitions -- --ignored --nocapture

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution analyzer_boundary_classifications_match_reference -- --ignored --nocapture

python3 - <<'PY'
import json, subprocess
command = ['python3', '-B', 'tests/reference/compiler-language/report.py', '--scope', 'historical-selected',
           '--run', '--strict', '--reference', 'both']
for row in map(json.loads, open('tests/reference/compiler-language/witnesses.jsonl')):
    if row['fixture_id'].startswith('cljs-definitions-'):
        command += ['--id', row['assertion_id']]
raise SystemExit(subprocess.call(command))
PY
```

Direct Node assertions bypass the shared encoder to check Var identity, exact
getter receiver, public fields, metadata identity, live callable replacement,
structural map-key lookup and callback counts. The independent probe file retains
literal values, effect traces and stdout expectations; failures cannot count as
passes. The namespace fixture also exercises runtime/macro aliases and renames,
macro-generated definitions and scopes, qualified declarations, saved functions,
Var metadata and Promise callbacks.

The differential runs passed **53 independent outcomes** and the shared namespace
program on both references. The strict witness run passed all **24 selected
sites**, with zero missing or version-difference rows. The complete analyzer
boundary comparison passed **181 classifications** on each pin; the existing
release/checkout difference for an ampersand let binding remains explicitly
classified. Reference runtime versions were Maven **1.12.134** and checkout
**0.0.135109660**, with Clojure **1.12.4**, Java **25.0.2** and Node **v24.18.0**.

## Output size and initialization

[Raw samples](../../benchmarks/results/compiler-2026-10-10/definitions/output-startup.json)
retain nine fresh Node parsing/initialization samples per program. Primitive `42`
remains **104 bytes**; a plain definition is **128 bytes** and a primitive binding
scope **261 bytes**, all without the runtime. Emission tracks used expressions,
so unused definition metadata cannot accidentally include the collection prelude.

The shared runtime grows from **78,234** to **80,031 bytes** (+1,797, about 2.3%);
deterministic gzip grows from **18,067** to **18,497 bytes** (+430). For the same
`(count [1 2 3])` program with before/after runtime text, median fresh Node
parse/initialization is **1.479 ms** before and **1.521 ms** after. A Var metadata
program is **80,403 bytes** with a **2.186 ms** median. These local samples are
observations, not browser performance guarantees. The core metadata source table
is compiler-side data and is not added wholesale to generated JavaScript.

The ordinary full Rust run passed **635 tests**, with **29** reference/tooling
checks explicitly ignored in that invocation. After the final existence-probe
resolution refinement, the focused analyzer/boundary/compiler/macro/reader/
namespace/provenance/inventory/execution run passed **155 tests**, with **25**
references explicitly ignored there. The two definition differential tests and
strict imported assertion comparison were run separately and passed. Direct
Node assertions, all-target Clippy, formatting, pinned metadata regeneration,
source inventory and whitespace checks passed.
