(ns app.main
  "Startup of the dev-workflow example: the host calls `start`, the
  program's value. A reload runs `stop` before and `reload` after the new
  code is defined."
  (:require [app.state :as state]
            [app.view :as view]))

(defn ^:dev/before-load stop
  "Runs before a reload defines new code."
  []
  (js/console.log "reloading"))

(defn ^:dev/after-load reload
  "Runs after a reload: one more click, rendered by the new code."
  []
  (state/click!)
  (view/render))

(defn start
  "Logs that the application started and renders the first click."
  []
  (js/console.log "start")
  (state/click!)
  (view/render))

start
