;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns library.upstream (:require [clojure.set :as set] [clojure.walk :as w]))
(defn test-union [] (do (let [x (set/union) y #{}] (assert (= x y))) (let [x (set/union #{}) y #{}] (assert (= x y))) (let [x (set/union #{1}) y #{1}] (assert (= x y))) (let [x (set/union #{1 2 3}) y #{1 2 3}] (assert (= x y))) (let [x (set/union #{} #{}) y #{}] (assert (= x y))) (let [x (set/union #{} #{1}) y #{1}] (assert (= x y))) (let [x (set/union #{} #{1 2 3}) y #{1 2 3}] (assert (= x y))) (let [x (set/union #{1} #{}) y #{1}] (assert (= x y))) (let [x (set/union #{1 2 3} #{}) y #{1 2 3}] (assert (= x y))) (let [x (set/union #{1} #{2}) y #{1 2}] (assert (= x y))) (let [x (set/union #{1} #{1 2}) y #{1 2}] (assert (= x y))) (let [x (set/union #{2} #{1 2}) y #{1 2}] (assert (= x y))) (let [x (set/union #{1 2} #{3}) y #{1 2 3}] (assert (= x y))) (let [x (set/union #{1 2} #{2 3}) y #{1 2 3}] (assert (= x y))) (let [x (set/union #{} #{} #{}) y #{}] (assert (= x y))) (let [x (set/union #{1} #{} #{}) y #{1}] (assert (= x y))) (let [x (set/union #{} #{1} #{}) y #{1}] (assert (= x y))) (let [x (set/union #{} #{} #{1}) y #{1}] (assert (= x y))) (let [x (set/union #{1 2} #{2 3} #{}) y #{1 2 3}] (assert (= x y))) (let [x (set/union #{1 2} #{3 4} #{5 6}) y #{1 2 3 4 5 6}] (assert (= x y))) (let [x (set/union #{1 2} #{2 3} #{1 3 4}) y #{1 2 3 4}] (assert (= x y))) (let [x (set/union #{1 2} #{:a :b} #{nil} #{false true} #{\c "abc"} #{[] [1 2]}
      #{{} {:a 1}} #{#{} #{1 2}}) y #{1 2 :a :b nil false true \c "abc" [] [1 2] {} {:a 1} #{} #{1 2}}] (assert (= x y))) (let [x (set/union (hash-set) (hash-set 1 2) (hash-set 2 3)) y (hash-set 1 2 3)] (assert (= x y)))))
(defn test-intersection [] (do (let [x (set/intersection #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1}) y #{1}] (assert (= x y))) (let [x (set/intersection #{1 2 3}) y #{1 2 3}] (assert (= x y))) (let [x (set/intersection #{} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{} #{1}) y #{}] (assert (= x y))) (let [x (set/intersection #{} #{1 2 3}) y #{}] (assert (= x y))) (let [x (set/intersection #{1} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1 2 3} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1 2} #{1 2}) y #{1 2}] (assert (= x y))) (let [x (set/intersection #{1 2} #{3 4}) y #{}] (assert (= x y))) (let [x (set/intersection #{1 2} #{1}) y #{1}] (assert (= x y))) (let [x (set/intersection #{1 2} #{2}) y #{2}] (assert (= x y))) (let [x (set/intersection #{1 2 4} #{2 3 4 5}) y #{2 4}] (assert (= x y))) (let [x (set/intersection #{} #{} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1} #{} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1} #{1} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1} #{} #{1}) y #{}] (assert (= x y))) (let [x (set/intersection #{1 2} #{2 3} #{}) y #{}] (assert (= x y))) (let [x (set/intersection #{1 2} #{2 3} #{5 2}) y #{2}] (assert (= x y))) (let [x (set/intersection #{1 2 3} #{1 3 4} #{1 3}) y #{1 3}] (assert (= x y))) (let [x (set/intersection #{1 2 3} #{3 4 5} #{8 2 3}) y #{3}] (assert (= x y))) (let [x (set/intersection (hash-set 1 2) (hash-set 2 3)) y #{2}] (assert (= x y)))))
(defn test-difference [] (do (let [x (set/difference #{}) y #{}] (assert (= x y))) (let [x (set/difference #{1}) y #{1}] (assert (= x y))) (let [x (set/difference #{1 2 3}) y #{1 2 3}] (assert (= x y))) (let [x (set/difference #{1 2} #{1 2}) y #{}] (assert (= x y))) (let [x (set/difference #{1 2} #{3 4}) y #{1 2}] (assert (= x y))) (let [x (set/difference #{1 2} #{1}) y #{2}] (assert (= x y))) (let [x (set/difference #{1 2} #{2}) y #{1}] (assert (= x y))) (let [x (set/difference #{1 2 4} #{2 3 4 5}) y #{1}] (assert (= x y))) (let [x (set/difference #{1 2} #{2 3} #{5 2}) y #{1}] (assert (= x y))) (let [x (set/difference #{1 2 3} #{1 3 4} #{1 3}) y #{2}] (assert (= x y))) (let [x (set/difference #{1 2 3} #{3 4 5} #{8 2 3}) y #{1}] (assert (= x y)))))
(defn test-select [] (do (let [x (set/select integer? #{}) y #{}] (assert (= x y))) (let [x (set/select integer? #{1 2}) y #{1 2}] (assert (= x y))) (let [x (set/select integer? #{1 2 :a :b :c}) y #{1 2}] (assert (= x y))) (let [x (set/select integer? #{:a :b :c}) y #{}] (assert (= x y)))))
(def compositions #{{:name "Art of the Fugue" :composer "J. S. Bach"}
    {:name "Musical Offering" :composer "J. S. Bach"}
    {:name "Requiem" :composer "Giuseppe Verdi"}
    {:name "Requiem" :composer "W. A. Mozart"}})
(defn test-project [] (do (let [x (set/project compositions [:name]) y #{{:name "Art of the Fugue"}
                                         {:name "Requiem"}
                                         {:name "Musical Offering"}}] (assert (= x y))) (let [x (set/project compositions [:composer]) y #{{:composer "W. A. Mozart"}
                                             {:composer "Giuseppe Verdi"}
                                             {:composer "J. S. Bach"}}] (assert (= x y))) (let [x (set/project compositions [:year]) y #{{}}] (assert (= x y))) (let [x (set/project #{{}} [:name]) y #{{}}] (assert (= x y)))))
(defn test-rename [] (do (let [x (set/rename compositions {:name :title}) y #{{:title "Art of the Fugue" :composer "J. S. Bach"}
                                               {:title "Musical Offering" :composer "J. S. Bach"}
                                               {:title "Requiem" :composer "Giuseppe Verdi"}
                                               {:title "Requiem" :composer "W. A. Mozart"}}] (assert (= x y))) (let [x (set/rename compositions {:year :decade}) y #{{:name "Art of the Fugue" :composer "J. S. Bach"}
                                                {:name "Musical Offering" :composer "J. S. Bach"}
                                                {:name "Requiem" :composer "Giuseppe Verdi"}
                                                {:name "Requiem" :composer "W. A. Mozart"}}] (assert (= x y))) (let [x (set/rename #{{}} {:year :decade}) y #{{}}] (assert (= x y)))))
(defn test-rename-keys [] (do (let [x (set/rename-keys {:a "one" :b "two"} {:a :z}) y {:z "one" :b "two"}] (assert (= x y))) (let [x (set/rename-keys {:a "one" :b "two"} {:a :z :c :y}) y {:z "one" :b "two"}] (assert (= x y))) (let [x (set/rename-keys {:a "one" :b "two" :c "three"} {:a :b :b :a}) y {:a "two" :b "one" :c "three"}] (assert (= x y)))))
(defn test-index [] (do (let [x (set/index  #{{:c 2} {:b 1} {:a 1 :b 2}} [:b]) y {{:b 2} #{{:a 1 :b 2}}, {:b 1} #{{:b 1}} {} #{{:c 2}}}] (assert (= x y)))))
(defn test-join [] (do (let [x (set/join compositions compositions) y compositions] (assert (= x y))) (let [x (set/join compositions #{{:name "Art of the Fugue" :genre "Classical"}}) y #{{:name "Art of the Fugue" :composer "J. S. Bach" :genre "Classical"}}] (assert (= x y)))))
(defn test-map-invert [] (do (let [x (set/map-invert {:a "one" :b "two"}) y {"one" :a "two" :b}] (assert (= x y)))))
(defn test-subset? [] (do (let [sub #{} super #{}] (assert (set/subset? sub super))) (let [sub #{} super #{1}] (assert (set/subset? sub super))) (let [sub #{1} super #{1}] (assert (set/subset? sub super))) (let [sub #{1 2} super #{1 2}] (assert (set/subset? sub super))) (let [sub #{1 2} super #{1 2 42}] (assert (set/subset? sub super))) (let [sub #{false} super #{false}] (assert (set/subset? sub super))) (let [sub #{nil} super #{nil}] (assert (set/subset? sub super))) (let [sub #{nil} super #{nil false}] (assert (set/subset? sub super))) (let [sub #{1 2 nil} super #{1 2 nil 4}] (assert (set/subset? sub super)))) (do (let [notsub #{1} super #{}] (assert (not (set/subset? notsub super)))) (let [notsub #{2} super #{1}] (assert (not (set/subset? notsub super)))) (let [notsub #{1 3} super #{1}] (assert (not (set/subset? notsub super)))) (let [notsub #{nil} super #{false}] (assert (not (set/subset? notsub super)))) (let [notsub #{false} super #{nil}] (assert (not (set/subset? notsub super)))) (let [notsub #{false nil} super #{nil}] (assert (not (set/subset? notsub super)))) (let [notsub #{1 2 nil} super #{1 2}] (assert (not (set/subset? notsub super))))))
(defn test-superset? [] (do (let [super #{} sub #{}] (assert (set/superset? super sub))) (let [super #{1} sub #{}] (assert (set/superset? super sub))) (let [super #{1} sub #{1}] (assert (set/superset? super sub))) (let [super #{1 2} sub #{1 2}] (assert (set/superset? super sub))) (let [super #{1 2 42} sub #{1 2}] (assert (set/superset? super sub))) (let [super #{false} sub #{false}] (assert (set/superset? super sub))) (let [super #{nil} sub #{nil}] (assert (set/superset? super sub))) (let [super #{false nil} sub #{false}] (assert (set/superset? super sub))) (let [super #{1 2 4 nil false} sub #{1 2 nil}] (assert (set/superset? super sub)))) (do (let [notsuper #{} sub #{1}] (assert (not (set/superset? notsuper sub)))) (let [notsuper #{2} sub #{1}] (assert (not (set/superset? notsuper sub)))) (let [notsuper #{1} sub #{1 3}] (assert (not (set/superset? notsuper sub)))) (let [notsuper #{nil} sub #{false}] (assert (not (set/superset? notsuper sub)))) (let [notsuper #{false} sub #{nil}] (assert (not (set/superset? notsuper sub)))) (let [notsuper #{nil} sub #{false nil}] (assert (not (set/superset? notsuper sub)))) (let [notsuper #{nil 2 3} sub #{false nil 2 3}] (assert (not (set/superset? notsuper sub))))))
(defrecord Rec1 [a])
(defn inc-leaf [x] (if (number? x) (inc x) x))
(defn test-api [] (do (assert (= [2 {1 "1", :two 2}] (w/postwalk inc-leaf [1 {0 "1", :two 1}]))) (assert (= [(Rec1. 2)] (w/postwalk inc-leaf [(Rec1. 1)]))) (assert (= (map->Rec1 {:a 1, ":a" 1})
           (w/prewalk #(if (keyword? %) (str %) %) (Rec1. 1))))))
(defn test-preserves-meta [] (do (assert (= (-> (w/prewalk identity [1 (with-meta [1 2] {:foo 3})])
             (nth 1) meta)
           {:foo 3})) (assert (= (-> (w/postwalk identity [1 (with-meta [1 2] {:foo 3})])
             (nth 1) meta)
          {:foo 3}))))
(defn test-map-entry [] (assert (= [:a 2] (clojure.walk/postwalk #(cond-> % (number? %) inc) (->MapEntry :a 1 nil)))))
(defn t-prewalk-replace [] (assert (= (w/prewalk-replace {:a :b} [:a {:a :a} (list 3 :c :a)])
        [:b {:b :b} (list 3 :c :b)])))
(defn t-postwalk-replace [] (assert (= (w/postwalk-replace {:a :b} [:a {:a :a} (list 3 :c :a)])
        [:b {:b :b} (list 3 :c :b)])))
(defn t-stringify-keys [] (assert (= (w/stringify-keys {:a 1, nil {:b 2 :c 3}, :d 4})
        {"a" 1, nil {"b" 2 "c" 3}, "d" 4})))
(defn t-prewalk-order [] (assert (= (let [a (atom [])]
           (w/prewalk (fn [form] (swap! a conj form) form)
             [1 2 {:a 3} (list 4 [5])])
           @a)
        [[1 2 {:a 3} (list 4 [5])]
         1 2 {:a 3} [:a 3] :a 3 (list 4 [5])
         4 [5] 5])))
(defn t-postwalk-order [] (assert (= (let [a (atom [])]
           (w/postwalk (fn [form] (swap! a conj form) form)
             [1 2 {:a 3} (list 4 [5])])
           @a)
        [1 2
         :a 3 [:a 3] {:a 3}
         4 5 [5] (list 4 [5])
         [1 2 {:a 3} (list 4 [5])]])))
(defrecord Foo [a b c])
(defn walk-mapentry [] (let [coll [:html {:a ["b" 1]} ""] f (fn [e] (if (and (vector? e) (not (map-entry? e))) (apply list e) e))] (assert (= (list :html {:a (list "b" 1)} "") (w/postwalk f coll)))))
(defrecord RM [a])
(defn retain-meta [] (let [m {:foo true}] (do (let [o '(1 2)] (assert (= m (meta (w/postwalk identity (with-meta o m)))))) (let [o [1 2]] (assert (= m (meta (w/postwalk identity (with-meta o m)))))) (let [o #{1 2}] (assert (= m (meta (w/postwalk identity (with-meta o m)))))) (let [o {1 2}] (assert (= m (meta (w/postwalk identity (with-meta o m)))))) (let [o (map inc (range 3))] (assert (= m (meta (w/postwalk identity (with-meta o m)))))) (let [o (->RM 1)] (assert (= m (meta (w/postwalk identity (with-meta o m)))))))))
(defn run [] #js [(try (test-union) true (catch js/Error e false)) (try (test-intersection) true (catch js/Error e false)) (try (test-difference) true (catch js/Error e false)) (try (test-select) true (catch js/Error e false)) (try (test-project) true (catch js/Error e false)) (try (test-rename) true (catch js/Error e false)) (try (test-rename-keys) true (catch js/Error e false)) (try (test-index) true (catch js/Error e false)) (try (test-join) true (catch js/Error e false)) (try (test-map-invert) true (catch js/Error e false)) (try (test-subset?) true (catch js/Error e false)) (try (test-superset?) true (catch js/Error e false)) (try (test-api) true (catch js/Error e false)) (try (test-preserves-meta) true (catch js/Error e false)) (try (test-map-entry) true (catch js/Error e false)) (try (t-prewalk-replace) true (catch js/Error e false)) (try (t-postwalk-replace) true (catch js/Error e false)) (try (t-stringify-keys) true (catch js/Error e false)) (try (t-prewalk-order) true (catch js/Error e false)) (try (t-postwalk-order) true (catch js/Error e false)) (try (walk-mapentry) true (catch js/Error e false)) (try (retain-meta) true (catch js/Error e false))])
(every? true? (run))
