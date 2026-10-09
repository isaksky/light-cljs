(ns report.main
  (:require [report.data :as data]
            [report.metrics :as metrics]))

(def result (metrics/checksum (metrics/totals data/rows)))
result
