(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def fixture "(defmacro ignored {:probe #\"[\"} [] nil) (when true 42)")
(spit (fs/file here "literal.cljs") fixture)
(doseq [[name binary expected] [["phase25" "/private/tmp/light-cljs-phase25/production/light-cljs" 1]
                                ["unguarded" "/private/tmp/light-cljs-phase27/light-cljs" 0]]]
  (let [result @(p/process [binary "--compile" (str (fs/file here "literal.cljs"))]
                          {:out :string :err :string})]
    (spit (fs/file here (str "literal-" name ".log")) (str "exit " (:exit result) "\n" (:err result)))
    (println name (:exit result) (str/trim (:err result)))
    (assert (= expected (:exit result)))))
