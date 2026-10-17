(require '[babashka.fs :as fs])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches" "tests" "examples"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(fs/copy "/private/tmp/light-cljs-phase11/probe.rs" (fs/file project "examples/replay_probe.rs") {:replace-existing true})
(fs/copy "src/js_runtime.js" (fs/file here "runtime.js") {:replace-existing true})
(println "Prepared clean isolated sources for native PGO experiment")
