(ns set-project.main (:require [clojure.set :as s :refer [project]]))

;; Both complete SCI sources remain byte-for-byte inside these bodies.
(defn original-relation-meta [] (meta (clojure.set/project (with-meta #{ {:a 1 :b 2} } {:m true}) [:a])))
(defn original-key-representative [] (let [old (with-meta (symbol "a") {:old true}) new (with-meta (symbol "a") {:new true})] (meta (first (keys (first (clojure.set/project #{{old 1}} [new])))))))

(defn project-contract []
  (let [old (with-meta (symbol "a") {:old true})
        requested (with-meta (symbol "a") {:new true})
        repeated (with-meta (symbol "a") {:repeated true})
        value (with-meta [1] {:value true})
        row (with-meta {old value :nil nil :false false :other 2} {:row true})
        relation (with-meta #{row} {:relation true})
        saved project
        public (get (ns-publics 'clojure.set) 'project)
        projected (first (project relation [requested repeated :nil :false :missing]))]
    (assert (nil? (original-relation-meta)))
    (assert (= {:new true} (original-key-representative)))
    (assert (= {requested value :nil nil :false false} projected))
    (assert (= {:row true} (meta projected)))
    (assert (= {:value true} (meta (get projected requested))))
    (assert (= {:new true} (meta (first (keys projected)))))
    (assert (= {:old true} (meta (first (keys row)))))
    (assert (= {:relation true} (meta relation)))
    (assert (= {:row true} (meta row)))
    (assert (= {old value :nil nil :false false :other 2} row))
    (doseq [f [project s/project clojure.set/project saved public]]
      (let [result (f relation [requested])]
        (assert (= #{{requested value}} result))
        (assert (nil? (meta result)))
        (assert (= {:row true} (meta (first result))))
        (assert (= {:new true} (meta (first (keys (first result))))))))
    (assert (= #{{requested value}} (apply saved [relation [requested]])))
    (assert (= #{} (project nil [:a]) (project (with-meta #{} {:empty true}) [:a])))
    (assert (nil? (meta (project (with-meta #{} {:empty true}) [:a]))))
    (assert (= #{{}} (project relation nil) (project relation []) (project [nil {}] [:a])))
    (assert (= #{{:a 1}} (project [{:a 1 :b 2} {:a 1 :b 3}] [:a :a :missing])))
    ;; index uses the same selection helper but preserves original grouped rows.
    (let [indexed (s/index [row] [requested]) key-map (first (keys indexed))]
      (assert (nil? (meta indexed)))
      (assert (= {:row true} (meta key-map)))
      (assert (= {:new true} (meta (first (keys key-map)))))
      (assert (identical? row (first (get indexed key-map)))))
    (assert (= #{{old value :nil nil :false false :other 2 :joined true}}
               (s/join #{row} #{{requested value :joined true}})))
    (assert (= #{{old value :nil nil :false false :other 2 :target value}}
               (s/join #{row} #{{:target value}} {requested :target})))
    (let [renamed (s/rename-keys row {requested :target})]
      (assert (= {:row true} (meta renamed)))
      (assert (= value (:target renamed)))
      (assert (not (contains? renamed old))))
    (assert (= #{{:target value}} (s/rename #{{old value}} {requested :target})))
    ;; Arguments and lazy key requests are evaluated in their observable order.
    (let [seen (atom [])
          keys (letfn [(step [n] (lazy-seq (when (< n 2) (swap! seen conj ["key" n]) (cons :a (step (inc n))))))] (step 0))]
      (assert (= #{{:a 1}}
                 (project (do (swap! seen conj "relation") [{:a 1} {:a 1 :b 2}])
                          (do (swap! seen conj "keys") keys))))
      (assert (= ["relation" "keys" ["key" 0] ["key" 1]] @seen)))
    true))

;; The supported checked native arity/map domain remains a separate local
;; contract. Canonical fixed-arity JS project tolerates these malformed calls.
(defn project-invalid-profile []
  (let [saved project relation #{{:a 1}}]
    (mapv (fn [args] (try (pr-str (apply saved args)) (catch :default e (.-message e))))
          [[] [relation] [relation [:a] :extra] [[1] [:a]]])))

(defn project-arity-domain-contract []
  (let [saved project relation #{{:a 1}}]
    (assert (= ["Invalid arity: 0" "Invalid arity: 1" "Invalid arity: 3"]
               (mapv (fn [args] (try (apply saved args) "unexpected success" (catch :default e (.-message e))))
                     [[] [relation] [relation [] :extra]])))
    (assert (try (saved [1] [:a]) false (catch :default _ true)))
    true))

(defn run [] (project-contract))
