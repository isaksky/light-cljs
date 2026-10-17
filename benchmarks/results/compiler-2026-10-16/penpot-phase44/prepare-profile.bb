(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def baseline (fs/file here "project"))
(def project (fs/file here "profile-project"))
(fs/create-dirs project)
(doseq [file ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy (fs/file baseline file) (fs/file project file) {:replace-existing true}))
(doseq [dir ["src" "benches"]]
  (fs/copy-tree (fs/file baseline dir) (fs/file project dir) {:replace-existing true}))
(when-not (fs/exists? (fs/file project "vendor"))
  (fs/create-sym-link (fs/file project "vendor") (fs/file baseline "vendor")))
(defn edit! [file before after]
  (let [path (fs/file project "src" file) text (slurp path)]
    (assert (str/includes? text before) file)
    (spit path (str/replace text before after))))
(fs/copy (fs/file here "timers.rs") (fs/file project "src/perf_phase35.rs") {:replace-existing true})
(edit! "lib.rs" "mod compiler_bootstrap;" "mod compiler_bootstrap;\nmod perf_phase35;")
(edit! "macro_host.rs" "        let run = || -> Result<Form, crate::evaluator::EvalError> {"
  "        let profile_name = format!(\"{}/{}\", String::from_utf16_lossy(origin.macro_name.namespace().unwrap_or(&[])), String::from_utf16_lossy(origin.macro_name.name()));\n        let run = || -> Result<Form, crate::evaluator::EvalError> {")
(doseq [[stage expression] [["form" "evaluator.import_macro_form(call, limits)"]
                            ["environment" "evaluator.import_environment(environment, limits)"]
                            ["arguments" "evaluator.macro_arguments(args[0], call, limits)"]
                            ["call" "evaluator.call(function, &args)"]
                            ["result" "evaluator.export_form(value, call, origin.clone(), limits)"]]]
  (edit! "macro_host.rs" expression
    (str "crate::perf_phase35::measure(&profile_name, \"" stage "\", || " expression ")")))
(edit! "compilation_session.rs"
  "let analysis = crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);"
  "let analysis = crate::perf_phase35::measure(\"compiler\", \"deferred-analysis\", || crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine));")
(edit! "emitter.rs" "    registration: Option<&mut String>,\n) -> String {"
  "    registration: Option<&mut String>,\n) -> String {\n    let _timer = crate::perf_phase35::timer(\"compiler\", \"unit-emission\");")
(println "Prepared isolated V8/QuickJS stage timers")
