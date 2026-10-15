(ns app.editor
  (:require [app.format :as format]))

(defn start [document] (format/line "editor" document))
