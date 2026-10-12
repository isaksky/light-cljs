;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

;; Adapted from pinned ClojureScript core_test.cljs; EPL-1.0.
(ns cljs.core-test)
;; ObjMap storage representation is outside this dialect; retain each map construction.
(def obj-map array-map)

(defmulti bar (fn [x y] [x y]))

(defmethod bar [::rect ::shape] [x y] :rect-shape)

(defmethod bar [::shape ::rect] [x y] :shape-rect)

(defmulti nested-dispatch (fn [m] (-> m :a :b)))

(defmethod nested-dispatch :c [m] :nested-a)

(defmethod nested-dispatch :default [m] :nested-default)

(defmulti nested-dispatch2 ffirst)

(defmethod nested-dispatch2 :a [m] :nested-a)

(defmethod nested-dispatch2 :default [m] :nested-default)

(defmulti foo1 (fn [& args] (first args)))

(defmethod foo1 :a [& args] :a-return)

(defmethod foo1 :default [& args] :default-return)

(defmulti area :Shape)

(defn rect [wd ht] {:Shape :Rect :wd wd :ht ht})

(defn circle [radius] {:Shape :Circle :radius radius})

(defmethod area :Rect [r]
    (* (:wd r) (:ht r)))

(defmethod area :Circle [c]
    (*  js/Math.PI (* (:radius c) (:radius c))))

(defmethod area :default [x] :oops)

(defmulti foo2 (fn []))

(defmethod foo2 :default [] :foo)

(defmulti apply-multi-test (fn ([_] 0) ([_ _] 0) ([_ _ _] 0)))

(defmethod apply-multi-test 0
  ([x] :one)
  ([x y] :two)
  ([x y & r] [:three r]))

(defmulti no-dispatch-value :test)

(def my-map-hierarchy
  (atom (-> (make-hierarchy)
          (derive (type (obj-map)) ::map)
          (derive (type (array-map)) ::map)
          (derive (type (hash-map)) ::map)
          (derive (type (sorted-map)) ::map))))

(defmulti my-map? type :hierarchy my-map-hierarchy)

(defmethod my-map? ::map [_] true)

(defmethod my-map? :default [_] false)

(defmulti foo2' identity)

(defmethod foo2' 0 [x] x)

(def three-levels-h (-> (make-hierarchy)
                        (derive :parent :gparent)
                        (derive :child :parent)))

(defmulti multi-with-h (fn [v] v) :hierarchy #'three-levels-h)

(defmethod multi-with-h :gparent [_] :gparent)

(defmethod multi-with-h :parent [_] :parent)

(defn run [observe]
(do
  (let [r (rect 4 13)
        c (circle 12)]
    (do
      (assert (= :nested-a (nested-dispatch {:a {:b :c}})))
      (assert (= :nested-a (nested-dispatch2 [[:a :b]])))
      (assert (= :a-return (foo1 :a)))
      (assert (= :default-return (foo1 1)))
      (assert (= 52 (area r)))
      (assert (= :oops (area {})))
      ;; CLJS-863
      (assert (= :foo (foo2)))
      ;; remove method tests
      (assert (= 2 (count (methods bar))))
      (remove-method bar [::rect ::shape])
      (assert (= 1 (count (methods bar))))
      (remove-all-methods bar)
      (assert (zero? (count (methods bar))))
      (assert (= [:three '(2)] (apply apply-multi-test [0 1 2])))
      (assert (try
            (no-dispatch-value {:test :test})
            (catch js/Error e
              (not= -1 (.indexOf (.-message e) "cljs.core-test/no-dispatch-value")))))
      (doseq [m [(obj-map) (array-map) (hash-map) (sorted-map)]]
        (assert (my-map? m)))
      (doseq [not-m [[] 1 "asdf" :foo]]
        (assert (not (my-map? not-m))))
      ;; multimethod hashing
      (assert (= foo2' (ffirst {foo2' 1})))
      (assert (= :parent (multi-with-h :child)))
)))
true)

run
