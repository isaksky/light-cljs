;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php).
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license. You must not remove this notice.

(ns cljs.core-test)


(deftype KeywordTest []
  ILookup
  (-lookup [o k] :nothing)
  (-lookup [o k not-found] not-found))

(defn tset-638 []
  (assert (= (:a (KeywordTest.)) :nothing)))

(deftype MyWatchable []
  IWatchable
  (-notify-watches [this oldval newval])
  (-add-watch [this key f])
  (-remove-watch [this key]))

(defn test-920-watch-ops-return-ref []
  (do
    (let [w (MyWatchable.)]
      (assert (identical? (add-watch w :foo (fn [])) w))
      (assert (identical? (remove-watch w :foo) w)))))

(deftype MyCustomAtom [^:mutable state]
  IDeref
  (-deref [_] state)
  IReset
  (-reset! [_ newval]
    (set! state newval)))

(defn test-919-generic-cas []
  (do
    (let [a0 (MyCustomAtom. 10)
          a1 (MyCustomAtom. 0)]
      (compare-and-set! a0 0 20)
      (compare-and-set! a1 0 20)
      (assert (== @a0 10))
      (assert (== @a1 20)))))

(deftype MapWithNoIKVReduce [backing-map]
  IMap
  (-dissoc [_ _] nil)

  ISeqable
  (-seq [_] (seq backing-map)))

(defn test-cljs-2083 []
  (do
    (assert (true?  (equiv-map (MapWithNoIKVReduce. {:a 1 :b 2 :c 3}) {:a 1 :b 2 :c 3})))
    (assert (false? (equiv-map (MapWithNoIKVReduce. {:a 1 :b 2 :c 3}) {:a 1 :b 2 :c 4})))))

(deftype Foo2407 [x y])
(defrecord Bar2407 [x y])

(defn test-cljs-2407 []
  (assert (= "Positional factory function for cljs.core-test/Foo2407." (:doc (meta #'->Foo2407))))
  (assert (= "Positional factory function for cljs.core-test/Bar2407." (:doc (meta #'->Bar2407))))
  (assert (= "Factory function for cljs.core-test/Bar2407, taking a map of keywords to field values." (:doc (meta #'map->Bar2407)))))

(defn run [observe] (tset-638) (test-920-watch-ops-return-ref) (test-919-generic-cas) (test-cljs-2083) (test-cljs-2407) true)

run
