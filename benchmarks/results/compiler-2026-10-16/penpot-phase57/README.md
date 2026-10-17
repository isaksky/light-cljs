# Unretained compiler runtime storage trials

Built from phase56 with its thin-LTO release profile. The trial transforms only
the runtime installed in the V8 macro session; emitted application runtime text
is unchanged. Every complete main-none output matches phase42.

| Compiler-runtime change | Seconds | Peak RSS bytes |
|---|---:|---:|
| Separate private-field class code for seq stores | 25.17 | 1,358,528,512 |
| Private-field storage for borrowed provenance | 24.61 | 1,315,110,912 |
| Private-field storage for metadata | 24.99 | 1,324,154,880 |

The unchanged phase56 observation was 25.14 s / 1,314,275,328 bytes. These
single-run differences are small; no full suite was run and none of the runtime
changes is retained. In particular, changing per-bridge provenance to private
fields needs separate lifecycle analysis: fields remain with a surviving value
even if its original bridge is released. The larger hypothetical combined
`all-stores` variant was not run.

The exact runtime transformations, build settings, benchmark environment,
commands, timing, build log and source/binary/output hashes are preserved.
