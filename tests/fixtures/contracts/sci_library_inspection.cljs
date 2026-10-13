;; Test-only shallow bridge through compiler-owned public language predicates.
;; The JavaScript observation adapter retains the recursive raw protocol.
(set! (.-lightLibraryInspect js/globalThis)
  (fn [value]
    (cond
      (reduced? value) (js-obj "kind" "opaque")
      (keyword? value) (js-obj "kind" "keyword" "namespace" (namespace value)
                            "name" (name value) "spelling" (subs (str value) 1))
      (symbol? value) (js-obj "kind" "symbol" "namespace" (namespace value)
                           "name" (name value) "spelling" (str value))
      (set? value)
      (let [items (array)]
        (doseq [member value] (.push items member))
        (js-obj "kind" "set" "items" items "count" (count value)))
      (map? value)
      (let [entries (array)]
        (doseq [[k v] value] (.push entries (array k v)))
        (js-obj "kind" "map" "entries" entries "count" (count value)))
      (or (vector? value) (seq? value) (list? value))
      (let [items (array) kind (cond (vector? value) "vector" (list? value) "list" :else "seq")]
        (loop [s (seq value)] (when s (.push items (first s)) (recur (next s))))
        (js-obj "kind" kind "items" items))
      :else nil)))
