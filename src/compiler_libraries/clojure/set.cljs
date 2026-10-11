;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

(ns ^{:doc "Set operations such as union/intersection."
       :author "Rich Hickey"}
       clojure.set)

(declare union intersection difference select project rename-keys rename index map-invert join subset? superset?)

(defn- require-set [x]
  (if (or (nil? x) (set? x)) x
    (throw (js/Error. "expected set or nil"))))
(defn- require-map [x]
  (if (or (nil? x) (map? x)) x
    (throw (js/Error. "expected map or nil"))))
(defn- selected-keys [m ks]
  (require-map m)
  (reduce (fn [result k] (if-let [entry (find m k)] (assoc result k (val entry)) result))
    (with-meta {} (meta m)) ks))

(defn- bubble-max-key [k coll]
  "Move a maximal element of coll according to fn k (which returns a number)
   to the front of coll."
  (let [max (apply max-key k coll)]
    (cons max (remove #(identical? max %) coll))))

(defn- union-impl
  "Return a set that is the union of the input sets"
  ([] #{})
  ([s1] (require-set s1))
  ([s1 s2]
     (require-set s1) (require-set s2)
     (if (< (count s1) (count s2))
       (reduce conj s2 s1)
       (reduce conj s1 s2)))
  ([s1 s2 & sets]
     (doseq [s (list* s1 s2 sets)] (require-set s))
     (let [bubbled-sets (bubble-max-key count (conj sets s2 s1))]
       (reduce into (first bubbled-sets) (rest bubbled-sets)))))

(defn- intersection-impl
  "Return a set that is the intersection of the input sets"
  ([] (throw (js/Error. "Invalid arity: 0")))
  ([s1] (require-set s1))
  ([s1 s2]
     (require-set s1) (require-set s2)
     (if (< (count s2) (count s1))
       (recur s2 s1)
       (reduce (fn [result item]
                   (if (contains? s2 item)
		     result
                     (disj result item)))
	       s1 s1)))
  ([s1 s2 & sets]
     (doseq [s (list* s1 s2 sets)] (require-set s))
     (let [bubbled-sets (bubble-max-key #(- (count %)) (conj sets s2 s1))]
       (reduce intersection (first bubbled-sets) (rest bubbled-sets)))))

(defn- difference-impl
  "Return a set that is the first set without elements of the remaining sets"
  ([] (throw (js/Error. "Invalid arity: 0")))
  ([s1] (require-set s1))
  ([s1 s2]
     (require-set s1) (require-set s2)
     (if (< (count s1) (count s2))
       (reduce (fn [result item]
                   (if (contains? s2 item)
                     (disj result item)
                     result))
               s1 s1)
       (reduce disj s1 s2)))
  ([s1 s2 & sets]
     (doseq [s (list* s1 s2 sets)] (require-set s))
     (reduce difference s1 (conj sets s2))))


(defn- select-impl
  "Returns a set of the elements for which pred is true"
  [pred xset]
    (require-set xset)
    (reduce (fn [s k] (if (pred k) s (disj s k)))
            xset xset))

(defn- project-impl
  "Returns a rel of the elements of xrel with only the keys in ks"
  [xrel ks]
    (set (map #(selected-keys % ks) xrel)))

(defn- rename-keys-impl
  "Returns the map with the keys in kmap renamed to the vals in kmap"
  [map kmap]
    (require-map map) (require-map kmap)
    (reduce
     (fn [m [old new]]
       (if (contains? map old)
         (assoc m new (get map old))
         m))
     (apply dissoc map (keys kmap)) kmap))

(defn- rename-impl
  "Returns a rel of the maps in xrel with the keys in kmap renamed to the vals in kmap"
  [xrel kmap]
    (set (map #(rename-keys % kmap) xrel)))

(defn- index-impl
  "Returns a map of the distinct values of ks in the xrel mapped to a
  set of the maps in xrel with the corresponding values of ks."
  [xrel ks]
    (reduce
     (fn [m x]
       (let [ik (selected-keys x ks)]
         (assoc m ik (conj (get m ik #{}) x))))
     {} xrel))

(defn- map-invert-impl
  "Returns the map with the vals mapped to the keys."
  [m]
  (reduce-kv (fn [result k v] (assoc result v k)) {} m))

;; Heterogeneous rows retain the supported relational behavior without relying
;; on the unordered set's choice of a first row to infer a shared schema.
(defn- heterogeneous-join [xrel yrel]
  (reduce (fn [result x]
    (require-map x)
    (reduce (fn [result y]
      (require-map y)
      (let [ks (intersection (set (keys x)) (set (keys y)))]
        (if (= (selected-keys x ks) (selected-keys y ks))
          (conj result (if (<= (count xrel) (count yrel)) (merge x y) (merge y x)))
          result))) result yrel)) #{} xrel))
(defn- uniform-schema? [relation schema]
  (every? (fn [row] (= schema (set (keys (require-map row))))) relation))

(defn- join-impl
  "When passed 2 rels, returns the rel corresponding to the natural
  join. When passed an additional keymap, joins on the corresponding
  keys."
  ([xrel yrel] ;natural join
   (if (and (seq xrel) (seq yrel))
     (let [xkeys (set (keys (first xrel))) ykeys (set (keys (first yrel)))]
       (if (and (uniform-schema? xrel xkeys) (uniform-schema? yrel ykeys))
         (let [ks (intersection xkeys ykeys)
           [r s] (if (<= (count xrel) (count yrel))
                   [xrel yrel]
                   [yrel xrel])
           idx (index r ks)]
       (reduce (fn [ret x]
                 (let [found (idx (selected-keys x ks))]
                   (if found
                     (reduce #(conj %1 (merge %2 x)) ret found)
                     ret)))
               #{} s))
         (heterogeneous-join xrel yrel)))
     #{}))
  ([xrel yrel km] ;arbitrary key mapping
   (let [[r s k] (if (<= (count xrel) (count yrel))
                   [xrel yrel (map-invert km)]
                   [yrel xrel km])
         idx (index r (vals k))]
     (reduce (fn [ret x]
               (let [found (idx (rename-keys (selected-keys x (keys k)) k))]
                 (if found
                   (reduce #(conj %1 (merge %2 x)) ret found)
                   ret)))
             #{} s))))

(defn- subset?-impl
  "Is set1 a subset of set2?"
  [set1 set2]
  (require-set set1) (require-set set2)
  (and (<= (count set1) (count set2))
       (every? #(contains? set2 %) set1)))

(defn- superset?-impl
  "Is set1 a superset of set2?"
  [set1 set2]
  (require-set set1) (require-set set2)
  (and (>= (count set1) (count set2))
       (every? #(contains? set1 %) set2)))


;; Preserve the supported native arity boundary for saved/apply calls too.
(defn- checked-arity [f minimum maximum]
  (js* "((f, lo, hi) => function(...args) { if (args.length < lo || (hi !== null && args.length > hi)) throw new Error('Invalid arity: ' + args.length); return f(...args); })(~{f}, ~{minimum}, ~{maximum})"))
(def ^{:arglists '([] [s1] [s1 s2] [s1 s2 & sets])} union "Return a set that is the union of the input sets"
  (checked-arity union-impl 0 nil))
(def ^{:arglists '([s1] [s1 s2] [s1 s2 & sets])} intersection "Return a set that is the intersection of the input sets"
  (checked-arity intersection-impl 1 nil))
(def ^{:arglists '([s1] [s1 s2] [s1 s2 & sets])} difference "Return a set that is the first set without elements of the remaining sets"
  (checked-arity difference-impl 1 nil))
(def ^{:arglists '([pred xset])} select "Returns a set of the elements for which pred is true"
  (checked-arity select-impl 2 2))
(def ^{:arglists '([xrel ks])} project "Returns a rel of the elements of xrel with only the keys in ks"
  (checked-arity project-impl 2 2))
(def ^{:arglists '([map kmap])} rename-keys "Returns the map with the keys in kmap renamed to the vals in kmap"
  (checked-arity rename-keys-impl 2 2))
(def ^{:arglists '([xrel kmap])} rename "Returns a rel of the maps in xrel with the keys in kmap renamed to the vals in kmap"
  (checked-arity rename-impl 2 2))
(def ^{:arglists '([xrel ks])} index "Returns a map of the distinct values of ks in the xrel mapped to a
  set of the maps in xrel with the corresponding values of ks."
  (checked-arity index-impl 2 2))
(def ^{:arglists '([m])} map-invert "Returns the map with the vals mapped to the keys."
  (checked-arity map-invert-impl 1 1))
(def ^{:arglists '([xrel yrel] [xrel yrel km])} join "When passed 2 rels, returns the rel corresponding to the natural
  join. When passed an additional keymap, joins on the corresponding
  keys."
  (checked-arity join-impl 2 3))
(def ^{:arglists '([set1 set2])} subset? "Is set1 a subset of set2?"
  (checked-arity subset?-impl 2 2))
(def ^{:arglists '([set1 set2])} superset? "Is set1 a superset of set2?"
  (checked-arity superset?-impl 2 2))
