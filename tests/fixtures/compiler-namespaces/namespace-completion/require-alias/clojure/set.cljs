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

(defn- bubble-max-key [k coll]
  "Move a maximal element of coll according to fn k (which returns a number) 
   to the front of coll."
  (let [max (apply max-key k coll)]
    (cons max (remove #(identical? max %) coll))))

(defn union
  "Return a set that is the union of the input sets"
  ([] #{})
  ([s1] s1)
  ([s1 s2]
     (if (< (count s1) (count s2))
       (reduce conj s2 s1)
       (reduce conj s1 s2)))
  ([s1 s2 & sets]
     (let [bubbled-sets (bubble-max-key count (conj sets s2 s1))]
       (reduce into (first bubbled-sets) (rest bubbled-sets)))))

(defn intersection
  "Return a set that is the intersection of the input sets"
  ([s1] s1)
  ([s1 s2]
     (if (< (count s2) (count s1))
       (recur s2 s1)
       (reduce (fn [result item]
                   (if (contains? s2 item)
		     result
                     (disj result item)))
	       s1 s1)))
  ([s1 s2 & sets] 
     (let [bubbled-sets (bubble-max-key #(- (count %)) (conj sets s2 s1))]
       (reduce intersection (first bubbled-sets) (rest bubbled-sets)))))
