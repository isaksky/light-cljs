;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

(ns cljs.test (:require [namespace-observer.analyzer :as ana]))
(defn vary-meta
  "Returns an object of the same type and value as obj, with
  (apply f (meta obj) args) as its metadata."
  {:added "1.0" :static true}
  [obj f & args]
  (with-meta obj (apply f (meta obj) args)))
(defmacro is
  "Generic assertion macro.  'form' is any predicate test.
  'msg' is an optional message to attach to the assertion.
\u0020\u0020
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

  When cljs.analyzer/*load-tests* is false, deftest is ignored."
  [name & body]
  (let [body (if (:async (meta name))
               (let [done-sym (gensym "done")]
                 [`(cljs.test/async ~done-sym
                                    (try ~@body
                                         (catch :default e#
                                           (cljs.test/do-report
                                             {:type :error
                                              :message "Uncaught exception, not in assertion."
                                              :expected nil
                                              :actual e#}))
                                         (finally (~done-sym))))])
               body)]
    (when ana/*load-tests*
      `(do
         (def ~(vary-meta name assoc :test `(fn [] ~@body))
           (fn [] (cljs.test/test-var (.-cljs$lang$var ~name))))
         (set! (.-cljs$lang$var ~name) (var ~name))))))
