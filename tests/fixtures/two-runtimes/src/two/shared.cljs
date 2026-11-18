(ns two.shared
  "What each light-cljs runtime on the page registers in
  `globalThis.twoRuntimes` (tests/support/two_runtimes.mjs): one value of
  each kind it makes, how it reads another runtime's values, and what it
  answers about host values and its own state.")

(defprotocol Greet
  (greet [x]))

(deftype Box [n]
  ICounted
  (-count [_] n))

(defrecord Point [x y])

(defonce loads (atom 0))

(defn- holder
  "A vector of this runtime holding the page's one shared host object."
  []
  [(unchecked-get js/globalThis "twoRuntimesShared")])

(defn- hash-map-of-ten []
  (zipmap (map #(keyword (str "k" %)) (range 10)) (range 10)))

(defn values
  "One value of each kind this runtime makes, by kind."
  []
  #js {:vector [1 2 3]
       :subvec (subvec [0 1 2 3] 1 3)
       :map {:a 1}
       :hashMap (hash-map-of-ten)
       :set #{1 2}
       :list (list 1 2)
       :lazy (map inc [0 1])
       :range (range 1 3)
       :keyword :k
       :symbol 'sym
       :entry (first {:a 1})
       :uuid (uuid "00000000-0000-0000-0000-000000000001")
       :record (->Point 1 2)
       :box (Box. 7)
       :atom (atom 5)
       :greeter (reify Greet (greet [_] "hi"))
       :exInfo (ex-info "boom" {:a 1} (ex-info "root" {:b 2}))
       :holder (holder)})

(defn- sum-to-2
  "The sum of the items before the first 2, a reduction stopped with this
  runtime's `reduced`."
  [a x]
  (if (= x 2) (reduced a) (+ a x)))

(defn- attempt [f]
  (try (pr-str (f)) (catch :default _ "throws")))

(def ^:private kinds
  "Each kind: its predicate, an operation on it, and the value of this
  runtime it should equal (nil: none, for a kind compared by identity)."
  [["vector" vector? #(vector (nth % 1) (reduce sum-to-2 0 %)) [1 2 3]]
   ["subvec" vector? #(nth % 0) [1 2]]
   ["map" map? #(get % :a) {:a 1}]
   ["hashMap" map? #(vector (get % :k5) (reduce-kv (fn [a _ v] (sum-to-2 a v)) 0 (into (sorted-map) %))) (hash-map-of-ten)]
   ["set" set? #(contains? % 2) #{1 2}]
   ["list" list? first '(1 2)]
   ["lazy" seq? vec '(1 2)]
   ["range" seq? #(vector (reduce + %) (reduce sum-to-2 0 %)) '(1 2)]
   ["keyword" keyword? name :k]
   ["symbol" symbol? name 'sym]
   ["entry" map-entry? key [:a 1]]
   ["uuid" uuid? str (uuid "00000000-0000-0000-0000-000000000001")]
   ["record" record? :x (->Point 1 2)]
   ["box" #(satisfies? ICounted %) count nil]
   ["atom" #(satisfies? IDeref %) deref nil]
   ["greeter" #(satisfies? Greet %) greet nil]
   ["exInfo" #(instance? ExceptionInfo %)
    #(vector (ex-message %) (ex-data %) (ex-data (ex-cause %)) (mapv :type (:via (Throwable->map %)))) nil]
   ;; A map of this runtime keyed by the other's vector, looked up with its
   ;; own: the host object's hash must be the same in both.
   ["holder" vector? #(get (hash-map % :found) (holder)) (holder)]])

(defn observe
  "How this runtime reads `their`, another runtime's `values` (or its own):
  per kind, the predicate, the operation, whether it equals this runtime's
  value of the kind with the same hash, and how it prints (`same` as this
  runtime's value of the kind, `object` as a host object)."
  [their]
  (->> kinds
       (map (fn [[kind pred op own]]
              (let [v (unchecked-get their kind)
                    printed (attempt #(pr-str v))
                    mine (attempt #(pr-str (unchecked-get (values) kind)))]
                (str kind ":" (attempt #(pred v)) "," (attempt #(op v)) ","
                     (if (nil? own)
                       "-"
                       (attempt #(and (= own v) (= (hash own) (hash v)))))
                     ","
                     (cond (= printed mine) "same"
                           (.startsWith printed "\"#object[") "object"
                           :else printed)))))
       (interpose " ")
       (apply str)))

(defn host
  "What this runtime answers about host values and its own state."
  []
  (str "date-count=" (attempt #(count (js/Date. 0)))
       " date-greet=" (attempt #(greet (js/Date. 0)))
       " date-counted=" (satisfies? ICounted (js/Date. 0))
       " loads=" @loads
       " box=" (count (Box. 7))))

(defn load-option
  "The page's option `name` for the runtime loading now."
  [name]
  (some-> (unchecked-get js/globalThis "twoRuntimesLoad") (unchecked-get name)))

(defn register!
  "Count this load and register this runtime on the page under the label
  the page gave the load."
  []
  (swap! loads inc)
  (let [registry (or (unchecked-get js/globalThis "twoRuntimes")
                     (let [registry #js []]
                       (unchecked-set js/globalThis "twoRuntimes" registry)
                       registry))]
    (.push registry #js {:label (load-option "label")
                         :values (values)
                         :observe observe
                         :host host})
    registry))
