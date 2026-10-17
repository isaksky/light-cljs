# Native QuickJS profile-guided compilation diagnostic

Production remains phase33. Clean isolated sources are built with Clang
instrumentation (-fprofile-instr-generate), trained on the two captured
grammar invocations, merged with llvm-profdata, then rebuilt with
-fprofile-instr-use. Rust optimization settings remain unchanged. The
PGO executable runs twice around a frozen ordinary baseline; jobs are serial.

| Replay | Invocation milliseconds |
| --- | --- |
| Instrumented training | 12799, 12895 |
| PGO first process | 10471, 10508 |
| Ordinary baseline | 9304, 9322 |
| PGO second process | 10510, 10496 |

Every encoded result matches the capture, but this trained build is about
13% slower than the ordinary build. It is rejected; no PGO flags, profile
data or native code changes are added to production. This single training
experiment does not rule out every compiler/PGO configuration.

Frozen executables, runtime, and training profile remain under
/private/tmp/light-cljs-phase40. Scripts and logs are archived here.
