# JavaScript vectors and keywords: initial smoke measurements

Local observation on 2026-10-09; no baseline speedup or performance threshold is claimed.

Environment: Apple M3 Ultra (Virtual), macOS-15.7.3-arm64-arm-64bit-Mach-O, Node v24.18.0, rustc 1.99.0 (b940084d7 2026-09-28) (Homebrew).

Built with `cargo build --release --locked`. Each row has three discarded warmups
and 20 measured fresh invocations, alternating compiler and Node. Compiler time
includes process startup, source parsing/analysis and emission through stdout;
the `let` row also invokes the native macro host. Node time includes fresh process
startup, parsing the emitted program, execution, a result assertion and exit.
These are smoke timings, not isolated runtime operation costs or stable ratios.

The original first-party runtime source is 7,335 bytes. Each emitted
collection program contains exactly one runtime factory, while primitive output
contains none and remains the byte-for-byte 104-byte `42` program. `let` and
`let*` emit identical JavaScript for this case.

| Source case | Emitted bytes | Compiler process median ms | Node startup + execution median ms |
| --- | ---: | ---: | ---: |
| `42` | 104 | 2.369 | 50.257 |
| `:demo/x` | 7,474 | 2.370 | 50.403 |
| `[1 2]` | 7,477 | 2.395 | 50.424 |
| `(let* [v [:demo/x 1]] (count v))` | 7,556 | 2.448 | 50.489 |
| `(let [v [:demo/x 1]] (count v))` | 7,556 | 2.578 | 51.248 |

Every measured compile succeeded, emitted identical bytes across samples and had
the expected runtime inclusion count. Every Node process validated its result
and exited successfully without stdout/stderr. The CLI acceptance result was `2`.

`samples.json` retains exact sources, result assertions, all timings, commands,
environment and binary/runtime SHA-256 identities. Sibling `.js` files retain
the exact measured outputs. The compiler tree includes the accumulated
uncommitted implementation; the starting HEAD alone does not identify it.

The future full collection benchmark measures compiler time, output size,
startup and resident operations independently. These observations establish an
initial functioning sample without making that broader claim.
