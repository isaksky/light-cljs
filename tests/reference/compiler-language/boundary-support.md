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

There are 13 separately executed runtime probes. They check stdout, effects and
values using the existing outcome adapter and both checked reference launchers:

- Unqualified host calls retain their receiver; dotted lexical function values
  retain the upstream null-receiver calling convention. `console.log` returns
  the tagged JavaScript `undefined` value and prints exactly `1\n`.
- Dotted definitions write host properties. `set!`, reads, Var dereference, and
  Var namespace/name metadata are checked. A separate complete diagnostic probe
  retains the warnings from the two metadata `:test` references per host Var.
- The global name `&`, doubled-dot JavaScript munging, qualified method paths,
  and ordinary property reads use their actual emitted runtime behavior.
  Qualified `.-a/value` calls `_a.value()`; unqualified `.-value` reads a field.
- Inline `.cljs` `defmacro` defines an ordinary runtime function with hidden
  `&form`/`&env` parameters, a macro marker, and public arglists that omit those
  two parameters. Docstrings, custom metadata, multiple/variadic methods,
  `apply`, receiver behavior, wrapper length, dispatch properties, and exact
  invalid user-arity error messages execute. Native `.clj` macro execution is a
  separate compiler facility.
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
wrong diagnostics, wrong runtime outcomes, and missing reference layers. The
committed catalog is a contract; only the current execution can verify it.
