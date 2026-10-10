# Validated compiler and JavaScript collection measurements

See [methodology and interpreted results](../../../compiler.md).

These are the compact reports from the completed run at
`/private/tmp/light-cljs-compiler-report-73-final`, which also retains five exact
executables, current and baseline source archives, emitted JS and Node drivers.
The manifest records every fixture/source/executable identity; baseline identity
records the original Git archive. All CSV/JSON samples and distributions are
retained here. `operation-output-size.json` is derived separately from the saved
`emitted/operations.js` using byte count, SHA-256 and Python `gzip.compress` with
`mtime=0`; it supplements the runner's startup-program size table.

The source snapshot contains accumulated uncommitted implementation work; do not
identify it solely by HEAD. The runner and runtime source hashes still match the
working source after measurement. Report prose and this index were added after
timing. No benchmark ran concurrently with the correctness suites.
