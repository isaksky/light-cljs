;; Reuse the pinned identity/settings-checked expression preflight. The ordinary
;; CLJS compiler dynamically binds these flags while analyzing complete files;
;; defprotocol/deftype expansion may set! them. Bind them here as well so the
;; expression preflight cannot invent protocol source rejections.
(require '[cljs.analyzer :as sci-ana])
(binding [sci-ana/*unchecked-if* false
          sci-ana/*unchecked-arrays* false]
  (load-file "tests/reference/execution.clj"))
