(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches" "tests" "examples"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(defn replace! [file before after]
  (let [path (fs/file project "src" file) text (slurp path)]
    (assert (str/includes? text before) (str "Missing anchor " file " " before))
    (spit path (str/replace text before after))))
(fs/copy (fs/file here "timers.rs") (fs/file project "src/perf_phase35.rs") {:replace-existing true})
(replace! "lib.rs" "mod compiler_bootstrap;" "mod compiler_bootstrap;\nmod perf_phase35;")
(replace! "macro_host.rs" "        let run = || -> Result<Form, crate::evaluator::EvalError> {"
  "        let profile_name = format!(\"{}/{}\", String::from_utf16_lossy(origin.macro_name.namespace().unwrap_or(&[])), String::from_utf16_lossy(origin.macro_name.name()));\n        let run = || -> Result<Form, crate::evaluator::EvalError> {")
(doseq [[stage expression] [["form" "evaluator.import_macro_form(call, limits)"]
                            ["environment" "evaluator.import_environment(environment, limits)"]
                            ["arguments" "evaluator.macro_arguments(args[0], call, limits)"]
                            ["call" "evaluator.call(function, &args)"]
                            ["result" "evaluator.export_form(value, call, origin.clone(), limits)"]]]
  (replace! "macro_host.rs" expression
    (str "crate::perf_phase35::measure(&profile_name, \"" stage "\", || " expression ")")))
(replace! "compilation_session.rs"
  "let analysis = crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);"
  "let analysis = crate::perf_phase35::measure(\"compiler\", \"deferred-analysis\", || crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine));")
(replace! "emitter.rs" "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {"
  "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {\n    let _timer = crate::perf_phase35::timer(\"compiler\", \"unit-emission\");")
(println "Prepared isolated macro and deferred-analysis timers")
