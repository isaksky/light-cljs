(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def file (fs/file here "project/src/analyzer.rs"))
(def original-text (slurp file))
(defn replace-region [s start end text]
  (let [a (str/index-of s start) b (when a (str/index-of s end a))]
    (assert (and a b) [start end])
    (str (subs s 0 a) text (subs s b))))
(def code (-> original-text
  (replace-region "    /// Only these namespaces can override qualification" "    /// A hash of the inputs" "    /// The globals whose namespace-local refers are already installed.\n    globals: usize,\n")
  (str/replace "        self.macro_host\n            .lower_form_in_view(source, &self.syntax, id, view, &self.discards)"
    "        // Only the spelling of a binding affects syntax quotation. Resolve\n        // names that are actually read against this analyzer's current snapshot.\n        let bindings = |name: &crate::compiler_data::Name| {\n            let target = if name.namespace().is_some() { name } else { view.refers.get(name)? };\n            let namespace = String::from_utf16(target.namespace()?).ok()?;\n            let member = String::from_utf16(target.name()).ok()?;\n            let id = self.globals.get(&(namespace, member))?;\n            self.program.globals[id.0].declared.then(|| target.clone())\n        };\n        self.macro_host.lower_form_in_view(source, &self.syntax, id, view, &self.discards, Some(&bindings))")
  (str/replace "        use crate::compiler_reader::{BindingId, BindingKind, ResolvedBinding};\n" "")
  (replace-region "            // By target, the locals referring to it" "            view.excludes" "")
  (replace-region "            // The compiler reader only asks this private view" "    fn expand_reader(" "            self.reader_view = Some(ReaderView {\n                view,\n                globals: self.program.globals.len(),\n                inputs,\n            });\n        }\n        let cache = self.reader_view.as_mut().unwrap();\n        // A newly introduced local name shadows a refer even before its value\n        // is declared. Declared binding precedence is resolved on demand above.\n        let added = cache.globals..self.program.globals.len();\n        cache.globals = self.program.globals.len();\n        for global in self.program.globals.range(added) {\n            if global.namespace == self.namespace {\n                cache.view.refers.insert(\n                    Name::new(None, &global.name),\n                    Name::new(Some(&global.namespace), &global.name),\n                );\n            }\n        }\n    }\n")))
(assert (not= original-text code))
(spit file code)
(spit (fs/file here "run.bb") (-> (slurp "/private/tmp/light-cljs-phase71/run.bb") (str/replace "light-cljs-direct-roots" "light-cljs-reader-lookup")))
