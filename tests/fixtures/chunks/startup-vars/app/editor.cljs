(ns app.editor
  (:require [app.state :as state]))

(def edits 0)

(defn start []
  (set! state/mode :edit)
  (set! edits (inc edits))
  (binding [state/*depth* edits]
    (with-redefs [state/fixed 2]
      [state/mode (state/depth) (state/fixed-value) edits])))
