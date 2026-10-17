(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(fs/create-dirs project)
(p/shell "rsync" "-a" "--exclude=target" "--exclude=.git" "/private/tmp/light-cljs-phase54/project/" (str project "/"))
(defn replace-once [text old new]
  (assert (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote old)) text))) old)
  (str/replace text old new))
(def analyzer (fs/file project "src/analyzer.rs"))
(spit analyzer
  (-> (slurp analyzer)
      (replace-once "namespace_globals: (usize, usize)," "namespace_globals: (usize, crate::arena::Arena<GlobalId>),")
      (replace-once "snapshot.map_or((0, 0), |s| s.namespace_globals)" "snapshot.map_or_else(Default::default, |s| s.namespace_globals.clone())")
      (replace-once "namespace_globals: (0, 0)," "namespace_globals: Default::default(),")
      (replace-once "namespace_globals: self.namespace_globals," "namespace_globals: self.namespace_globals.clone(),")
      (replace-once "self.count_namespace_globals(self.program.globals.len());\n        let snapshot"
        "self.count_namespace_globals(self.program.globals.len());\n        self.namespace_globals.1.freeze();\n        let snapshot")
      (replace-once "let count = self.count_namespace_globals(shared);" "self.count_namespace_globals(shared);")
      (replace-once "            shared,\n            count,\n        )?;" "            shared,\n            &self.namespace_globals.1,\n        )?;")
      (replace-once "let (seen, count) = self.namespace_globals;\n        debug_assert!(seen <= shared);\n        let count = count\n            + self\n                .program\n                .globals\n                .range(seen..shared)\n                .filter(|g| g.namespace == self.namespace)\n                .count();\n        self.namespace_globals = (shared, count);\n        count"
        "let (seen, ids) = &mut self.namespace_globals;\n        debug_assert!(*seen <= shared);\n        ids.extend(\n            (*seen..shared)\n                .zip(self.program.globals.range(*seen..shared))\n                .filter_map(|(index, global)| (global.namespace == self.namespace).then_some(GlobalId(index))),\n        );\n        *seen = shared;\n        ids.len()")
      (replace-once "fn reader_bindings(&self) -> Vec<(String, String, String)> {"
        "fn reader_bindings(&mut self) -> Vec<(String, String, String)> {\n        let shared = if self.deferred_unit {\n            self.program.globals.frozen()\n        } else {\n            self.program.globals.len()\n        };\n        self.count_namespace_globals(shared);")
      (replace-once "self.program\n                .globals\n                .iter()\n                .filter(|g| g.namespace == self.namespace)\n                .map(|g| (g.name.clone(), g.namespace.clone(), g.name.clone()))"
        "self.namespace_globals\n                .1\n                .iter()\n                .map(|id| &self.program.globals[id.0])\n                .chain(self.program.globals.range(shared..self.program.globals.len()).filter(|g| g.namespace == self.namespace))\n                .map(|g| (g.name.clone(), g.namespace.clone(), g.name.clone()))")))
(def session (fs/file project "src/compilation_session.rs"))
(spit session
  (-> (slurp session)
      (replace-once "pub namespace_globals: (usize, usize)," "pub namespace_globals: (usize, crate::arena::Arena<crate::analyzer::GlobalId>),")
      (replace-once "namespace_globals: self.namespace_globals," "namespace_globals: self.namespace_globals.clone(),")))
(def host (fs/file project "src/macro_host.rs"))
(spit host
  (-> (slurp host)
      (replace-once "        visible: usize,\n    ) -> Result<(crate::compiler_data::Datum, Option<ReaderRegistration>), String> {"
        "        namespace_globals: &crate::arena::Arena<crate::analyzer::GlobalId>,\n    ) -> Result<(crate::compiler_data::Datum, Option<ReaderRegistration>), String> {\n        let visible = namespace_globals.len();")
      (replace-once "names = globals\n                .range(position..shared)\n                .filter(|g| g.namespace == namespace)\n                .map(|g| g.name.clone())\n                .collect();"
        "names = namespace_globals\n                .range(count..visible)\n                .map(|id| globals[id.0].name.clone())\n                .collect();")
      (replace-once "                       visible| {\n            let (env, registration) = host"
        "                       visible| {\n            let ids = globals.range(0..shared).enumerate()\n                .filter_map(|(index, global)| (global.namespace == \"demo\").then_some(crate::analyzer::GlobalId(index)))\n                .collect::<crate::arena::Arena<_>>();\n            assert_eq!(ids.len(), visible);\n            let (env, registration) = host")
      (replace-once "                    shared,\n                    visible,\n                )"
        "                    shared,\n                    &ids,\n                )")))
(def evaluator (fs/file project "src/eval_compiler.rs"))
(spit evaluator
  (-> (slurp evaluator)
      (replace-once "let visible = globals\n            .iter()\n            .filter(|g| g.namespace == self.namespace)\n            .count();"
        "let namespace_globals = globals\n            .iter()\n            .enumerate()\n            .filter_map(|(index, global)| (global.namespace == self.namespace).then_some(crate::analyzer::GlobalId(index)))\n            .collect();")
      (replace-once "            globals.len(),\n            visible,"
        "            globals.len(),\n            &namespace_globals,")))
(spit analyzer (str/replace (slurp analyzer) "Snapshots preserve this prefix count" "Snapshots preserve these namespace member IDs"))
(spit host (str/replace (slurp host) "`visible` belong to `namespace`: the session registers each of those" "`namespace_globals` identifies the members of `namespace`: the session registers each of those"))
(p/shell {:dir (str project)} "cargo" "fmt")
(fs/copy "/private/tmp/light-cljs-phase50/run-bench.bb" (fs/file here "run-bench.bb") {:replace-existing true})
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase50/run.bb")
      (str/replace "light-cljs-mimalloc" "light-cljs-namespace-ids")
      (str/replace "main-mimalloc" "main-namespace-ids")
      (str/replace "(def project (fs/file here \"project\"))"
        "(def project (fs/file here \"project\"))\n(p/shell \"touch\" (str (fs/file project \"src/lib.rs\")) (str (fs/file project \"src/main.rs\")))")))
(println "Prepared persistent namespace-member IDs trial, not built.")
