(ns app.shared
  "Keyword and map helpers. app.main uses `label` for its menu; both features
  read `documents` through the map helpers, which startup does not use.")

(def documents
  [{:id :intro :title "Introduction" :words 120 :tags #{:guide :draft}}
   {:id :setup :title "Setup" :words 340 :tags #{:guide}}
   {:id :faq :title "Questions" :words 80 :tags #{:reference :draft}}
   {:id :api :title "Reference" :words 510 :tags #{:reference}}])

(defn label
  "The display text of a keyword: :reports is \"Reports\"."
  [k]
  (let [text (name k)]
    (str (.toUpperCase (subs text 0 1)) (subs text 1))))

(defn index-by
  "A map from each map's value at `k` to that map."
  [k maps]
  (zipmap (map k maps) maps))

(defn pluck
  "The values at `k` of each map, in order."
  [k maps]
  (mapv k maps))
