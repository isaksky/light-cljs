(ns admission.app
  (:require [admission.record] [admission.util]))

;; These declarations used to fail before their macro bodies ever ran.
(js/console.log 42)
