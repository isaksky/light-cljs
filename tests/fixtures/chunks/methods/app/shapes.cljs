(ns app.shapes
  (:require [app.main :as main]))

(defmethod main/area :square [{:keys [side]}] (* side side))
