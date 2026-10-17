# Primitive brand rejection: no measurable gain

An isolated runtime adds an early object/function check to storesPredicate,
avoiding the fixed private-brand inventory for primitive values. It changes
no protocol masks or public method slots. Both trials reproduce the captured
Instaparse grammar result exactly.

| Variant | First invocation ms | Second invocation ms |
| --- | ---: | ---: |
| Phase33 baseline | 9,298 | 9,289 |
| Primitive rejection | 9,255 | 9,287 |

The difference is negligible. The experiment is rejected; no production
change or whole-build speedup is claimed. The trials ran sequentially after
phase36 sampling, with no other CPU-heavy task overlapping. Scripts, logs
and fingerprints are retained here. Generated runtime variants remain at
`/private/tmp/light-cljs-phase37/` and use the phase31 replay and existing
standalone QuickJS probe.

The production baseline remains phase33: 1,399 passing tests, zero failures,
main none 121.15 seconds / 1.17 GB RSS, main advanced 125.18 seconds / 1.32 GB.
The speed goal is still unmet. Current scoped timings and native sampling
are archived in phases35 and36; they identify macro execution and transport
as the larger continuing targets.
