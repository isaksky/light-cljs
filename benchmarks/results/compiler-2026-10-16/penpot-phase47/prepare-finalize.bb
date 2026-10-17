(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(spit (fs/file here "finalize.bb")
  (-> (slurp "/private/tmp/light-cljs-phase45/finalize.bb")
      (str/replace "penpot-phase45" "penpot-phase47")
      (str/replace "[(fs/file production \"results.edn\")]"
                   "(map #(fs/file production %) [\"results.edn\" \"implementation.patch\" \"name_index.rs\"])")
      (str/replace "[\"finalize.bb\" \"validate-production.bb\" \"summarize.bb\" \"inclusive-samples.txt\"]"
                   "[\"finalize.bb\" \"prepare-finalize.bb\" \"validate-production.bb\"]")
      (str/replace "[\"src/analyzer.rs\" \"src/compilation_session.rs\"]"
                   "[\"src/analyzer.rs\" \"src/compiler_data.rs\" \"tests/compiler/macros.rs\"]")))
(println "Prepared final production evidence collection.")
