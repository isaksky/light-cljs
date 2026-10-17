(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def source-text "(defmacro renamed [value] (set! (.-name value) \"changed\") value) (let [original 1 changed 2] (renamed original))")
(spit (fs/file here "symbol-snapshot.cljs") source-text)
(def result (p/shell {:out :string :err :string}
                    (str (fs/file here "production/light-cljs")) "--compile"
                    (str (fs/file here "symbol-snapshot.cljs"))))
(spit (fs/file here "symbol-snapshot.js") (str "console.log(" (:out result) ");"))
(def execution (p/shell {:out :string :err :string} "node" (str (fs/file here "symbol-snapshot.js"))))
(assert (= "2\n" (:out execution)))
(spit (fs/file here "symbol-snapshot.log") (:out execution))
(println "Imported symbol mutation is preserved:" (:out execution))
