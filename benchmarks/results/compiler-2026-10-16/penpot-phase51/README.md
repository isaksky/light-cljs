# Compact registration strings with the production QuickJS backend

This isolated candidate applies only phase44's compile-time registration
string-quoting change to production phase47. Printable ASCII uses the existing
readable quoting routine; quotes, backslashes, non-ASCII and control units keep
correct escaping. Application emission is unchanged. Registration remains in
the original unit and its order is unchanged; this trial does not introduce
V8 or the prototype's separate registration evaluation.

The clean full main-none build takes **104.20 s**, 103.89 user seconds, 0.29
system seconds and **1,169,752,064 bytes peak RSS**. Its complete output matches
phase42 byte for byte. Compared with production phase47's 111.18 s /
1,251,033,088 bytes, this observation is 6.3% faster with 6.5% lower RSS.

The change is not yet promoted and its full suite has not yet run. The next
combined QuickJS candidate will test this quoting change with the allocator.
No browser validation is claimed; main still has the baseline ESM syntax defect.

`registration-quoting.patch` applies to production phase47. Frozen binary and
complete output remain under `/private/tmp/light-cljs-phase51/`, with hashes,
commands and build/timing logs here.
