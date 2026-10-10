;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

(ns ^{:foo bar} weeble.ns {:foo baz}
  (:require-macros [namespace-observer.macros :refer [namespace-symbol-metadata]]))

(defn run [observe]
  ;; Original *cljs-ns* namespace-symbol metadata expectation.
  (assert (= (namespace-symbol-metadata) {:foo 'baz}))
  ;; Original analyzed namespace :name symbol metadata expectation.
  (assert (= (namespace-symbol-metadata) {:foo 'baz}))
  true)
run
