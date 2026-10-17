(require '[babashka.fs :as fs] '[clojure.string :as str])
(def root "/private/tmp/light-cljs-phase44/project")
(defn edit [file before after]
  (let [path (fs/file root file) text (slurp path)]
    (assert (str/includes? text before) file)
    (spit path (str/replace-first text before after))))
(edit "src/compiler_bootstrap.rs" "pub(crate) evaluator: Rc<Evaluator>," "pub(crate) evaluator: Rc<dyn crate::v8_evaluator::CompilerEvaluator>,")
(edit "src/compiler_bootstrap.rs" "evaluator: Rc::new(owner.new_evaluator().map_err(|e| e.to_string())?)," "evaluator: owner.new_compiler_evaluator().map_err(|e| e.to_string())?,")
(edit "src/compilation_session.rs"
  "        let engine = Rc::downgrade(evaluator);\n        evaluator\n            .install_unit_service(\"__lightCompileUnit\", move |ctx, id| {\n                let session = weak.upgrade().ok_or(\"compilation session expired\")?;\n                let evaluator = engine.upgrade().ok_or(\"evaluator expired\")?;\n                let active = evaluator.active(ctx).map_err(|e| e.to_string())?;\n                let (program, _) = session.analyze(id as usize, Some(&active))?;"
  "        evaluator.clone()\n            .install_compiler_service(\"__lightCompileUnit\", Rc::new(move |active, id| {\n                let session = weak.upgrade().ok_or(\"compilation session expired\")?;\n                let (program, _) = session.analyze(id as usize, Some(active))?;")
(edit "src/compilation_session.rs"
  "                Ok(crate::emitter::emit_deferred_unit(&program))\n            })"
  "                Ok(crate::emitter::emit_deferred_unit(&program))\n            }))")
(edit "src/macro_host.rs" "impl MacroHost {\n    /// Allocate"
  "impl MacroHost {
    pub(crate) fn new_compiler_evaluator(&self) -> Result<std::rc::Rc<dyn crate::v8_evaluator::CompilerEvaluator>, crate::evaluator::EvalError> {
        use crate::evaluator::MacroEvaluator;
        if std::env::var_os(\"LIGHT_V8_MACROS\").is_none() || self.interactive {
            return Ok(std::rc::Rc::new(self.new_evaluator()?));
        }
        let evaluator = crate::v8_evaluator::V8Evaluator::new(self.evaluator_options.clone())?;
        if self.assertions_disabled {
            let done = evaluator.evaluate_unit(\"rt => { rt.core.assertions = false; return null; }\")?;
            evaluator.release(done)?;
        }
        if self.opaque_packages {
            let done = evaluator.evaluate_unit(OPAQUE_PACKAGES_PRELUDE)?;
            evaluator.release(done)?;
        }
        Ok(std::rc::Rc::new(evaluator))
    }
    /// Allocate")
(println "wired isolated backend")
