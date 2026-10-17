# Cold Penpot comparison, phase6

The validated binary includes the persistent name/arena/environment work, native
UTF-16 bridge, namespace registration index, completed-initializer cleanup,
native Map hash repair, and precomputed protocol masks. It does not include the
subsequent checked-array warning index or failed-import validation cleanup.

`results.edn` records exact commands and measurements. The five jobs ran
sequentially using `bb benchmarks/penpot.bb /private/tmp/light-cljs-phase6b`.
Shadow's compiler cache was removed before each job. The unchanged input port's
path/content hash is in `port-hash.txt` (sorted repository-relative paths followed
by file bytes, without separators). `source-hashes.txt` identifies the source
base, saved patch, untracked name-index source and executable.

`tests.log` records 1,376 passing tests and 106 skips. Strict Clippy passed.
Direct Node runtime, core-helper and execution workflows passed; the execution
log is included. `size-tests.log` verifies the existing output-size ceilings.
An earlier suite run found a five-byte ceiling overrun, corrected by compacting
the protocol-mask entries; no test ceiling changed.

`equivalence.txt` compares each application body with phase2f after excluding the
embedded runtime, which changed by 30 bytes. Complete outputs also pass Node's
syntax checker. No Penpot bundle or browser execution was performed.

Large JavaScript outputs, the executable and source patch remain in
`/private/tmp/light-cljs-phase6b/`; this directory keeps the compact evidence.
