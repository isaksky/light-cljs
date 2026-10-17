(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(def archive "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16")
(doseq [file ["src/analyzer.rs" "src/emitter.rs" "src/compilation_session.rs" "src/evaluator.rs" "src/compiler_bootstrap.rs"]]
  (fs/copy (fs/file "/private/tmp/light-cljs-phase87/project" file) (fs/file project file) {:replace-existing true}))
;; Reuse only detached registration emission, without the rejected bytecode
;; cache or its JavaScript runtime changes. Ordinary evaluate_unit is sufficient.
(p/shell {:dir (str project)} "git" "apply" "--include=src/emitter.rs"
  "--include=src/compiler_bootstrap.rs" "--include=src/compilation_session.rs"
  (str (fs/file archive "penpot-phase88/changes.patch")))
(def patch-inputs (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase97/project"] ["after" "/private/tmp/light-cljs-phase99/project"]]
        file ["src/analyzer.rs" "src/emitter.rs" "src/compilation_session.rs"]]
  (fs/create-dirs (fs/parent (fs/file patch-inputs side file)))
  (fs/copy (fs/file root file) (fs/file patch-inputs side file) {:replace-existing true}))
(def result (p/shell {:dir (str patch-inputs) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after"))
(assert (= 1 (:exit result)))
(def delta (-> (:out result) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/")))
(spit (fs/file here "v8-source-cache.patch") delta)
;; The registration callback is native-QuickJS-specific. Keep the other hunks.
(def portable
  (str/join "" (remove #(and (str/starts-with? % "@@")
                            (str/includes? % "session.analyze(id as usize"))
                  (str/split delta #"(?m)(?=^@@|^diff --git)"))))
(spit (fs/file here "source-cache.patch") portable)
(p/shell {:dir (str project)} "git" "apply" "--check" (str (fs/file here "source-cache.patch")))
(p/shell {:dir (str project)} "git" "apply" (str (fs/file here "source-cache.patch")))
(defn replace-once [file before after]
  (let [path (fs/file project file) input-text (slurp path)]
    (assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote before)) input-text))) file)
    (spit path (str/replace input-text before after))))
(replace-once "src/evaluator.rs"
  "    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError>;"
  "    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError>;\n    /// Registration is kept lazy even when a unit's source is prepared early.\n    fn evaluate_registration(\n        &self,\n        source: &str,\n        _chunks: &[(String, std::ops::Range<usize>)],\n    ) -> Result<(), EvalError> {\n        let value = self.evaluate_unit(&format!(\"rt => {{ {source} return null; }}\"))?;\n        self.release(value)\n    }")
(replace-once "src/compilation_session.rs"
  "                let (program, _) = session.analyze(id as usize, Some(&active))?;\n                let unit = session.catalog.borrow().units[id as usize].clone();\n                unit.borrow_mut().emitted = true;\n                let (registration, unit) = crate::emitter::emit_deferred_unit(&program);"
  "                let unit = session.catalog.borrow().units[id as usize].clone();\n                let prepared = match &unit.borrow().state {\n                    AnalysisState::Prepared(_, _, prepared) => Some(prepared.clone()),\n                    _ => None,\n                };\n                let (registration, code) = if let Some(prepared) = prepared {\n                    prepared.emit()\n                } else {\n                    let (program, _) = session.analyze(id as usize, Some(&active))?;\n                    crate::emitter::emit_deferred_unit(&program)\n                };\n                unit.borrow_mut().emitted = true;")
(replace-once "src/compilation_session.rs" "                Ok(unit)" "                Ok(code)")
(spit (fs/file here "validate.bb")
  (-> (slurp "/private/tmp/light-cljs-phase87/validate.bb")
      (str/replace "QuickJS shared storage" "QuickJS prepared units")
      (str/replace "[1401 0 106]" "[1403 0 106]")
      (str/replace "(p/shell {:dir (str project)} \"cargo\" \"fmt\")"
        "(p/shell \"touch\" (str (fs/file project \"vendor/rquickjs-sys/build.rs\")) (str (fs/file project \"vendor/rquickjs-sys/quickjs/quickjs.c\")))\n(p/shell {:dir (str project)} \"cargo\" \"fmt\")")))
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase87/run.bb")
      (str/replace "light-cljs-quickjs-shared-storage" "light-cljs-quickjs-prepared-units")
      (str/replace "QuickJS shared storage" "QuickJS prepared units")
      (str/replace "(with-open [out (io/output-stream"
        "(p/shell \"touch\" (str (fs/file project \"vendor/rquickjs-sys/build.rs\")) (str (fs/file project \"vendor/rquickjs-sys/quickjs/quickjs.c\")))\n(with-open [out (io/output-stream")))
