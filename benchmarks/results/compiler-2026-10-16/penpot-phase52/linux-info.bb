(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def connection (edn/read-string (slurp (fs/file here "linux-connection.edn"))))
(def results
  (mapv (fn [command]
          (let [result (p/shell {:out :string :err :string :continue true}
                         "ssh" "-S" (:socket connection) "-o" "BatchMode=yes"
                         "-o" "StrictHostKeyChecking=yes" "-o" "ConnectTimeout=30" "oma" command)]
            {:command command :exit (:exit result) :out (:out result) :err (:err result)}))
    ["uname -m" "mise exec -- rustc --version" "mise exec -- cargo --version"
     "mise exec -- node --version" "df -h /home/isak" "df -i /home/isak"]))
(spit (fs/file here "linux-info.edn") (pr-str results))
(doseq [result results] (prn result))
