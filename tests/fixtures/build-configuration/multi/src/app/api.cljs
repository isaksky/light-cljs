(ns app.api
  "The :app/api-origin of light-cljs.edn's :config, which :dev overrides."
  (:require [light.build :as b]))

(defmacro origin [] (b/config :app/api-origin))
