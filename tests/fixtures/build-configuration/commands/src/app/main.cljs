(ns app.main (:require [app.macros :refer [setting]] [app.util :as util]))

(def root "root-src")

(str root " " (setting :app/flavor) " " (setting :app/size) " " (util/twice "x"))
