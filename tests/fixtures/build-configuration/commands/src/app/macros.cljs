(ns app.macros (:require [light.build :as b]))

(defmacro setting [key] (pr-str (b/config key)))
