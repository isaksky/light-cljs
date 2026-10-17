# Phase22 intrinsic list construction

`listFromArray` builds nodes with their known stored counts. Previously every
node called general `count` on its newly created tail, triggering protocol lookup
and compiler declaration-demand work. Besides wasting time, a public ICounted
override could corrupt a three-item list's stored count to 1000. The direct
builder keeps it at three while an explicit public count still sees the override.

The regression matches pinned upstream ClojureScript 1.12.134 and passes in
application code at all optimization levels and inside macros. It explicitly
declares the extension so Advanced retains its public dispatch hooks. The old
implementation fails the construction assertion; direct QuickJS before/after
probes and upstream build/output logs are retained here.

| Job | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 21.09 | 540917760 |
| main none | 159.03 | 1364803584 |
| main advanced | 162.93 | 1424621568 |
| shadow release | 55.70 | 3980705792 |
| shadow compile | 30.65 | 2302279680 |

Fresh processes, cold shadow compiler caches, one job at a time, no overlapping
builds, tests or profiles. Main none and advanced improve about 3% from phase20;
advanced peak RSS is about 7% lower in this observation. Advanced remains 2.93
times slower than fresh shadow release at 36% of its peak RSS; none is 5.19
times slower than shadow dev at 59% of its peak. Scope differences and JVM
RSS variability remain relevant. The speed goal is not achieved.

`cargo test`: 1,384 passed, zero failures, 106 ignored. Strict Clippy, formatting,
diff checks and all three direct Node workflows pass. The first full suite
caught a manifest requirement: a builder that writes list cells must be a target
of every corresponding kind hook. The seven effect headers are corrected and
the full suite was rerun. The earlier failure log is retained separately.

The readable runtime grows by 390 bytes, and terse slices that use the builder
grow by 99 bytes. Eighteen none/prepare ceilings move up 1,000 bytes; only the
features advanced fixture exceeds its prior ceiling, by 12 bytes, and moves to
71,000. Retain this correctness/performance tradeoff in the eventual commit.

All three application bodies are byte-identical to phase6b outside the embedded
runtime and complete outputs pass Node syntax checking. The 996-file port hash
is unchanged. Penpot application execution is still untested.

Diagnostic evidence uses the isolated phase21 interpreter sampler. Render falls
22.51 -> 20.14 seconds and main 166.78 -> 155.48 seconds; these are not clean
production timings. Main interrupt-check samples fall 549,340 -> 488,399.
The one-million-node QuickJS construction probe falls 4,135 -> 540 ms. Sampler
counts describe poll frequencies, not wall-clock shares. Prototype output checks
are separate from the production verification above.

Production binary, exact tracked patch and untracked name-index source are under
`/private/tmp/light-cljs-phase22/production/` and fingerprinted here. The isolated
prototype, probes and reference fixture are under `/private/tmp/light-cljs-phase22/`.
All production changes remain uncommitted.
