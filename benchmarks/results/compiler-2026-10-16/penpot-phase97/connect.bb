(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(doseq [platform ["linux" "windows"]]
  (let [socket-dir (fs/create-temp-dir {:dir "/private/tmp" :prefix (str "p97-" platform "-")})
        metadata {:socket (str (fs/file socket-dir "s")) :logs (str (fs/file socket-dir "logs"))}]
    (spit (fs/file here (str platform "-connection.edn")) (pr-str metadata))
    ;; Existing skill readiness implementation; task orchestration uses bb.
    (let [result (p/shell {:continue true} "python3"
                   (str "/Users/admin/.codex/skills/" platform "-testing/scripts/connect.py")
                   "--control-path" (:socket metadata) "--log-dir" (:logs metadata))]
      (spit (fs/file here (str platform "-connection-exit.edn")) (pr-str (:exit result)))
      (println platform (:exit result)))))
