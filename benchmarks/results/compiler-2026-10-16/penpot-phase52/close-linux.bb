(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def connection (edn/read-string (slurp (fs/file here "linux-connection.edn"))))
(fs/copy-tree (:logs connection) (fs/file here "linux-readiness") {:replace-existing true})
(def result (p/shell {:continue true :out :string :err :string}
              "ssh" "-S" (:socket connection) "-O" "exit" "oma"))
(spit (fs/file here "linux-close.edn") (pr-str (select-keys result [:exit :out :err])))
(println "Saved readiness logs and closed this task's SSH master." (:exit result))
