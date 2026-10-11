# Supplemental public boundary verification

`boundary-support.jsonl` retains the exact source and SHA-256 of the 27 legacy
boundary rows whose upstream acceptance had been reported as a missing local
capability. It is independent of the imported test inventory: none of these
checks adds an upstream assertion, witness, or semantic pass.

`report.py --run --strict` runs this catalog in addition to the selected source
witnesses. `source_strict_complete` and `boundary_strict_complete` are separate;
`strict_complete` requires both. Without live local and both pinned reference
observations, supplemental entries remain unverified. The catalog fixes all 27
IDs so a deleted entry cannot silently remove a required check.
The retired `macro-defmacro` rejection row remains anchored here to its exact
legacy source and hash; its accepted declaration no longer belongs in the
rejected-source inventory.

The original source is analyzed in its original expression, statement,
namespace, or `.cljc` context. This avoids treating the legacy local test's
unselected-branch wrapper as equivalent to bare upstream source. Every probe
checks compilation acceptance or the exact analysis rejection, ordered warning
kinds and messages, compiler identity, and effective settings (`:none`,
`:static-fns false`, `:def-emits-var false`, checked arrays disabled). Accepted
local output must also pass JavaScript syntax checking. Quiet public syntax
uses default compilation. Genuine warning cases use `--warnings`; namespace
and `.cljc` cases use the public source-root loader with explicitly retained
source files and dependency declarations.

There are 10 runtime probes shared by local and reference execution, one local
source-macro runtime probe, and three retained reference-only macro runtime
probes. They check stdout, effects and values using the existing outcome adapter
and both checked reference launchers:

- Unqualified host calls retain their receiver; dotted lexical function values
  retain the upstream null-receiver calling convention. `console.log` returns
  the tagged JavaScript `undefined` value and prints exactly `1\n`.
- Dotted definitions write host properties. `set!`, reads, Var dereference, and
  Var namespace/name metadata are checked. A separate complete diagnostic probe
  retains the warnings from the two metadata `:test` references per host Var.
- The global name `&`, doubled-dot JavaScript munging, qualified method paths,
  and ordinary property reads use their actual emitted runtime behavior.
  Qualified `.-a/value` calls `_a.value()`; unqualified `.-value` reads a field.
- Inline `.cljs` `defmacro` is an `intentional-difference`. The three original
  runtime sources and hashes remain under `reference_runtime`: both pinned
  references execute the ordinary runtime-function ABI, including hidden
  `&form`/`&env` parameters, metadata, multiple/variadic methods, `apply`, receiver
  behavior, dispatch properties and exact invalid user-arity errors. Local
  `local_runtime` instead executes compile-time source expansion, original
  `&form`, expression `&env`, and multiple/variadic macro methods from a real
  namespace fixture. Separate hashed `local_probes` require exact phase errors
  for runtime value, `apply` target and Var uses. Local declarations emit no
  application macro function or Var. This verifies the deliberate phase change;
  it does not count unchanged runtime compatibility or an upstream assertion.
- Duplicate namespace aliases retain function identity and callable alias and
  macro imports; reserved `js` aliases preserve JavaScript interop. `.cljc`
  selection executes through the file API.

Four malformed raw `fn*`/`let*` inputs remain explicit
`primitive-permissiveness` differences. For each, the exact corresponding
public `fn`/`let` spelling must reject locally and in both references. These
rows verify diagnostic differences, not runtime support. The local `&` binding
row separately verifies the accepted release versus rejected checkout/local
version difference.

Expression runtime sources are hashed; namespace runtime trees retain hashes
for every source file. Failure-injection tests cover omitted catalog entries,
changed original sources, dropped runtime proofs, altered public spellings,
wrong diagnostics, wrong runtime outcomes, crossed macro phase proofs, and
missing reference layers. The
committed catalog is a contract; only the current execution can verify it.


## Same-file macro verification (2026-10-10)

The following focused live check exited 0 with
`Macro boundary status: verified-intentional-difference`. It checked the local
compiler and both pinned reference layers; the three original reference runtime
sources and hashes remained unchanged.

```sh
python3 - <<'PY'
import sys,json,tempfile
from pathlib import Path
sys.dont_write_bytecode=True
sys.path.insert(0,'tests/reference/compiler-language')
import report
case=next(c for c in report.boundary_support_contract() if c['id']=='macro-defmacro')
with tempfile.TemporaryDirectory(prefix='light-cljs-101-boundary-') as directory:
 evidence=report.run_boundary_support([case],str(Path('target/debug/light-cljs').resolve()),Path('research/clojurescript').resolve(),Path(directory),60,180,'both')
 print(json.dumps(evidence,indent=2))
 status=report.boundary_support_status(case,evidence[case['id']])
 print('Macro boundary status:',status)
 assert status=='verified-intentional-difference'
PY
```
