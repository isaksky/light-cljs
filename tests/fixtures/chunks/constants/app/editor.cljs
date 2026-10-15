(ns app.editor
  (:require [app.common :as common]))

(def layout {:both-features "editor-map" :editor-only 1})

(defn start []
  (common/show "editor:" [layout {:both-features "shared-map"} :editor-and-settings :all-features]))
