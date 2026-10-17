(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(fs/create-dirs project)
(p/shell "rsync" "-a" "--exclude=target" "--exclude=.git" "/private/tmp/light-cljs-phase50/project/" (str project "/"))
(def file (fs/file project "src/analyzer.rs"))
(def before (slurp file))
(def after
  (-> before
      (str/replace "for index in added.clone() {\n            let global = &self.program.globals[index];"
        "for global in self.program.globals.range(added.clone()) {")
      (str/replace "for index in pending.into_iter().chain(added).chain(changed) {\n            let global = &self.program.globals[index];"
        "let existing = |index| (index, &self.program.globals[index]);\n        for (index, global) in pending\n            .into_iter()\n            .map(existing)\n            .chain(added.clone().zip(self.program.globals.range(added)))\n            .chain(changed.into_iter().map(existing))\n        {")))
(assert (not= before after))
(spit file after)
(p/shell {:dir (str project)} "cargo" "fmt")
(doseq [filename ["run-bench.bb"]]
  (fs/copy (fs/file "/private/tmp/light-cljs-phase50" filename) (fs/file here filename) {:replace-existing true}))
(spit (fs/file here "run.bb")
  (-> (slurp "/private/tmp/light-cljs-phase50/run.bb")
      (str/replace "light-cljs-mimalloc" "light-cljs-ranges")
      (str/replace "main-mimalloc" "main-ranges")))
(println "Prepared isolated reader range iteration trial; not built.")
