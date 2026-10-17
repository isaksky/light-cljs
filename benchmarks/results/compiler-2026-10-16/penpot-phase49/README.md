# Vectorizable prefilter for generated parameter names

This isolated trial keeps the existing streamed substring search, including
matches spanning strings and unpaired UTF-16 surrogates. When no partial match
is pending, it skips strings lacking the identifier's first character using
the standard library's vectorizable `u16` slice search.

Both existing regressions pass: all split points against the prior lossy
concatenation oracle, and raw-JavaScript runtime-parameter hygiene. The full
V8 main output remains byte-identical to phase42. The clean observation is
**34.91 s / 1,410,367,488 bytes RSS**, versus phase47's 35.71 s / 1,373,241,344.
That is only a 2.2% time difference in one trial, with higher RSS; it needs a
repeat before claiming a reliable whole-build improvement. No full suite has
been rerun for this variant. It is not promoted.

`scan.patch` applies to the phase47 V8 prototype. The change itself is engine
independent. The frozen executable and full output remain in
`/private/tmp/light-cljs-phase49/`, with hashes here. The known baseline main
ESM defect remains.
