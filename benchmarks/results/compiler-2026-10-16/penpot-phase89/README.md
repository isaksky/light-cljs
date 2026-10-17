# Phase89: QuickJS detached registration without caching — not retained

The exact phase88 binary with `LIGHT_QJS_NAMESPACE_CACHE=0`. Registration remains
separate from deferred unit code, but each registration is evaluated normally.

Main none takes **90.25 s / 947322880 peak RSS bytes**, compared with cached
phase88's 89.04 s / 943587328 bytes and phase87's 91.34 s / 893534208 bytes.
Neither the split alone nor the cache gives a useful improvement over phase87.
Both are excluded from the selected QuickJS candidate.

Complete output matches phase42, including the known main ESM defect. This is a
negative control, not a new source revision or a full-suite validation. The exact
command, environment override, timing and binary/output hashes are retained.
