(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(doseq [[platform host] [["linux" "oma"] ["windows" "windows-test-vm"]]]
  (let [connection (edn/read-string (slurp (fs/file here (str platform "-connection.edn"))))
        result (p/shell {:out :string :err :string :continue true}
                 "ssh" "-S" (:socket connection) "-O" "exit" host)]
    (spit (fs/file here (str platform "-close.edn")) (pr-str (select-keys result [:exit :out :err])))))
