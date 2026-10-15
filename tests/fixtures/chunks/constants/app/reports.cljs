(ns app.reports
  (:require [app.common :as common]))

(defn start []
  (common/show "reports:" [{:both-features "shared-map"} :both-features :reports-only :all-features]))
