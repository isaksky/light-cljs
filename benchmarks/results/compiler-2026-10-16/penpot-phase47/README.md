# Restrict the compiler reader's temporary binding table

The compiler's private reader view only qualifies names. Previously, every new
view allocated binding names and hash-table entries for all declared globals,
including namespaces that could not affect qualification. The view now keeps
bindings from the current namespace, referred namespaces and alias prefixes.
Fully qualified names outside those namespaces already qualify to themselves.
Declaration frontiers and pending declarations still advance as before; public
`NamespaceView` binding introspection is unchanged.

The regression covers a namespace alias colliding with a transitively loaded
namespace, an unresolved name through that alias, a renamed refer, an excluded
core name, and a qualified name outside the retained namespace set. It observes
both ordinary syntax-quote and a macro's captured definition view. The test
passes against the old reader and the candidate. Two earlier fixture drafts
used a forbidden direct alias collision and an unset print function; their
failed logs are retained, and neither is the final regression.

The combined isolated V8 candidate includes phase46's ASCII name allocation
change. Main-none takes **35.71 s**, 40.23 user seconds, 0.84 system seconds and
**1,373,241,344 bytes peak RSS**, down from phase46's 39.28 s / 1,429,569,536 bytes.
The complete output matches phase42 byte for byte. The full suite passes
**1,402 tests, zero failures, 106 ignored**, with `--test-threads=4`. Phase46's
default-concurrency deadline failures remain documented; this does not make
the experimental V8 backend production-ready.

Both engine-independent changes are now retained in production. Its full suite
passes **1,401 tests, zero failures, 106 ignored** at normal test concurrency.
Strict Clippy, formatting, diff checks and all three standalone Node workflows
pass. The fresh five-job benchmark chain completed sequentially:

| Build | Wall seconds | User seconds | System seconds | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: |
| light render | 14.07 | 13.95 | 0.09 | 500580352 |
| light main none | 111.18 | 110.73 | 0.42 | 1251033088 |
| light main advanced | 116.56 | 116.07 | 0.46 | 1342390272 |
| shadow release main | 56.36 | 135.09 | 387.27 | 5344395264 |
| shadow compile main | 30.27 | 87.76 | 389.35 | 2139226112 |

Main none is 5.7% faster than phase45's 117.87 s, with 1.8% higher peak RSS.
Advanced is 5.2% faster than phase45's 123.01 s, with essentially unchanged RSS.
Production remains **3.67 times slower** than shadow dev at 58% of its RSS;
advanced is **2.07 times slower** than shadow release at 25% of its RSS.
The isolated V8 main is still 18% slower than this shadow dev observation.
The requested overall speed goal remains unmet. JVM RSS varies substantially
between runs; the scope and optimizer differences still apply.

All three complete production outputs match phase42 byte for byte. Explicit
ESM syntax checking passes render and rejects both main outputs with the known
rumext redeclaration defect. The port's 996-file fingerprint is unchanged.
`production/` contains full logs, commands, timing records, syntax diagnostics,
the captured tracked patch and the essential untracked `name_index.rs`.

`reader-bindings.patch` is the delta from phase46; apply that phase's
`name-allocation.patch` to phase45 first. The V8 experiment reconstructs from
the phase42 worktree using phase44's prototype patch, phase45's namespace-count
patch, then these two patches. Production keeps QuickJS and its dependency
files unchanged. Frozen binaries and outputs remain under
`/private/tmp/light-cljs-phase47/`, with hashes here.

All heavy builds, tests, samples and benchmarks run sequentially. Main's known
baseline ESM redeclaration defect remains; byte equivalence does not establish
that Penpot runs in a browser. Phase48 contains the new native profile.
