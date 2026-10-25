;; This Source Code Form is subject to the terms of the Mozilla Public
;; License, v. 2.0. If a copy of the MPL was not distributed with this
;; file, You can obtain one at http://mozilla.org/MPL/2.0/.
;; Copyright (c) KALEIDOS SUBSIDIARY SL
;; Reduced from the pinned port's app/common/math.cljc; macro body unchanged.
(ns app.common.math
  (:refer-clojure :exclude [max])
  (:require [clojure.core :as c]))
(defmacro max [& params]
  (if (:ns &env)
    `(js/Math.max ~@params)
    `(c/max ~@params)))
(max 1 2)
