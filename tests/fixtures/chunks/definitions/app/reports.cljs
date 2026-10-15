(ns app.reports
  (:require [app.shared :as shared]))

(defmethod shared/render :table [{:keys [rows]}] (str (count rows) " rows"))

(defn start []
  (str (shared/fmt "reports" [(reduce + [1 2 3]) "total"])
       "; " (shared/render {:kind :table :rows [[1] [2]]})))
