(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def repo "/Users/admin/src/light-cljs")
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase43"))
(fs/create-dirs output)
(doseq [file (concat ["archive.bb" "prepare-compiler.bb"]
                    (map str (map fs/file-name (fs/glob here "*.json"))))]
  (fs/copy (fs/file here file) (fs/file output file) {:replace-existing true}))
(doseq [pattern ["*.time" "*.log"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(fs/create-dirs (fs/file output "v8-probe/src/bin"))
(doseq [file ["Cargo.toml" "Cargo.lock" "src/main.rs" "src/bin/contracts.rs" "build.log" "contracts-build.log" "replay-build.log"]]
  (fs/copy (fs/file here "v8-probe" file) (fs/file output "v8-probe" file) {:replace-existing true}))
(defn sha [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (.update digest (fs/read-all-bytes path))
    (apply str (map #(format "%02x" (bit-and % 255)) (.digest digest)))))
(spit (fs/file output "source-hashes.txt")
  (apply str (for [path ["/private/tmp/light-cljs-phase31/capture.json"
                        "/private/tmp/light-cljs-phase31/replay.js"
                        "/private/tmp/light-cljs-phase38/runtime.js"
                        "/private/tmp/light-cljs-phase38/probe-before"
                        (str (fs/file here "v8-probe/src/main.rs"))
                        (str (fs/file here "v8-probe/src/bin/contracts.rs"))
                        (str (fs/file here "v8-probe/Cargo.lock"))
                        (str (fs/file here "v8-probe/target/release/light-v8-probe"))
                        (str (fs/file here "v8-probe/target/release/contracts"))]]
               (str (sha path) "  " path "\n"))))
(spit (fs/file output "README.md")
"# Embedded V8 feasibility probe

This standalone Rust executable embeds pinned `v8 = 152.2.0`, using the
same phase31 captured Instaparse grammar invocation and phase38/current
runtime as the QuickJS replay. It has no Node runtime dependency. The
production compiler and its dependency manifest are unchanged.

Every trial below reproduces the captured encoded result exactly after the
capture's documented provenance removal. This is one macro invocation,
not full engine conformance or a prediction for a whole build. The replay
uses already emitted sources, excluding Rust compilation callbacks.

| Fresh process | First call ms | Second call ms | Peak RSS bytes |
| --- | ---: | ---: | ---: |
| QuickJS, current retained patches | 9253 | 9276 | 182435840 |
| V8, 4,000,000,000-byte heap limit, run 1 | 1149 | 1081 | 244744192 |
| V8, same, run 2 | 1119 | 1069 | see v8-2.time |
| V8, 64 MiB heap limit | 1463 | 1386 | 150159360 |
| V8, 128 MiB heap limit | 1146 | 1089 | 149536768 |
| V8, JIT disabled, 4,000,000,000-byte heap limit | 5148 | 5154 | 224706560 |

The first two V8 runs preceded the runner's optional environment-variable
switches; they used its unchanged default heap parameters. The archived
runner reproduces those defaults and the subsequent heap/JIT experiments.
All jobs ran separately, after builds completed.

`contracts.rs` verifies direct UTF-16 including isolated surrogates and
NUL, negative zero and NaN, opaque descriptor identity, native-to-JS
compiler callback reentry, explicit promise-job checkpoints, and GC-owned
Rust provenance. Retained inputs keep their payload; unreachable inputs
release it; remaining payloads are released at isolate destruction. All
payload destructors ran on the owner thread with atomic cppgc marking and
sweeping. It also verifies uncatchable external termination, termination
from a near-heap-limit callback, and a bounded custom ArrayBuffer allocator
whose refusal remains observable after JavaScript catches the exception.

These are boundary probes, not the full evaluator contract. The heap
callback reserves 16 MiB emergency headroom to let termination unwind;
`heap_limits` does not establish a strict process-RSS cap. Whole-compiler
integration still needs stack-limit behavior, sticky resource diagnostics,
complete bridge regressions, and the full suite. No backend replacement is
promoted on the strength of these probes. See phase44 for the isolated
compiler experiment that followed.

Reproduce by building `v8-probe` with `cargo build --release`. Its replay
binary takes runtime-source and replay-source paths (fingerprinted here).
Set `V8_PROBE_HEAP_BYTES` for a heap limit or `V8_PROBE_FLAGS=--jitless` for
the interpreter control. Run `contracts RUNTIME`, `contracts heap`, and
`contracts buffers` separately. The large capture, runtime and executables
remain in the temporary directories named in `source-hashes.txt`.

Primary API references: [rusty_v8](https://github.com/denoland/rusty_v8),
[CreateParams](https://docs.rs/v8/152.2.0/v8/struct.CreateParams.html), and
[cppgc](https://docs.rs/v8/152.2.0/v8/cppgc/index.html).
")
(println "Archived embedded-engine probe")
