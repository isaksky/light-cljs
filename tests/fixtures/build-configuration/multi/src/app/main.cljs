(ns app.main
  "The page: it greets, and in a browser starts the worker build, served
  under /worker/ by the same server, so the Worker is same-origin."
  (:require [app.api :refer [origin]]
            [app.build-info :as info]
            [app.config :as config]
            [app.shared :as shared]))

(js/console.log "app:" (shared/greeting "page") (origin) config/VERSION info/built-by)

(when (exists? js/document)
  (js/Worker. "/worker/index.js" #js {:type "module"}))
