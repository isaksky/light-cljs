# Phase71: typed compiler root registration

Isolated V8 trial on phase70; not promoted, no full suite for this delta yet.

Root registration now passes strings, an integer ID, booleans and null through
a private evaluator service. It looks up the public root callback at each call.
Replacing global eval falls back to the original emitted-source path.

Eight focused V8 tests pass, including two new tests covering escaped strings,
null, booleans, callback replacement, replaced eval, owned exceptions and a
terminal session after failure. Existing cache/reentry tests also pass.

The diagnostic main-none build takes **22.40 s / 1177714688 peak RSS bytes**.
This includes statistics and forced terminal GC. Normal evaluated source falls
from 14,977 units / 4,463,749 bytes in phase70 to **993 / 1,666,732**. Namespace
cache counts remain 131,239 hits / 1,292 misses. All complete output matches
phase42, including its known main ESM syntax defect.

The modest timing difference is a single observation. This trial does not
resolve the V8 resource-limit or packaging gaps. Production remains phase52.
`changes.patch` is relative to phase70; scripts, logs, commands and hashes are
preserved alongside it. Frozen binary/output remain in the phase71 temp folder.
