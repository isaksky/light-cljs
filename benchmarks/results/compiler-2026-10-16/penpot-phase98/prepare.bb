(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(defn replace-once [file old new]
  (let [path (fs/file project file) text (slurp path)]
    (assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote old)) text))) file)
    (spit path (str/replace text old new))))
(doseq [file ["src/lib.rs" "src/main.rs" "src/compilation_session.rs"]]
  (fs/copy (fs/file "/private/tmp/light-cljs-phase97/project" file) (fs/file project file) {:replace-existing true}))
(fs/copy "/private/tmp/light-cljs-phase81/project/src/allocation_probe.rs" (fs/file project "src/allocation_probe.rs") {:replace-existing true})
(replace-once "src/lib.rs" "pub mod analyzer;" "pub mod allocation_probe;\npub mod analyzer;")
(replace-once "src/main.rs" "static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;"
  "static ALLOCATOR: light_cljs::allocation_probe::Allocator = light_cljs::allocation_probe::Allocator;")
(replace-once "src/main.rs" "    let program = light_cljs::compiler::analyze_namespace_entry_with_sources("
  "    let mut host = flags.host();\n    let program = light_cljs::compiler::analyze_namespace_entry_with_sources(")
(replace-once "src/main.rs" "        &mut flags.host(),\n        options,\n        &mut warnings,"
  "        &mut host,\n        options,\n        &mut warnings,")
(replace-once "src/main.rs" "    let (program, files) = program?;"
  "    light_cljs::allocation_probe::report(\"after-analysis\");\n    drop(host);\n    light_cljs::allocation_probe::report(\"after-session-drop\");\n    let (program, files) = program?;")
(replace-once "src/compilation_session.rs" "impl CompilationSession {"
  (str (slurp (fs/file here "drop-probe.rs")) "\nimpl CompilationSession {"))
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase97/run.bb")
      (str/replace "light-cljs-v8-stack-limits" "light-cljs-retained-program-probe")))
