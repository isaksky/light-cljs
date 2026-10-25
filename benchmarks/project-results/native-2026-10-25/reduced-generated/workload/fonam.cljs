;; From seed42/200k: workload.fonam/stipopi and its first consumer.
;; Macro body unchanged; keep one protocol signature and remove dependencies.
(ns workload.fonam)
(defmacro stipopi [pname & sigs]
  `(defprotocol ~pname ~@sigs))
(stipopi Bagrir (mamjalsko [this]))
