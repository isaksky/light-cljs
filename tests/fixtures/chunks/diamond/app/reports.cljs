(ns app.reports
  (:require [app.format :as format]))

(defn start [counts] (format/line "reports" (reduce + counts)))
