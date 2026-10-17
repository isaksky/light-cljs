# Unread core macro `&form` trial — not retained

Phase58 with an isolated shortcut for compiler-owned macros whose first formal
is proven unread, including nested captures and argument reflection. Source
macros stay on full transport. A conservative input guard rejects root/head
metadata, keyword or scalar metadata, reader literals and insufficient
node/depth budgets. Eligible calls import nil as the unused formal and import
each positional argument with the ordinary snapshot path.

The clean main-none result is **24.82 s / 1,343,504,384 bytes peak RSS**.
Complete output is byte-identical to phase42. Phase66's QuickJS observation is
only 1.7% faster than production, with slightly higher memory; the complexity
does not justify retaining this shortcut. No full suite was run or promotion
performed. The known main ESM failure and V8 resource gaps remain.

All **10 focused bootstrap tests pass**, including new checks for captured form
references, argument reflection, source data/metadata equivalence with the
unmodified transport, reader literals and conversion limits. Intermediate test
logs preserve corrections to the test comparator (the bridge's canonical
comparator does not compare separately allocated source Integer/Character
nodes) and to a keyword-metadata fixture rejected by the surface reader. The
final fixture constructs that native compiler datum explicitly. These were
test issues, not observed compiler regressions. The benchmark preceded addition
of the test-only code; implementation behavior is unchanged.

The first 24.45 s run is **invalid and excluded**: the shared Cargo target
recompiled Rust but reused the preceding active-stack trial's generated runtime.
It is saved as `stale-generated-runtime-*`. Scratch builds now touch `build.rs`
as well as both crate roots so generated runtime source also refreshes. The
valid binary has a different hash and its complete output matches phase42.

Patches are relative to phase58. Commands, focused test logs, timings and hashes
are archived. Frozen binaries and full outputs remain under
`/private/tmp/light-cljs-phase65`.
