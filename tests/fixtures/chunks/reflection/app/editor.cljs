(ns app.editor
  (:require [app.shared :as shared]))

(defn start []
  [(ns-name (find-ns 'app.shared))
   (some? (find-ns 'app.missing))
   (mapv shared/outline [["alpha" "beta" "gamma"] ["delta"]])])
