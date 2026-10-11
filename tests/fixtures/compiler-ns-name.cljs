;; Public result-type contract from pinned cljs/core.cljs; EPL-1.0.
;; This exact source executes on both references and in local applications/macros.
(def ns-name-value 41)
(defn ns-name-contract []
  (let [sym (symbol "reflection.scope") obj #js {:answer 42} saved ns-name]
    (doseq [name [sym "reflection.scope"]
            n [(Namespace. obj name) (let [ctor Namespace] (new ctor obj name))
               (->Namespace obj name) (let [factory ->Namespace] (factory obj name))
               (apply ->Namespace [obj name])]]
      (assert (identical? name (ns-name n)))
      (assert (identical? name (saved n)))
      (assert (identical? name (apply saved [n])))
      (assert (identical? name (.getName n)))
      (assert (= "reflection.scope" (str n)))
      (assert (= n (Namespace. obj name)))
      (assert (= (hash name) (hash n)))
      (let [v (.findInternedVar n 'answer)]
        (assert (= 42 @v))
        (assert (identical? n (:ns (meta v))))
        (assert (identical? name (ns-name (:ns (meta v)))))))
    (doseq [n [(create-ns sym) (create-ns sym obj) (find-ns 'cljs.core)]]
      (assert (symbol? (ns-name n)))
      (assert (= (ns-name n) (.getName n)))
      (assert (= (str (ns-name n)) (str n))))
    (doseq [v [#'ns-name-value #'ns-name-contract #'ns-name #'->Namespace]]
      (let [n (:ns (meta v)) name (namespace (.-sym v))]
        (assert (symbol? n))
        (assert (string? (ns-name n)))
        (assert (= name (ns-name n) (saved n) (apply saved [n])))
        (assert (= name (str n)))))
    (assert (= "member" (ns-name 'qualified/member)))
    (assert (= "object.name" (ns-name #js {:name "object.name"})))
    true))
