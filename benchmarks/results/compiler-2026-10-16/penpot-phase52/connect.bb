(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def socket-dir (fs/create-temp-dir {:dir "/private/tmp" :prefix "lt-p52-"}))
(def metadata {:socket (str (fs/file socket-dir "s")) :logs (str (fs/file socket-dir "logs"))})
(spit (fs/file here "linux-connection.edn") (pr-str metadata))
;; Use the skill's existing readiness helper; new task orchestration remains bb.
(p/shell "python3" "/Users/admin/.codex/skills/linux-testing/scripts/connect.py"
  "--control-path" (:socket metadata) "--log-dir" (:logs metadata))
(prn metadata)
