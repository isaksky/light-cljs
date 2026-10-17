(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "profile-project"))
(defn edit! [file before after]
  (let [path (fs/file project "src" file) text (slurp path)]
    (assert (str/includes? text before) file)
    (spit path (str/replace-first text before after))))
(edit! "compiler.rs"
  "    loader.visit(entry)?;"
  "    crate::perf_phase35::measure(\"compiler\", \"initial-load\", || loader.visit(entry))?;")
(edit! "compiler.rs"
  "    while index < loader.ordered.len() {"
  "    while index < loader.ordered.len() {\n        let _namespace_timer = crate::perf_phase35::timer(\"compiler\", \"namespace-analysis\");")
(edit! "compiler.rs"
  "    let program = analysis\n        .finish(entry_index, macros)"
  "    let _finish_timer = crate::perf_phase35::timer(\"compiler\", \"analysis-finish\");\n    let program = analysis\n        .finish(entry_index, macros)")
(edit! "main.rs"
  "    let program = light_cljs::compiler::analyze_namespace_entry_with_sources("
  "    let profile_start = std::time::Instant::now();\n    let program = light_cljs::compiler::analyze_namespace_entry_with_sources(")
(edit! "main.rs"
  "    let (program, files) = program?;"
  "    eprintln!(\"CLI-PROFILE analysis-and-host-drop {}\", profile_start.elapsed().as_secs_f64());\n    let (program, files) = program?;")
(edit! "main.rs"
  "    let prepared = prepare_application(&program, flags.options(&program)?);"
  "    let profile_start = std::time::Instant::now();\n    let prepared = prepare_application(&program, flags.options(&program)?);\n    eprintln!(\"CLI-PROFILE preparation {}\", profile_start.elapsed().as_secs_f64());")
(edit! "main.rs"
  "        let output = flags.output(&prepared, esm, &sources)?;"
  "        let profile_start = std::time::Instant::now();\n        let output = flags.output(&prepared, esm, &sources)?;\n        eprintln!(\"CLI-PROFILE output {}\", profile_start.elapsed().as_secs_f64());")
(println "Prepared namespace and CLI timing boundaries")
