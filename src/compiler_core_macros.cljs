;; Compiler-owned source macro algorithms. Loaded in the bootstrap namespace;
;; each public macro function receives the usual hidden form and environment.
;; Construction uses ordinary core functions and primitive control forms only.

(defn macro-short-circuit [forms is-or]
  (if (seq forms)
    (reduce (fn [result form]
              (let [temp (gensym "logic")]
                (list 'let* [temp form]
                  (if is-or (list 'if temp temp result)
                    (list 'if temp result temp)))))
      (last forms) (reverse (butlast forms)))
    (if is-or nil true)))

(defmacro macro-and [& forms] (macro-short-circuit forms false))
(defmacro macro-or [& forms] (macro-short-circuit forms true))
(defmacro macro-with-in-str [text & body]
  (list 'cljs.core/compiler-reader-with-input text
    (cons 'fn* (cons [] body))))
(defmacro macro-if-not
  ([test then]
   (list 'if (list 'cljs.core/not test) then nil))
  ([test then otherwise]
   (list 'if (list 'cljs.core/not test) then otherwise)))
(defmacro macro-when [test & body]
  (list 'if test (cons 'do body)))
(defmacro macro-when-not [test & body]
  (list 'if test nil (cons 'do body)))

(defn macro-conditional-binding [bindings body some? when? first?]
  (if (vector? bindings) nil
    (throw (ex-info "conditional binding requires a vector" {})))
  (if (= 2 (count bindings)) nil
    (throw (ex-info "conditional binding requires exactly two forms" {})))
  (let [temp (gensym "binding")
        input (if first? (list 'cljs.core/seq (second bindings)) (second bindings))
        value (if first? (list 'cljs.core/first temp) temp)
        yes (cons 'cljs.core/let
              (cons [(first bindings) value] (if when? body (list (first body)))))
        no (if when? nil (second body))]
    (list 'let* [temp input]
      (if some? (list 'if (list 'cljs.core/nil? temp) no yes)
        (list 'if temp yes no)))))

(defmacro macro-if-let [bindings then & otherwise]
  (macro-conditional-binding bindings (cons then otherwise) false false false))
(defmacro macro-if-some [bindings then & otherwise]
  (macro-conditional-binding bindings (cons then otherwise) true false false))
(defmacro macro-when-let [bindings & body]
  (macro-conditional-binding bindings body false true false))
(defmacro macro-when-some [bindings & body]
  (macro-conditional-binding bindings body true true false))
(defmacro macro-when-first [bindings & body]
  (macro-conditional-binding bindings body false true true))

(defmacro macro-cond [& clauses]
  (if (seq clauses)
    (if (next clauses)
      (list 'if (first clauses) (second clauses)
        (cons 'cljs.core/cond (nnext clauses)))
      (throw (ex-info "cond requires an even number of forms" {})))
    nil))

(defmacro macro-condp [pred expr & clauses]
  (let [p (gensym "predicate") value (gensym "value")
        parsed (loop* [remaining clauses pairs []]
                 (if (next remaining)
                   (let [arrow (if (= :>> (second remaining))
                                 (boolean (nnext remaining)) false)
                         width (if arrow 3 2)]
                     (recur (drop width remaining)
                       (conj pairs [(vec (take width remaining)) arrow])))
                   [pairs remaining]))
        result (if (seq (second parsed)) (first (second parsed))
                 (list 'compiler-case* value [] []))]
    (list 'let* [p pred value expr]
      (reduce (fn [result pair]
                (let [clause (first pair) arrow (second pair)
                      test (list p (first clause) value)]
                  (if arrow
                    (let [matched (gensym "matched")]
                      (list 'let* [matched test]
                        (list 'if matched (list (nth clause 2) matched) result)))
                    (list 'if test (second clause) result))))
        result (reverse (first parsed))))))

(defn macro-thread-step [value form last?]
  (let [forms (if (seq? form) (if (seq form) form (list nil)) (list form))
        result (if last? (apply list (concat forms (list value)))
                 (list* (first forms) value (next forms)))]
    (with-meta result (meta form))))
(defmacro macro-thread-first [value & forms]
  (reduce (fn [value form] (macro-thread-step value form false)) value forms))
(defmacro macro-thread-last [value & forms]
  (reduce (fn [value form] (macro-thread-step value form true)) value forms))
(defmacro macro-as-thread [value name & forms]
  (list 'cljs.core/let
    (vec (concat [name value] (mapcat (fn [form] [name form]) (butlast forms))))
    (if (seq forms) (last forms) name)))

(defn macro-conditional-thread [value clauses last? some?]
  (if some? nil
    (if (even? (count clauses)) nil
      (throw (ex-info "conditional threading requires test/step pairs" {}))))
  (let [temp (gensym "thread") width (if some? 1 2)]
    (loop* [remaining (seq clauses) bindings [temp value] result temp started? false]
      (if remaining
        (let [clause (take width remaining)
              threaded (macro-thread-step temp (last clause) last?)]
          (recur (seq (drop width remaining))
            (if started? (conj bindings temp result) bindings)
            (if some? (list 'if (list 'cljs.core/nil? temp) nil threaded)
              (list 'if (first clause) threaded temp))
            true))
        (list 'let* bindings result)))))
(defmacro macro-cond-thread-first [value & forms]
  (macro-conditional-thread value forms false false))
(defmacro macro-cond-thread-last [value & forms]
  (macro-conditional-thread value forms true false))
(defmacro macro-some-thread-first [value & forms]
  (macro-conditional-thread value forms false true))
(defmacro macro-some-thread-last [value & forms]
  (macro-conditional-thread value forms true true))
(defmacro macro-comment [& forms] nil)

(defmacro macro-case [value & clauses]
  (let [pairs (partition 2 clauses)
        groups (mapv (fn [pair]
                       (let [constants (first pair)]
                         (if (seq? constants) (vec constants) [constants]))) pairs)
        thens (mapv second pairs)
        result (list 'compiler-case* value groups thens)]
    (if (odd? (count clauses)) (concat result (list (last clauses))) result)))

(defmacro macro-while [test & body]
  (list 'loop* []
    (list 'if test (cons 'do (concat body (list (list 'recur)))) nil)))
(defmacro macro-dotimes [bindings & body]
  (if (vector? bindings) nil
    (throw (ex-info "iteration bindings must be a vector" {})))
  (if (= 2 (count bindings)) nil
    (throw (ex-info "dotimes requires exactly two binding forms" {})))
  (let [name (first bindings) limit (gensym "limit")]
    (list 'let* [limit (second bindings)]
      (list 'cljs.core/loop [name 0]
        (list 'if (list 'cljs.core/< name limit)
          (cons 'do (concat body (list (list 'recur (list 'cljs.core/inc name))))) nil)))))
(defmacro macro-vswap! [vol f & args]
  ;; The pinned CLJS macro evaluates vol again after f, for the old value.
  (list 'cljs.core/-vreset! vol (list* f (list 'cljs.core/-deref vol) args)))

(defmacro macro-doto [x & forms]
  (let [target (gensym "target")]
    (list* 'cljs.core/let [target x]
      (concat
        (map (fn [form]
               (if (seq? form) (list* (first form) target (next form))
                 (list form target))) forms)
        [target]))))
(defmacro macro-js-obj [& entries]
  (let [raw-pairs (map vec (partition 2 entries))
        ;; Preserve the existing small-map insertion and duplicate-key policy.
        pairs (if (<= (count raw-pairs) 8)
                (reduce (fn [pairs pair]
                          (if (some (fn [p] (= (first p) (first pair))) pairs)
                            (mapv (fn [p] (if (= (first p) (first pair)) pair p)) pairs)
                            (conj pairs pair))) [] raw-pairs)
                (into {} raw-pairs))
        strings (filter (fn [p] (string? (first p))) pairs)
        symbols (filter (fn [p] (symbol? (first p))) pairs)
        expressions (filter (fn [p]
                              (if (string? (first p)) false
                                (if (symbol? (first p)) false true))) pairs)
        bound (mapv (fn [p] [(gensym "key") (first p) (second p)]) expressions)
        obj (gensym "object")]
    (list* 'cljs.core/let
      (vec (concat (mapcat (fn [p] [(first p) (second p)]) bound)
             [obj (cons 'js* (cons
                    (str "({" (apply str (interpose "," (repeat (count strings) "~{}:~{}"))) "})")
                    (mapcat identity strings)))]))
      (concat
        (map (fn [p] (list 'cljs.core/unchecked-set obj (first p) (second p))) symbols)
        (map (fn [p] (list 'cljs.core/unchecked-set obj (first p) (nth p 2))) bound)
        [obj]))))
(defmacro macro-memfn [name & args]
  (let [target (gensym "target")]
    (list 'cljs.core/fn (vec (cons target args)) (list '. target (cons name args)))))
(defmacro macro-amap [a idx ret expr]
  (let [arr (gensym "array") len (gensym "length")]
    (list 'cljs.core/let [arr a len (list 'cljs.core/alength arr) ret (list 'cljs.core/aclone arr)]
      (list 'cljs.core/loop [idx 0]
        (list 'if (list 'cljs.core/< idx len)
          (list 'do (list 'cljs.core/aset ret idx expr)
            (list 'recur (list 'cljs.core/inc idx))) ret)))))
(defmacro macro-areduce [a idx ret init expr]
  (let [arr (gensym "array") len (gensym "length")]
    (list 'cljs.core/let [arr a len (list 'cljs.core/alength arr)]
      (list 'cljs.core/loop [idx 0 ret init]
        (list 'if (list 'cljs.core/< idx len)
          (list 'recur (list 'cljs.core/inc idx) expr) ret)))))
(defmacro macro-dot-dot [x form & more]
  (let [step (list '. x form)]
    (if (seq more) (cons 'cljs.core/.. (cons step more)) step)))
(defmacro macro-lazy-cat [& colls]
  (cons 'cljs.core/concat (map (fn [coll] (list 'cljs.core/lazy-seq coll)) colls)))
(defmacro macro-selector [m]
  (if (map? m) nil (throw (ex-info "expected a map" {})))
  (let [dirs [:select :excess :missing :all]
        names (into {} (map (fn [dir] [dir (gensym)]) (filter m dirs)))]
    (if (empty? names)
      (throw (ex-info "form must contain at least one of :select :excess :missing :all" {})) nil)
    (let [argument (gensym "map")]
      (list 'cljs.core/fn (gensym "selector") [argument]
        (list 'cljs.core/let [(merge m names) argument]
          (if (= 1 (count names)) (second (first names))
            (list 'cljs.core/some-vals names)))))))

(defmacro macro-defmulti [name & declarations]
  (let [doc (if (string? (first declarations)) (first declarations) nil)
        declarations (if doc (next declarations) declarations)
        attributes (if (map? (first declarations)) (first declarations) nil)
        declarations (if attributes (next declarations) declarations)
        dispatch (first declarations)
        options (next declarations)]
    (if (symbol? name) nil
      (throw (ex-info "defmulti requires a symbol name" {})))
    (if (seq declarations) nil
      (throw (ex-info "defmulti requires a dispatch function" {})))
    (if (even? (count options)) nil
      (throw (ex-info "defmulti requires keyword/value option pairs" {})))
    (let [options (apply hash-map options)
          _ (reduce (fn [_ option]
                      (if (contains? #{:default :hierarchy} option) nil
                        (throw (ex-info "Unsupported defmulti option" {:option option}))))
              nil (keys options))
          metadata (merge (meta name) attributes (if doc {:doc doc} {}))
          name (with-meta name metadata)
          qualified (if (namespace name) (str name)
                      (str (get-in &env [:ns :name]) "/" name))]
      (list 'cljs.core/defonce name
        (list 'new 'cljs.core/MultiFn (list 'quote (symbol qualified)) dispatch
          (get options :default :default)
          (get options :hierarchy (list 'cljs.core/get-global-hierarchy))
          (list 'cljs.core/atom {}) (list 'cljs.core/atom {})
          (list 'cljs.core/atom {}) (list 'cljs.core/atom nil))))))

(defmacro macro-defmethod [multifn dispatch-value & fn-tail]
  (list 'cljs.core/-add-method multifn dispatch-value
    (cons 'cljs.core/fn fn-tail)))
