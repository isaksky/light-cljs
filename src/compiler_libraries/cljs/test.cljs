;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

;;; Adapted for light-cljs from ClojureScript's cljs/test.cljs (the runtime)
;;; and cljs/test.cljc (the macros). Both live in this one namespace, as
;;; light-cljs runs macros from ordinary sources. Where upstream asks
;;; cljs.analyzer.api at macro time, this file emits ns-interns and var
;;; reflection or asks light.build; assertion locations come from the
;;; form's metadata and &env instead of JavaScript stack strings, and the
;;; ::pprint reporter prints with pr-str until cljs.pprint is bundled.

(ns
  ^{:author "Stuart Sierra, with contributions and suggestions by
  Chas Emerick, Allen Rohner, Stuart Halloway, David Nolen, and
  Leon Grapenthin",
    :doc "A unit testing framework.

   ASSERTIONS

   The core of the library is the \"is\" macro, which lets you make
   assertions of any arbitrary expression:

   (is (= 4 (+ 2 2)))
   (is (instance? js/Date (js/Date.)))
   (is (.startsWith \"abcde\" \"ab\"))

   The \"is\" macro always returns the result of the inner expression.
   (is (thrown? c body)) checks that an instance of c is thrown from
   body; (is (thrown-with-msg? c re body)) also checks that the message
   matches re. (are [x y] (= x y) 2 (+ 1 1)) checks a template for each
   group of values. \"is\" takes an optional message, and \"testing\"
   adds a context string to the reports of the assertions in its body.

   DEFINING TESTS

   (deftest addition
     (is (= 4 (+ 2 2))))

   A test is a function of no arguments whose body is in its Var's :test
   metadata; tests may call other tests. (async done ...) returned from a
   test continues it asynchronously until done is called, and a test
   named ^:async has an async function body.

   RUNNING TESTS

   (run-tests 'your.namespace 'some.other.namespace) runs the tests of
   those namespaces, or of the current one without arguments, and
   (run-all-tests re) those of every namespace whose name matches re. A
   namespace with a test-ns-hook function runs that instead of its tests.
   (use-fixtures :once f ...) and (use-fixtures :each f ...) attach
   fixtures: maps of :before and :after functions, or functions that take
   the test to run (which async tests cannot use).

   EXTENDING TEST-IS

   Methods of the assert-expr multimethod run while \"is\" expands, so a
   defmethod for it belongs in a namespace that macros load. The report
   multimethod dispatches on the environment's :reporter and the event's
   :type, so a reporter adds methods for [reporter type]."}
  cljs.test
  (:require [clojure.template]
            [light.build :as build]))

;; =============================================================================
;; Default Values

(defn empty-env
  "Generates a testing environment with a reporter.
   (empty-env) - uses the :cljs.test/default reporter.
   (empty-env :cljs.test/pprint) - the default reporter; it prints data
                                   with pr-str until cljs.pprint is bundled.
   (empty-env reporter) - uses a reporter of your choosing.

   To create your own reporter see cljs.test/report"
  ([] (empty-env ::default))
  ([reporter]
   (cond-> {:report-counters {:test 0 :pass 0 :fail 0 :error 0}
            :testing-vars ()
            :testing-contexts ()
            :formatter pr-str
            :reporter reporter}
     (= ::pprint reporter) (assoc :reporter ::default
                                  :formatter pr-str))))

(def ^:dynamic *current-env* nil)

(defn get-current-env []
  (or *current-env* (empty-env)))

(defn update-current-env! [ks f & args]
  (set! *current-env* (apply update-in (get-current-env) ks f args)))

(defn set-env! [new-env]
  (set! *current-env* new-env))

(defn clear-env! []
  (set! *current-env* nil))

(defn get-and-clear-env!
  "Like get-current-env, but cleans env before returning."
  []
  (let [env (get-current-env)]
    (clear-env!)
    env))

;; =============================================================================
;; Test Reporting

(defn testing-vars-str
  "Returns a string representation of the current test.  Renders names
  in :testing-vars as a list, then the source file and line of
  current assertion."
  [m]
  (let [{:keys [file line column]} m]
    (str
     (reverse (map #(:name (meta %)) (:testing-vars (get-current-env))))
     " (" file ":" line (when column (str ":" column)) ")")))

(defn testing-contexts-str
  "Returns a string representation of the current test context. Joins
  strings in :testing-contexts with spaces."
  []
  (apply str (interpose " " (reverse (:testing-contexts (get-current-env))))))

(defn inc-report-counter!
  "Increments the named counter in :report-counters of the current
  environment. Does nothing if it has no :report-counters."
  [name]
  (when (:report-counters (get-current-env))
    (update-current-env! [:report-counters name] (fnil inc 0))))

(defmulti
  ^{:doc "Generic reporting function, may be overridden to plug in
   different report formats (e.g., TAP, JUnit).  Assertions such as
   'is' call 'report' to indicate results.  The argument given to
   'report' will be a map with a :type key."
    :dynamic true}
  report (fn [m] [(:reporter (get-current-env)) (:type m)]))

(defmethod report :default [m])

(defmethod report [::default :pass] [m]
  (inc-report-counter! :pass))

(defn- print-comparison [m]
  (let [formatter-fn (or (:formatter (get-current-env)) pr-str)]
    (println "expected:" (formatter-fn (:expected m)))
    (println "  actual:" (formatter-fn (:actual m)))))

(defmethod report [::default :fail] [m]
  (inc-report-counter! :fail)
  (println "\nFAIL in" (testing-vars-str m))
  (when (seq (:testing-contexts (get-current-env)))
    (println (testing-contexts-str)))
  (when-let [message (:message m)] (println message))
  (print-comparison m))

(defmethod report [::default :error] [m]
  (inc-report-counter! :error)
  (println "\nERROR in" (testing-vars-str m))
  (when (seq (:testing-contexts (get-current-env)))
    (println (testing-contexts-str)))
  (when-let [message (:message m)] (println message))
  (print-comparison m))

(defmethod report [::default :summary] [m]
  (println "\nRan" (:test m) "tests containing"
           (+ (:pass m) (:fail m) (:error m)) "assertions.")
  (println (:fail m) "failures," (:error m) "errors."))

(defmethod report [::default :begin-test-ns] [m]
  (println "\nTesting" (name (:ns m))))

;; Ignore these message types:
(defmethod report [::default :end-test-ns] [m])
(defmethod report [::default :begin-test-var] [m])
(defmethod report [::default :end-test-var] [m])
(defmethod report [::default :end-run-tests] [m])
(defmethod report [::default :end-test-all-vars] [m])
(defmethod report [::default :end-test-vars] [m])

(defn do-report
  "Reports event m. Assertions put their source location (:file, :line
  and :column) in m when they expand, so no stack trace is read here."
  [m]
  (report m))

;; =============================================================================
;; Async

(defprotocol IAsyncTest
  "Marker protocol denoting CPS function to begin asynchronous
  testing.")

(defn async?
  "Returns whether x implements IAsyncTest."
  [x]
  (satisfies? IAsyncTest x))

(defn run-block
  "Invoke all functions in fns with no arguments. A fn can optionally
  return

  an async test - is invoked with a continuation running left fns

  a seq of fns tagged per block - are invoked immediately after fn"
  [fns]
  (when-first [f fns]
    (let [obj (f)]
      (if (async? obj)
        (obj (let [d (delay (run-block (rest fns)))]
               (fn []
                 (if (realized? d)
                   (println "WARNING: Async test called done more than one time.")
                   @d))))
        (recur (cond->> (rest fns)
                 (::block? (meta obj)) (concat obj)))))))

(defn block
  "Tag a seq of fns to be picked up by run-block as injected
  continuation.  See run-block."
  [fns]
  (some-> fns
          (vary-meta assoc ::block? true)))

;; =============================================================================
;; Running Tests

(defn- test-var-block*
  [v t]
  {:pre [(instance? Var v)]}
  [(fn []
     (update-current-env! [:testing-vars] conj v)
     (update-current-env! [:report-counters :test] inc)
     (do-report {:type :begin-test-var :var v})
     (try
       (t)
       (catch :default e
         (case e
           ::async-disabled (throw "Async tests require fixtures to be specified as maps.  Testing aborted.")
           (do-report
            (merge (select-keys (meta v) [:file :line :column])
                   {:type :error
                    :message "Uncaught exception, not in assertion."
                    :expected nil
                    :actual e}))))))
   (fn []
     (do-report {:type :end-test-var :var v})
     (update-current-env! [:testing-vars] rest))])

(defn test-var-block
  "Like test-var, but returns a block for further composition and
  later execution."
  [v]
  (when-let [t (:test (meta v))]
    (test-var-block* v t)))

(defn test-var
  "If v has a function in its :test metadata, calls that function,
  add v to :testing-vars property of env."
  [v]
  (run-block (test-var-block v)))

(defn- default-fixture
  "The default, empty, fixture function.  Just calls its argument.

  NOTE: Incompatible with map fixtures."
  [f]
  (f))

(defn compose-fixtures
  "Composes two fixture functions, creating a new fixture function
  that combines their behavior.

  NOTE: Incompatible with map fixtures."
  [f1 f2]
  (fn [g] (f1 (fn [] (f2 g)))))

(defn join-fixtures
  "Composes a collection of fixtures, in order.  Always returns a valid
  fixture function, even if the collection is empty.

  NOTE: Incompatible with map fixtures."
  [fixtures]
  (reduce compose-fixtures default-fixture fixtures))

(defn- wrap-map-fixtures
  "Wraps block in map-fixtures."
  [map-fixtures block]
  (concat (keep :before map-fixtures)
          block
          (reverse (keep :after map-fixtures))))

(defn- execution-strategy [once each]
  (letfn [(fixtures-type [coll]
            (cond
              (empty? coll) :none
              (every? map? coll) :map
              (every? fn? coll) :fn))
          (fixtures-types []
            (->> (map fixtures-type [once each])
                 (remove #{:none})
                 (distinct)))]
    (let [[type :as types] (fixtures-types)]
      (assert (not-any? nil? types)
              "Fixtures may not be of mixed types")
      (assert (> 2 (count types))
              "fixtures specified in :once and :each must be of the same type")
      ({:map :async :fn :sync} type :async))))

(defn- disable-async [f]
  (fn []
    (let [obj (f)]
      (when (async? obj)
        (throw ::async-disabled))
      obj)))

(defn test-vars-block
  "Like test-vars, but returns a block for further composition and
  later execution."
  [vars]
  (map
   (fn [[ns vars]]
     (fn []
       (block
        (let [env (get-current-env)
              once-fixtures (get-in env [:once-fixtures ns])
              each-fixtures (get-in env [:each-fixtures ns])]
          (case (execution-strategy once-fixtures each-fixtures)
            :async
            (->> vars
                 (filter (comp :test meta))
                 (mapcat (comp (partial wrap-map-fixtures each-fixtures)
                               test-var-block))
                 (wrap-map-fixtures once-fixtures))
            :sync
            (let [each-fixture-fn (join-fixtures each-fixtures)]
              [(fn []
                 ((join-fixtures once-fixtures)
                  (fn []
                    (doseq [v vars]
                      (when-let [t (:test (meta v))]
                        (each-fixture-fn
                         (fn []
                           (run-block
                            (test-var-block* v (disable-async t))))))))))]))))))
   (group-by (comp :ns meta) vars)))

(defn test-vars
  "Groups vars by their namespace and runs test-var on them with
  appropriate fixtures assuming they are present in the current
  testing environment."
  [vars]
  (run-block (concat (test-vars-block vars)
                     [(fn []
                        (report {:type :end-test-vars :vars vars}))])))

(defn test-all-vars-block*
  "The block test-all-vars-block expands to: test the Vars of `interns`,
  the `ns-interns` map of namespace `ns`, that have :test metadata, in
  source order (line, then column), with the fixtures `use-fixtures` defined in it."
  [ns interns]
  (let [env (get-current-env)]
    (concat
     [(fn []
        (when (nil? env)
          (set-env! (empty-env)))
        (when-let [v (get interns 'cljs-test-once-fixtures)]
          (update-current-env! [:once-fixtures] assoc ns @v))
        (when-let [v (get interns 'cljs-test-each-fixtures)]
          (update-current-env! [:each-fixtures] assoc ns @v)))]
     (test-vars-block
      (->> (vals interns)
           (filter (comp :test meta))
           (sort-by (comp (juxt :line :column) meta))))
     [(fn []
        (when (nil? env)
          (clear-env!)))])))

(defn test-ns-block*
  "The block test-ns-block expands to, for namespace `ns` whose
  `ns-interns` map is `interns`."
  [env ns interns]
  [(fn []
     (set-env! env)
     (do-report {:type :begin-test-ns, :ns ns})
     ;; If the namespace has a test-ns-hook function, call that:
     (if-let [hook (get interns 'test-ns-hook)]
       (hook)
       ;; Otherwise, just test every var in the namespace.
       (block (test-all-vars-block* ns interns))))
   (fn []
     (do-report {:type :end-test-ns, :ns ns}))])

(defn successful?
  "Returns true if the given test summary indicates all tests
  were successful, false otherwise."
  [summary]
  (and (zero? (:fail summary 0))
       (zero? (:error summary 0))))

;; =============================================================================
;; Utilities for assertions (run while the macros below expand)

(defn function?
  "Returns true if argument is a symbol that names a function (not a
  macro) where the macro whose &env is menv expands."
  [menv x]
  (and (symbol? x) (= :fn-var (build/classify menv x))))

(defn- location
  "The source location of `form` for a report: its own :file, :line and
  :column metadata, or else the expanding call's in `menv`."
  [menv form]
  (merge (select-keys menv [:file :line :column])
         (select-keys (meta form) [:file :line :end-line :column :end-column])))

(defn- event
  "The report map for an assertion of `form` at `menv`."
  [menv type msg form]
  (merge {:type type :message msg} (location menv form) {:expected (list 'quote form)}))

(defn assert-predicate
  "Returns generic assertion code for any functional predicate.  The
  'expected' argument to 'report' will contains the original form, the
  'actual' argument will contain the form with all its sub-forms
  evaluated.  If the predicate returns false, the 'actual' form will
  be wrapped in (not...)."
  ([msg form] (assert-predicate nil msg form))
  ([menv msg form]
   (let [args (rest form)
         pred (first form)
         values (gensym "values")
         result (gensym "result")]
     `(let [~values (list ~@args)
            ~result (apply ~pred ~values)]
        (if ~result
          (report ~(assoc (event menv :pass msg form) :actual `(cons '~pred ~values)))
          (report ~(assoc (event menv :fail msg form)
                          :actual `(list '~'not (cons '~pred ~values)))))
        ~result))))

(defn assert-any
  "Returns generic assertion code for any test, including macros, host
  method calls, or isolated symbols."
  ([msg form] (assert-any nil msg form))
  ([menv msg form]
   (let [value (gensym "value")]
     `(let [~value ~form]
        (if ~value
          (report ~(assoc (event menv :pass msg form) :actual value))
          (report ~(assoc (event menv :fail msg form) :actual value)))
        ~value))))

;; =============================================================================
;; Assertion Methods

;; You don't call these, but you can add methods to extend the 'is'
;; macro.  These define different kinds of tests, based on the first
;; symbol in the test expression. A method runs while 'is' expands, so
;; it must be declared in a namespace that is loaded at compile time,
;; such as one whose macros the tests use.

(defmulti assert-expr
  (fn [menv msg form]
    (cond
      (nil? form) :always-fail
      (seq? form) (first form)
      :else :default)))

(defmethod assert-expr :always-fail [menv msg form]
  ;; nil test: always fail
  `(report ~(dissoc (event menv :fail msg form) :expected)))

(defmethod assert-expr :default [menv msg form]
  (if (and (sequential? form)
           (function? menv (first form)))
    (assert-predicate menv msg form)
    (assert-any menv msg form)))

(defmethod assert-expr 'instance? [menv msg form]
  ;; Test if x is an instance of y.
  (let [object (gensym "object")
        result (gensym "result")]
    `(let [klass# ~(nth form 1)
           ~object ~(nth form 2)]
       (let [~result (instance? klass# ~object)]
         (if ~result
           (report ~(assoc (event menv :pass msg form) :actual `(type ~object)))
           (report ~(assoc (event menv :fail msg form) :actual `(type ~object))))
         ~result))))

(defmethod assert-expr 'thrown? [menv msg form]
  ;; (is (thrown? c expr))
  ;; Asserts that evaluating expr throws an exception of class c.
  ;; Returns the exception thrown.
  (let [klass (second form)
        body (nthnext form 2)
        e (gensym "e")]
    `(try
       ~@body
       (report ~(assoc (event menv :fail msg form) :actual nil))
       (catch ~klass ~e
         (report ~(assoc (event menv :pass msg form) :actual e))
         ~e))))

(defmethod assert-expr 'thrown-with-msg? [menv msg form]
  ;; (is (thrown-with-msg? c re expr))
  ;; Asserts that evaluating expr throws an exception of class c.
  ;; Also asserts that the message string of the exception matches
  ;; (with re-find) the regular expression re.
  (let [klass (nth form 1)
        re (nth form 2)
        body (nthnext form 3)
        e (gensym "e")]
    `(try
       ~@body
       (report ~(assoc (event menv :fail msg form) :actual nil))
       (catch ~klass ~e
         (let [m# (.-message ~e)]
           (if (re-find ~re m#)
             (report ~(assoc (event menv :pass msg form) :actual e))
             (report ~(assoc (event menv :fail msg form) :actual e))))
         ~e))))

(defmacro try-expr
  "Used by the 'is' macro to catch unexpected exceptions.
  You don't call this."
  [msg form]
  (let [t (gensym "t")]
    `(try
       ~(assert-expr &env msg form)
       (catch :default ~t
         (report ~(assoc (event &env :error msg form) :actual t))))))

;; =============================================================================
;; Assertion Macros

(defmacro is
  "Generic assertion macro.  'form' is any predicate test.
  'msg' is an optional message to attach to the assertion.

  Example: (is (= 4 (+ 2 2)) \"Two plus two should be 4\")

  Special forms:

  (is (thrown? c body)) checks that an instance of c is thrown from
  body, fails if not; then returns the thing thrown.

  (is (thrown-with-msg? c re body)) checks that an instance of c is
  thrown AND that the message on the exception matches (with
  re-find) the regular expression re."
  ([form] `(cljs.test/is ~form nil))
  ([form msg]
   `(try-expr ~msg ~form)))

(defmacro are
  "Checks multiple assertions with a template expression.
  See clojure.template/do-template for an explanation of
  templates.

  Example: (are [x y] (= x y)
                2 (+ 1 1)
                4 (* 2 2))
  Expands to:
           (do (is (= 2 (+ 1 1)))
               (is (= 4 (* 2 2))))

  Note: Every assertion reports the location of the 'are' call."
  [argv expr & args]
  (if (or
       ;; (are [] true) is meaningless but ok
       (and (empty? argv) (empty? args))
       ;; Catch wrong number of args
       (and (pos? (count argv))
            (pos? (count args))
            (zero? (mod (count args) (count argv)))))
    `(clojure.template/do-template ~argv (is ~expr) ~@args)
    (throw (js/Error. "The number of args doesn't match are's argv."))))

(defmacro testing
  "Adds a new string to the list of testing contexts.  May be nested,
  but must occur inside a test function (deftest)."
  ([string & body]
   `(do
      (update-current-env! [:testing-contexts] conj ~string)
      (try
        ~@body
        (finally
          (update-current-env! [:testing-contexts] rest))))))

;; =============================================================================
;; Defining Tests

(defmacro deftest
  "Defines a test function with no arguments.  Test functions may call
  other tests, so tests may be composed.  If you compose tests, you
  should also define a function named test-ns-hook; run-tests will
  call test-ns-hook instead of testing all vars.

  If the test is marked ^:async, the body of the test is emitted as a
  host async function with an implicit try/catch. As with synchronous
  test cases, uncaught exceptions will be reported as failures.

  Note: Actually, the test body goes in the :test metadata on the var,
  and the real function (the value of the var) calls test-var on
  itself.

  When the build's :load-tests is false (release builds' default),
  deftest expands to nil and defines nothing."
  [name & body]
  (when (not= false (build/config :light-cljs/load-tests))
    (let [body (if (:async (meta name))
                 (let [done-sym (gensym "done")
                       e (gensym "e")]
                   [`(cljs.test/async ~done-sym
                                      (try ~@body
                                           (catch :default ~e
                                             (cljs.test/do-report
                                              ~(assoc (select-keys &env [:file :line :column])
                                                      :type :error
                                                      :message "Uncaught exception, not in assertion."
                                                      :expected nil
                                                      :actual e)))
                                           (finally (~done-sym))))])
                 body)]
      `(def ~(vary-meta name assoc :test `(fn [] ~@body))
         (fn [] (cljs.test/test-var (var ~name)))))))

(defmacro async
  "Wraps body as a CPS function that can be returned from a test to
  continue asynchronously.  Binds done to a function that must be
  invoked once and from an async context after any assertions.

  (deftest example-with-timeout
    (async done
      (js/setTimeout (fn []
                       ;; make assertions in async context...
                       (done) ;; ...then call done
                       )
                     0)))

  The body may await. An exception it throws, or a rejection it awaits,
  is reported as an error at the async form, and the test continues as
  if done had been called (unless it already was)."
  [done & body]
  (let [continue (gensym "continue")
        called (gensym "called")
        e (gensym "e")]
    `(reify
       cljs.test/IAsyncTest
       cljs.core/IFn
       (~'-invoke [_# ~continue]
        (let [~called (cljs.core/volatile! false)
              ~done (fn [] (cljs.core/vreset! ~called true) (~continue))]
          (.catch ((^:async fn [] ~@body))
                  (fn [~e]
                    (cljs.test/do-report
                     ~(assoc (location &env &form)
                             :type :error
                             :message "Uncaught exception, not in assertion."
                             :expected nil
                             :actual e))
                    (when-not (deref ~called) (~done)))))))))

;; =============================================================================
;; Running Tests

(defn ns?
  "Whether macro argument x is a quoted form, as a quoted namespace
  symbol is."
  [x]
  (and (seq? x) (= (first x) 'quote)))

(defn- quoted-namespace
  [form what]
  (let [[quote ns] (when (seq? form) form)]
    (when-not (and (= quote 'quote) (symbol? ns) (= 2 (count form)))
      (throw (js/Error. (str "Argument to " what " must be a quoted symbol, got "
                             (pr-str form)))))
    ns))

(defmacro run-tests-block
  "Like test-vars, but returns a block for further composition and
  later execution."
  [env-or-ns & namespaces]
  (doseq [ns namespaces]
    (quoted-namespace ns "run-tests"))
  (let [is-ns (ns? env-or-ns)
        env (gensym "env")
        summary (gensym "summary")]
    `(let [~env ~(if is-ns
                   `(empty-env)
                   env-or-ns)
           ~summary (cljs.core/volatile!
                     {:test 0 :pass 0 :fail 0 :error 0
                      :type :summary})]
       (concat ~@(map
                  (fn [ns]
                    `(concat (test-ns-block ~env ~ns)
                             [(fn []
                                (cljs.core/vswap!
                                 ~summary
                                 (partial merge-with +)
                                 (:report-counters
                                  (get-and-clear-env!))))]))
                  (if is-ns
                    (concat [env-or-ns] namespaces)
                    namespaces))
               [(fn []
                  (set-env! ~env)
                  (do-report (deref ~summary))
                  (report (assoc (deref ~summary) :type :end-run-tests))
                  (clear-env!))]))))

(defmacro run-tests
  "Runs all tests in the given namespaces; prints results.
  Defaults to current namespace if none given. Does not return a meaningful
  value due to the possiblity of asynchronous execution. To detect test
  completion add a :end-run-tests method case to the cljs.test/report
  multimethod."
  ([] `(run-tests (empty-env) '~(:name (:ns &env))))
  ([env-or-ns]
   (if (ns? env-or-ns)
     `(run-tests (empty-env) ~env-or-ns)
     `(run-tests ~env-or-ns '~(:name (:ns &env)))))
  ([env-or-ns & namespaces]
   `(run-block (run-tests-block ~env-or-ns ~@namespaces))))

(defmacro run-all-tests
  "Runs all tests in all namespaces; prints results.
  Optional argument is a regular expression; only namespaces with
  names matching the regular expression (with re-matches) will be
  tested. The namespaces are those loaded where the call expands,
  except cljs.core, which defines no tests."
  ([] `(run-all-tests nil (empty-env)))
  ([re] `(run-all-tests ~re (empty-env)))
  ([re env]
   ;; The list depends on every namespace of the build, so a cached
   ;; expansion is reused only while that inventory is unchanged.
   (build/depend-on! :namespaces)
   (let [pattern (cond
                   (nil? re) nil
                   (regexp? re) re
                   :else (throw (js/Error. (str "run-all-tests takes a regular expression literal, got "
                                                (pr-str re)))))]
     `(run-tests ~env
                 ~@(->> (all-ns)
                        (map ns-name)
                        (remove #{'cljs.core})
                        (filter #(or (nil? pattern) (re-matches pattern (name %))))
                        (map (fn [ns] `(quote ~ns))))))))

(defmacro test-all-vars-block
  "A block that tests every Var with :test metadata interned in the
  quoted namespace, with fixtures."
  [form]
  (quoted-namespace form "test-all-vars")
  `(test-all-vars-block* ~form (ns-interns ~form)))

(defmacro test-all-vars
  "Calls test-vars on every var with :test metadata interned in the
  namespace, with fixtures."
  [form]
  `(run-block
    (concat (test-all-vars-block ~form)
            [(fn []
               (report {:type :end-test-all-vars :ns ~form}))])))

(defmacro test-ns-block
  "Like test-ns, but returns a block for further composition and
  later execution.  Does not clear the current env."
  [env form]
  (quoted-namespace form "test-ns")
  `(test-ns-block* ~env ~form (ns-interns ~form)))

(defmacro test-ns
  "If the namespace defines a function named test-ns-hook, calls that.
  Otherwise, calls test-all-vars on the namespace.  'ns' is a
  quoted namespace symbol."
  ([ns] `(test-ns (empty-env) ~ns))
  ([env form]
   `(run-block
     (concat (test-ns-block ~env ~form)
             [(fn []
                (clear-env!))]))))

(defn- target-namespace
  "The namespace that symbol `sym` names a Var of where the macro whose
  &env is `env` expands, as light.build/classify resolves it."
  [env sym]
  (let [ns-env (:ns env)
        caller (:name ns-env)]
    (if-let [qualifier (namespace sym)]
      (let [target (get (:requires ns-env) (symbol qualifier) (symbol qualifier))]
        (if (= target 'clojure.core) 'cljs.core target))
      (or (when (not= :unresolved (build/classify env (symbol (str caller) (name sym))))
            caller)
          (some (fn [[local target _]] (when (= local (name sym)) (symbol target)))
                (:refers (:light-cljs/reader-context env)))
          caller))))

(defmacro run-test
  "Runs a single test.

  Because the intent is to run a single test, there is no check for the namespace test-ns-hook."
  [test-symbol]
  (if (contains? #{:fn-var :var} (build/classify &env test-symbol))
    (let [ns (target-namespace &env test-symbol)
          fixtures (fn [kind sym]
                     (let [qualified (symbol (str ns) sym)]
                       (when (not= :unresolved (build/classify &env qualified))
                         `(update-current-env! [~kind] assoc '~ns ~qualified))))]
      `(let [v# (var ~test-symbol)
             env# (get-current-env)]
         (if (:test (meta v#))
           (run-block
            (concat
             [(fn []
                (when (nil? env#)
                  (set-env! (empty-env)))
                ~(fixtures :once-fixtures "cljs-test-once-fixtures")
                ~(fixtures :each-fixtures "cljs-test-each-fixtures"))]
             (test-vars-block [v#])
             [(fn []
                (when (nil? env#)
                  (clear-env!)))]))
           (binding [*print-fn* *print-err-fn*]
             (println ~(str test-symbol) "is not a test")))))
    `(binding [*print-fn* *print-err-fn*]
       (println "Unable to resolve" ~(str test-symbol) "to a test function."))))

;; =============================================================================
;; Fixtures

(defmacro use-fixtures
  "Attaches fixtures to the current namespace: :once fixtures run once
  around all of its tests, :each fixtures around each test."
  [type & fns]
  (case type
    :once
    `(def ~'cljs-test-once-fixtures
       [~@fns])
    :each
    `(def ~'cljs-test-each-fixtures
       [~@fns])
    (throw
     (js/Error. "First argument to cljs.test/use-fixtures must be :once or :each"))))
