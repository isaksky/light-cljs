(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def text "(defmacro shares [x y] (list 'quote [(identical? (second &form) x) (identical? (nth &form 2) y)])) (pr-str [(shares [1] #js [2]) (shares unresolved-name #\"x\")])")
(spit (fs/file here "argument-sharing.cljs") text)
(def result (p/shell {:out :string :err :string}
                    (str (fs/file here "light-cljs")) "--compile"
                    (str (fs/file here "argument-sharing.cljs"))))
(spit (fs/file here "argument-sharing.js") (str "console.log(" (:out result) ");"))
(def execution (p/shell {:out :string :err :string} "node" (str (fs/file here "argument-sharing.js"))))
(spit (fs/file here "argument-sharing.log") (:out execution))
(assert (= "[[true true] [true true]]\n" (:out execution)))
(println "Macro arguments share their matching &form elements:" (:out execution))
