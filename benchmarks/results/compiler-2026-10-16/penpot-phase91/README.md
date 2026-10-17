# Phase91: macro and transport timing

Diagnostic-only instrumentation of production phase87. Main none takes
91.16 seconds / 892,174,336 peak RSS bytes and matches phase42 exactly.
Timers cover 48,216 JavaScript expansions and separately account for nested
macro calls and nested bridge stages. Nanosecond totals, raw logs, scripts,
patch, binary/source/output hashes are retained. No full suite was run for this
instrumented binary, and no diagnostic code is promoted.

| Stage | Seconds |
| --- | ---: |
| Macro call, including demanded helper work | 54.289 |
| Form import | 7.347 |
| Result export | 6.826 |
| Environment import | 6.464 |
| Argument preparation | 4.009 |
| Root release | 0.617 |

The largest complete expansions, including transport, are `rumext.v2/defc`
(21.332 seconds across 1,033 calls), Instaparse's one grammar precompilation
(12.372 seconds), CSS (5.657 seconds), HTML (5.266 seconds) and CSS* (4.233
seconds). Macro and stage totals are different views of the same work; do not
add them. These times include instrumentation and are not a new production
benchmark. Output identity preserves the known main ESM defect.
