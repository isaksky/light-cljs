(ns app.common
  "Required by the editor and the reports only, so it is their shared chunk.")

(defn show [label value]
  (js/console.log label (pr-str value)))
