# Phase95: native private-store operations, rejected

An isolated phase87 variant replaces the four private-field store factories
with WeakMaps whose native methods are bound once. Main none takes **89.13
seconds / 916,766,720 peak RSS bytes**, versus phase87's isolated 91.34 seconds
/ 893,534,208 bytes: about 2.4% faster with higher memory. Output differs from
phase42 only in the edited `privateStore` runtime unit. No full suite was run
and the representation change is not promoted.

Phase34 previously tested ordinary WeakMaps on the captured grammar macro and
found only a small gain, with a conflict against the runtime's V8 allocation
strategy. This new whole-build result does not establish a worthwhile tradeoff.
Bound native methods would also evade the existing test's interception of
`WeakMap.prototype.set`; that test must not be cited as evidence that this
variant avoids WeakMap writes. The test was not changed or run for this trial.

Scripts, patch, output comparison, build/timing logs and binary/source/output
hashes are retained. The clean binary was checked for the absence of diagnostic
engine sampling. Production remains phase87.
