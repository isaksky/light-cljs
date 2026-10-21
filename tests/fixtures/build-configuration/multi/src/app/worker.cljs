(ns app.worker
  "The web worker build's entry."
  (:require [app.jobs :as jobs]
            [app.shared :as shared]))

(js/console.log "worker:" (shared/greeting "worker") (jobs/run 2))
