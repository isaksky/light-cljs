;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

; Complete pinned array-setting body; assertion adapter only.
(ns cljs.array-access-test
  (:require [cljs.array-access.alpha :as alpha]))

(defn run [observe]
(let [array-settings-failures (atom [])
      array-settings-check (fn [identity value & message]
        (when-not value (swap! array-settings-failures conj identity)) value)]
(do (array-settings-check "src/test/cljs/cljs/array_access_test.cljs::cljs-2861-test::assertion-001" (false? *unchecked-arrays*)))
(if (empty? @array-settings-failures) true @array-settings-failures))
)
run
