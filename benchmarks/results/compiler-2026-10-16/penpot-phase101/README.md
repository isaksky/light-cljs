# Phase 101: valid Penpot ES-module output

The compiler now supports native `var` destructuring into existing ClojureScript
locals, as emitted by Rumext. It parses candidate raw-JavaScript templates and
turns those declarations into assignments. Raw names sharing the declaration
retain their function-scoped declaration. Nested-function bindings and strict
directives retain their semantics. Locals exposed directly through `js*` holes
use lexical `let`, including per-iteration copies, preserving closure capture.
The rule also applies to chunked output and compile-time macro execution.

The compiler leaves templates it cannot parse unchanged. This does not attempt
to turn arbitrary JavaScript statements into expressions.

## Validation and output audit

The exact promoted source passes **1,406 tests, zero failures, 106 ignored** at
normal concurrency, strict Clippy, formatting and all three Node workflows.
The new tests exercise positional and named holes, mixed declarations, defaults,
rest patterns, uninitialized redeclarations, for/in and for/of parsing, nested
functions, strict directives, ordinary assignments, loop closures and use inside
a macro. Execution regressions run at every optimization level.

All compiler, test, vendor, bench and build inputs match the validated candidate.
The production binary was rebuilt after this identity check. All three complete
outputs pass `node --check --input-type=module` through stdin. The 996-file port
hash is unchanged. No Penpot source or macro port was edited.

Every changed output line was audited against phase100:

| Output | `const` to lexical `let` | `var` pattern to assignment | Unchanged lines |
| --- | ---: | ---: | ---: |
| render | 409 | 0 | 148,763 |
| main none | 8,012 | 74 | 411,722 |
| main advanced | 7,704 | 74 | 328,190 |

There are no other output changes. Main none is also byte-identical between
the isolated validated candidate and the production rebuild. This fixes the
known main ESM syntax failure; it does not establish bundled/browser execution
of the complete application.

## Fresh sequential cold builds

| Build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| light-cljs render | 11.47 | 309,379,072 |
| light-cljs main none | 103.58 | 803,225,600 |
| light-cljs main advanced | 108.10 | 900,988,928 |
| shadow release main | 64.96 | 4,344,512,512 |
| shadow compile main | 36.19 | 2,330,918,912 |

The isolated candidate first took 103.43 seconds / 823,541,760 bytes for main.
Production main remains 2.86 times slower than fresh Shadow dev, at 34.5% of its
RSS. The performance goal remains unmet. The V8 port is measured separately.

## Evidence

`changes.patch` is relative to phase100. Validation logs, commands, scripts,
output audits, source hashes, all five production measurements and explicit
syntax-check logs are retained here. `production/implementation.patch` and
`production/name_index.rs` preserve the complete dirty compiler implementation.
The binaries and complete generated outputs remain under
`/private/tmp/light-cljs-phase101/`.
