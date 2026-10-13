(ns host.packages)
;; The embedding host owns the package registry. These stand-ins replace the
;; real packages; no package code is read by the compiler.
(set! js/lightCljsPackages
      (js-obj "react" (js-obj "createElement" (fn [tag] [:element tag])
                              "useState" (fn [value] [:state value])
                              "default" (js-obj "version" "stub"))
              "@scope/pkg" (js-obj "default" (fn [x] [:scoped x]) "named" 7)
              "lodash.debounce" (js-obj "default" (fn [f] f) "flush" (fn [] :flushed))
              "some-package/sub" (js-obj "helper" (fn [x] (* 2 x)))))
