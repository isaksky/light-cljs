;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns interop.main (:refer-global :only [String Object]))
(defn run [observe]
  (assert (= ["FOO" "BAR" "BAZ"]
             (map String/.toUpperCase ["foo" "bar" "baz"])))
  (let [f (fn [] String/.toUpperCase) m (f)]
    (assert (= "FOO" (m "foo"))))
  (let [f Object/new] (assert (some? (f))))
  true)
run
