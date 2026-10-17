# Keyword record-field lookup

Phase32 makes record lookup, contains and assoc compare keyword names,
matching upstream's keyword case and keyword identity. General IEquiv
dispatch previously allowed a key to select the wrong field. The new
regression fails before the fix, matches ClojureScript 1.12.134, and
passes at every optimization level and in a source macro. Dissoc's
existing equality path is unchanged.

The captured Instaparse macro replay drops from about 11.0 seconds
to 9.56 and 9.42 seconds with the production runtime and identical
encoded results (the earlier prototype measured 9.3 seconds). Its diagnostic and the
rejected declaration-demand cache experiment are in phase31. The full
runtime grows 493 readable bytes; twenty none/prepare size ceilings
across eleven fixtures move to the next 1,000 bytes, with advanced
ceilings unchanged. This rationale belongs in the eventual commit.

The full suite passes **1,394 tests, zero failures, 106 ignored**.
Strict Clippy, formatting, diff checking and all three Node workflows pass.

## Clean rerun

Fresh processes, cold shadow caches, sequential jobs, no overlapping
CPU-heavy tests, builds or profiles.

| Job | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| render | 15.63 | 520765440 | 12621057 |
| main-none | 122.11 | 1275019264 | 33587071 |
| main-advanced | 126.8 | 1390968832 | 25665874 |
| shadow-release | 55.79 | 4160765952 | — |
| shadow-compile | 30.73 | 2421358592 | — |

Advanced remains **2.27 times slower** than shadow release at **33% of its peak RSS**; none is **3.97 times slower** than shadow dev at **53% of its RSS**. The requested speed goal is still unmet.

All three application bodies match phase29 byte for byte outside the
embedded runtime and pass Node syntax checking. The 996-file port
fingerprint is unchanged. Shadow main includes render, rasterizer and
code-highlight; light-cljs main includes its five lazy modules, with
render measured separately. Bundling and browser execution remain
untested. Single-run times and JVM peak RSS have ordinary variability.

The executable, tracked patch, untracked name-index source and emitted
JavaScript remain in `/private/tmp/light-cljs-phase32/production/`.
Source and port fingerprints accompany these results.

## Next transport diagnostic

`argument-sharing-probe.bb` confirms that the current macro argument values
are distinct from their matching elements in `&form`, for vectors, JS arrays,
symbols and regexes. Upstream's `cljs.analyzer/macroexpand-1*` passes
`(apply @mac-var form env (rest form))`, sharing these values. This is an
unfixed issue, not a claimed benefit of phase32.

Removing the separate argument imports must preserve the bridge's source
provenance. Environment imports can overwrite the latest scalar-by-value
provenance and the provenance of interned keyword objects. The old argument
imports restore their provenance and canonical snapshots after the environment
import. A shortcut that merely reuses the whole form's values would omit that
work and could change numeric spelling or source recovery.
