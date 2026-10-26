(ns inherit.main (:require [inherit.app :as app] [inherit.macros :refer [next-value]]))
(js/console.log (pr-str [app/x app/s app/t app/y (next-value)]))
