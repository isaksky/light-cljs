(ns app.editor
  (:require [app.shared :as shared]))

(deftype Square [side]
  shared/Shape
  (area [_] (* side side)))

(deftype Circle [radius]
  shared/Shape
  (area [_] (* 3 radius radius)))

(defmethod shared/render :para [{:keys [text]}] (str "<p>" text "</p>"))

(defn start []
  (str (shared/fmt "editor" (map shared/area [(->Square 3) (->Circle 1)]))
       "; " (apply str (interpose " | " (map shared/outline [["alpha" "beta" "gamma"] ["delta"]])))
       "; " (shared/render {:kind :para :text "hi"})))
