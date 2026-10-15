(ns app.reports
  "The reports feature: sets, `reduce` and `clojure.string`."
  (:require [app.shared :as shared]
            [clojure.string :as string]))

(defn tagged
  "The ids of the documents carrying `tag`, as a set."
  [documents tag]
  (set (map :id (filter #(contains? (:tags %) tag) documents))))

(defn start
  "Logs one line summarizing tags, drafts and words."
  []
  (let [tags (reduce into #{} (map :tags shared/documents))
        drafts (tagged shared/documents :draft)
        words (reduce + (shared/pluck :words shared/documents))]
    (js/console.log
     (str "reports: tags " (string/join ", " (sort (map shared/label tags)))
          "; drafts " (string/join ", " (map (comp string/upper-case name)
                                             (sort drafts)))
          "; " words " words"))))
