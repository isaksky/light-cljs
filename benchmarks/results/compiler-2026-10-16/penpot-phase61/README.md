# Macro argument traversal replay — V8 trial, not retained

Phase58 with only a compiler form bridge change: record the initial descriptor
postorder and replay each argument's range, instead of constructing another
explicit traversal stack. Metadata/provenance restoration still precedes each
argument's canonical snapshot. Final descriptor-to-value lookups preserve shared
descriptor behavior.

Main none takes **25.29 s / 1,320,534,016 bytes RSS**, versus phase58's 25.69 s.
This small difference does not establish a speed gain. The separate QuickJS
trial in phase62 shows no improvement either. The change is not retained.

The 500-case differential probe passes, including shared descriptors, input
identity, interleaved metadata/provenance, input release and terminal limits.
No full suite was run for this rejected optimization. The initial byte comparison
failed because none-mode embeds the modified bridge. An exact replacement check
confirms that this expected runtime function is the only output difference.
The known main ESM error and V8 resource-limit gaps remain.

Commands, runtime patch, differential probe, output equivalence and hashes are
preserved. The frozen executable and full output remain under
`/private/tmp/light-cljs-phase61`.
