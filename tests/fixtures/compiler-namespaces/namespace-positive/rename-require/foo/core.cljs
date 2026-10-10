;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

(ns foo.core
  (:require [clojure.set :as set :refer [intersection] :rename {intersection foo}]))

(defn run [observe]
  (assert (fn? foo))
  (assert (identical? foo set/intersection))
  (assert (= #{:b} (foo #{:a :b} #{:b :c})))
  (assert (identical? foo clojure.set/intersection))
  true)
run
