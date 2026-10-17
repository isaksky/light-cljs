# Phase90: native profile of promoted QuickJS production

The exact phase87 production binary was sampled every 10 ms during a complete
main-none build. This diagnostic run took 93.336 seconds; its output matches
phase42, including the known main ESM defect. No source changes were made.

The profile contains 7,784 main-thread observations:

| Frame | Observations | Share | Attribution |
| --- | ---: | ---: | --- |
| `JS_CallInternal` | 4,130 | 53.1% | Exclusive: interpreter loop itself |
| `MacroHost::expand_javascript` | 6,808 | 87.5% | Inclusive: expansion, bridge and nested compiler work |
| `expand_source_javascript` | 5,646 | 72.5% | Inclusive: subset of expansion work |

Inclusive categories overlap and must not be added. Other prominent exclusive
frames are QuickJS equality, property access, string hashing, maps and allocation.
This points to macro execution for the next substantial speed improvement;
it does not mean that all expansion time is interpreter overhead.

Mimalloc reports 922.1 MiB peak committed, 2 GiB reserved and 1.5 GiB cumulative
purged. These differ from the reported 847.7 MiB peak process RSS and are not
live object-size measurements.

Scripts, logs, compressed raw sample, extracted counts and binary/output hashes
are retained. The summary parser stops at aggregate tables as well as subsequent
thread headers; its exclusive total is asserted against the 7,784-observation
main-thread header.
