(require '[babashka.fs :as fs] '[clojure.string :as s])
(def project (fs/file (fs/parent *file*) "project"))
(defn edit [file f] (let [path (fs/file project file)] (spit path (f (slurp path)))))
(let [path (fs/file project "src/v8_evaluator.rs")
      text (slurp path)
      start (s/index-of text "pub(crate) type UnitService")
      end (s/index-of text "struct State {")]
  (spit (fs/file project "src/compiler_evaluator.rs")
    (str "//! Engine-independent services used by compiler macro sessions.\n"
         "use crate::evaluator::{EvalError, Evaluator, MacroEvaluator, ValueId};\nuse std::rc::Rc;\n\n"
         (subs text start end)))
  (spit path (-> (str (subs text 0 start) (subs text end))
                (s/replace "//! Isolated compiler prototype, not a supported production backend." "//! Native compiler macro sessions with bounded V8 execution.")
                (s/replace "use crate::{" "use crate::{\n    compiler_evaluator::{CompilerEvaluator, UnitService},")
                (s/replace "EvalError, Evaluator, EvaluatorOptions" "EvalError, EvaluatorOptions"))))
(doseq [file ["src/compiler_bootstrap.rs" "src/compilation_session.rs" "src/macro_host.rs"]]
  (edit file #(s/replace % "crate::v8_evaluator::CompilerEvaluator" "crate::compiler_evaluator::CompilerEvaluator")))
(edit "src/lib.rs" #(-> %
  (s/replace "mod v8_bridge;\nmod v8_evaluator;" "#[cfg(light_v8)]\nmod v8_bridge;\n#[cfg(light_v8)]\nmod v8_evaluator;")
  (s/replace "mod compiler_bootstrap;" "mod compiler_bootstrap;\nmod compiler_evaluator;")))
(edit "src/macro_host.rs"
  #(let [start (s/index-of % "        use crate::evaluator::MacroEvaluator;" (s/index-of % "fn new_compiler_evaluator"))
         end (s/index-of % "    /// Allocate a fresh synchronous heap" start)]
     (str (subs % 0 start)
       "        #[cfg(light_v8)]\n        if !self.interactive {\n            use crate::evaluator::MacroEvaluator;\n            let evaluator = crate::v8_evaluator::V8Evaluator::new(self.evaluator_options.clone())?;\n            if self.assertions_disabled {\n                let done = evaluator.evaluate_unit(\"rt => { rt.core.assertions = false; return null; }\")?;\n                evaluator.release(done)?;\n            }\n            if self.opaque_packages {\n                let done = evaluator.evaluate_unit(OPAQUE_PACKAGES_PRELUDE)?;\n                evaluator.release(done)?;\n            }\n            return Ok(std::rc::Rc::new(evaluator));\n        }\n        Ok(std::rc::Rc::new(self.new_evaluator()?))\n    }\n"
       (subs % end))))
(edit "build.rs"
  #(let [start (s/index-of % "    println!(\"cargo::rerun-if-changed=src/v8_stack.cc\");")
         end (s/index-of % "    let out = env::var_os(\"OUT_DIR\")" start)]
     (str (subs % 0 start)
       "    println!(\"cargo::rustc-check-cfg=cfg(light_v8)\");\n    let arch = env::var(\"CARGO_CFG_TARGET_ARCH\").unwrap();\n    let os = env::var(\"CARGO_CFG_TARGET_OS\").unwrap();\n    if env::var_os(\"CARGO_FEATURE_V8_MACROS\").is_some()\n        && matches!(arch.as_str(), \"aarch64\" | \"x86_64\")\n        && matches!(os.as_str(), \"macos\" | \"linux\" | \"windows\")\n    {\n        println!(\"cargo::rustc-cfg=light_v8\");\n"
       (subs % start end) "    }\n" (subs % end))))
(println "Integrated native feature and engine-neutral compiler services")
