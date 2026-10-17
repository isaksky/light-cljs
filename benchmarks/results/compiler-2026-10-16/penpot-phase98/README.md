# Phase98: remaining analyzed-program retention

An isolated diagnostic based on phase97 measures requested Rust allocations
while releasing disjoint parts of the completed compiler session. It does not
alter analysis, macro execution or generated output. These counters exclude
V8/C++, allocator overhead and virtual address reservations.

At the end of Penpot main analysis, all **13,367** analyzed program handles are
uniquely owned by the session. Only **277** have been emitted for macro execution;
the other **13,090** remain available for possible later demand.

| Teardown checkpoint | Live requested Rust bytes |
| --- | ---: |
| after analysis | 499298634 |
| before program release | 495495907 |
| emitted programs released | 491484429 |
| all programs released | 285938105 |
| catalog released | 279228091 |
| bootstrap released | 199530265 |
| complete host released | 196674975 |

Releasing already-emitted programs alone saves only **4.0 MB**. The still-lazy
programs retain another **205.5 MB**. These releases are measured in that order,
not independent sizes; shared tables are freed only when their last owner drops.
The results rule out dual-consumer cleanup alone as a major memory improvement.
Phase99 instead tests retaining generated source and reader-registration tables
after application assembly, while preserving lazy macro initialization.

Diagnostic main takes **22.70 s / 1120501760 RSS bytes** and is byte-identical to
phase42. Explicit ESM checking reproduces its known main defect. No full suite
or application execution is claimed for this instrumentation. Production remains
phase87; the latest validated isolated V8 candidate remains phase97.
