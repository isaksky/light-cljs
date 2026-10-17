(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def results
  (for [[label old] [["render-v12-256m-repeat" "render"]
                     ["main-v12-256m" "main-none"]
                     ["advanced-v12-256m" "main-advanced"]]
        :let [file (fs/file here (str label ".js"))]]
    (do
      (p/shell "cmp" (str file) (str "/private/tmp/light-cljs-phase42/production/" old ".js"))
      (let [checked (with-open [input (io/input-stream file)]
                      (p/shell {:in input :out :string :err :string :continue true}
                        "node" "--check" "--input-type=module"))]
        (spit (fs/file here (str label "-syntax.txt"))
          (str "Byte-identical to phase42 " old ".\n"
               "node --check --input-type=module < " file "\n"
               "Exit: " (:exit checked) "\n" (:out checked) (:err checked)))
        {:job label :byte-identical? true :module-syntax-exit (:exit checked)}))))
(prn (vec results))
