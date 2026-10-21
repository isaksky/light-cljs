(ns app.test-runner
  "The test build's entry: it checks app.shared."
  (:require [app.shared :as shared]))

(js/console.log "test:" (= "hello, test" (shared/greeting "test")))
