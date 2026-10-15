(ns npm.bundle
  (:require ["react" :as r :refer [useState] :rename {useState use-state}]
            ["@scope/pkg" :default Scoped]
            ["lodash.debounce" :as debounce]
            ["some-package/sub" :as sub]
            ["only-in-removed-code" :as removed]))

;; Application preparation removes this unused definition, so neither
;; react's unusedExport nor only-in-removed-code is imported.
(defn unused [] [(r/unusedExport) (removed/anything)])

;; The alias object escapes as a value, so its namespace object is imported.
(defn describe [package] (.-label package))

;; The entry's value, and what npm.split reads when it loads this namespace
;; as a feature.
(def line
  (str (r/createElement "div") " " (use-state 1) " " (Scoped 2) " "
       (describe debounce) " " (sub/helper 21) " " (exists? debounce/missingExport)))

line
