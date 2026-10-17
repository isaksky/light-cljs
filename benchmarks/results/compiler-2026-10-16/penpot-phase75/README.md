# Phase75: reader input revision trial — not retained

Based on the isolated phase71 V8 candidate. Macro import/refers mutations carry
a revision; a reader view compares it and the macro count instead of rebuilding
and hashing unchanged aliases/refers/excludes for each form.

The diagnostic main-none run takes **22.54 s / 1239957504 peak RSS bytes**,
including statistics and terminal GC. Phase71 took 22.40 s / 1177714688 bytes.
This does not show a performance benefit. The trial is not retained and no full
suite was run for it. Complete output matches phase42, including its known
main ESM syntax defect.

Further inspection of the phase74 native sample attributes 386 of the 479
exclusive global-iterator observations to reader-view updates. Eliminating
unnecessary global iteration is a more promising target than replacing the
configuration hash alone. The patch and exact run evidence are preserved here;
production remains the separately validated QuickJS phase72.
