;; Local contract based on public declarations in pinned cljs/core.cljs,
;; a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e (EPL-1.0).
;; Ordinary defmacro setup is the local phase dialect; it is not an upstream body.
(defn metadata-contract []
  (let [vars [#'*1 #'*2 #'*3 #'*assert* #'*print-err-fn*]
        names '[*1 *2 *3 *assert* *print-err-fn*]
        lines [213 218 223 71 94]
        docs ["bound in a repl thread to the most recent value printed"
              "bound in a repl thread to the second most recent value printed"
              "bound in a repl thread to the third most recent value printed"
              "When set to logical false, 'assert' will omit assertion checks in\n compiled code. Defaults to true."
              "Each runtime environment provides a different way to print error output.\n  Whatever function *print-err-fn* is bound to will be passed any\n  Strings which should be printed."]]
    (doseq [i (range 5)]
      (let [v (nth vars i) m (meta v)]
        (assert (var? v))
        (assert (= (nth names i) (:name m)))
        (assert (symbol? (:ns m)))
        (assert (= "cljs.core" (ns-name (:ns m))))
        (assert (= "cljs/core.cljs" (:file m)))
        (assert (= (nth lines i) (:line m)))
        (assert (= 1 (:column m)))
        (assert (= (nth docs i) (:doc m)))
        (assert (true? (:dynamic m)))
        (assert (seq? (:arglists m)))
        (assert (= '() (:arglists m)))))
    (assert (= [:one :two :three false "error:x"]
               (binding [*1 :one *2 :two *3 :three *assert* false
                         *print-err-fn* (fn [s] (str "error:" s))]
                 [@#'*1 @#'*2 @#'*3 @#'*assert* (#'*print-err-fn* "x")])))
    ;; These aliases share an operation, while their Vars retain source identity.
    (doseq [[v name line args] [[#'clone 'clone 1271 '([value])]
                               [#'-clone '-clone 615 '([value])]
                               [#'str 'str 3140 '([] [x] [x & ys])]
                               [#'str_ 'str_ 3117 '([x var-args])]
                               [#'Var 'Var 1188 '()]
                               [#'->Var '->Var 1188 '([val sym _meta])]
                               [#'Namespace 'Namespace 12132 '()]
                               [#'get 'get 2032 '([o k] [o k not-found])]]]
      (let [m (meta v)]
        (assert (= name (:name m)))
        (assert (= line (:line m)))
        (assert (symbol? (:ns m)))
        (assert (= "cljs.core" (ns-name (:ns m))))
        (assert (= "cljs/core.cljs" (:file m)))
        (assert (seq? (:arglists m)))
        (assert (= args (:arglists m)))
        (assert (every? vector? (:arglists m)))))
    (assert (= "Clone the supplied value which must implement ICloneable."
               (:doc (meta #'clone))))
    (assert (= "Creates a clone of value." (:doc (meta #'-clone))))
    (assert (= 'cljs.core/ICloneable (:protocol (meta #'-clone))))
    (assert (nil? (:protocol (meta #'clone))))
    (assert (true? (:private (meta #'str_))))
    (let [original (with-meta [1 2] {:m 1}) a (#'clone original) b (#'-clone original)]
      (assert (= [1 2] a))
      (assert (= [1 2] b))
      (assert (not (identical? original a)))
      (assert (not (identical? original b))))
    (assert (= "ab" (#'str "a" "b")))
    (assert (= "ab" (#'str_ "a" "b")))
    (assert (= [2 :missing] [(#'get {:a 2} :a) (#'get {} :a :missing)]))
    true))
