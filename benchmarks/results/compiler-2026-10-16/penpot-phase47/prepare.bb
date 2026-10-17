(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(fs/create-dirs project)
(p/shell "rsync" "-a" "--exclude=target" "--exclude=.git"
  "/private/tmp/light-cljs-phase46/project/" (str project "/"))
(def path (fs/file project "src/analyzer.rs"))
(defn replace-once [text before after]
  (assert (= 2 (count (str/split text (re-pattern (java.util.regex.Pattern/quote before)) -1))) before)
  (str/replace text before after))
(spit path
  (-> (slurp path)
    (replace-once "    /// The globals the view covers.\n    globals: usize,"
      "    /// Only these namespaces can override qualification in this private view.\n    binding_namespaces: std::collections::HashSet<String>,\n    /// The globals the view covers.\n    globals: usize,")
    (replace-once "            self.reader_view = Some(ReaderView {\n                view,"
      "            // The compiler reader only asks this private view to qualify names.\n            // A qualified binding outside aliases, refers and the current namespace\n            // would return its input unchanged, just like ordinary qualification.\n            // Keep every namespace that can override that result; the public\n            // NamespaceView binding-introspection surface is unchanged.\n            let binding_namespaces = globals\n                .iter()\n                .map(|(_, namespace, _)| namespace.clone())\n                .chain(aliases.iter().map(|(alias, _)| alias.clone()))\n                .chain(std::iter::once(self.namespace.to_owned()))\n                .collect();\n            self.reader_view = Some(ReaderView {\n                view,\n                binding_namespaces,")
    (replace-once "            let qualified = Name::new(Some(&global.namespace), &global.name);\n            let binding = ResolvedBinding {"
      "            cache.view.declaration_frontier = cache.view.declaration_frontier.max(index + 1);\n            if !cache.binding_namespaces.contains(&global.namespace) {\n                continue;\n            }\n            let qualified = Name::new(Some(&global.namespace), &global.name);\n            let binding = ResolvedBinding {")
    (replace-once "            cache.view.declaration_frontier = cache.view.declaration_frontier.max(index + 1);\n        }\n    }\n    fn expand_reader"
      "        }\n    }\n    fn expand_reader")))
(fs/copy "/private/tmp/light-cljs-phase46/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(p/shell {:dir (str project)} "cargo" "fmt")
(println "Prepared relevant-namespace binding probe; not built yet.")
