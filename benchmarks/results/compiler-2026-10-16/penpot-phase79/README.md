# Phase79: allocation probe with shared metadata-map layers

Phase77's diagnostic allocator with phase78's shared map layers. All complete
output remains byte-identical to phase42, including the known main ESM defect.

| Checkpoint | Live requested bytes | Peak requested bytes | Cumulative requested bytes |
| --- | ---: | ---: | ---: |
| after analysis, session retained | 651969388 | 663954450 | 12908775241 |
| after macro-session release | 196537702 | 663954450 | 12908775626 |
| after output preparation | 197753012 | 663954450 | 12943895401 |
| after output emission | 264959874 | 663954450 | 13427574639 |

Compared with phase77, this removes 2,317,342,878 cumulative requested bytes
(15.2%) during analysis. It does **not** reduce retained memory: live and peak
requested bytes are slightly higher, and post-session live bytes are essentially
identical. The optimization reduces copying, not the remaining retained state.
These counters exclude V8/C++ allocations and allocator overhead; they are not RSS.

The diagnostic run takes 20.39 s / 1162821632 peak RSS bytes. Phase78's two
focused map-snapshot tests pass; no full suite is claimed for this instrumentation
or the shared-layer delta. Production remains QuickJS phase72 and the latest
fully validated V8 prototype remains phase76. The patch here is relative to the
phase77 allocation probe.
