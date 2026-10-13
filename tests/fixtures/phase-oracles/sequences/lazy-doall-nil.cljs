(= '[[true true [true]] ""] (let [api-observed (atom nil) api-stdout (with-out-str (reset! api-observed (let [api-same? (fn api-same? [expected actual]
  (cond
    (vector? expected) (and (vector? actual) (= (count expected) (count actual))
                           (every? true? (map api-same? expected actual)))
    (seq? expected) (and (sequential? actual) (not (vector? actual))
                        (= (count expected) (count actual))
                        (every? true? (map api-same? expected actual)))
    (map? expected) (and (map? actual) (= (count expected) (count actual))
                        (every? (fn [[k v]]
                          (boolean (some (fn [[a b]]
                            (and (api-same? k a) (api-same? v b))) actual))) expected))
    (set? expected) (and (set? actual) (= (count expected) (count actual))
                        (every? (fn [x] (boolean (some (fn [y] (api-same? x y)) actual))) expected))
    (nil? expected) (nil? actual)
    (boolean? expected) (and (boolean? actual) (= expected actual))
    (string? expected) (and (string? actual) (= expected actual))
    (keyword? expected) (and (keyword? actual) (= expected actual))
    (symbol? expected) (and (symbol? actual) (= expected actual))
    (number? expected) (and (number? actual) (= expected actual)
                           (not (js* "Object.is(~{}, -0)" actual)))
    :else false)) api-effects (atom []) observe (fn [x] (swap! api-effects conj x) x) api-result (do [(doall nil) (dorun nil) (doall 0 nil)])] [(api-same? '[nil nil nil] api-result) (api-same? '[] @api-effects) [(identical? clojure.core/doall doall)]])))] [@api-observed api-stdout]))