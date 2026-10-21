(ns app.other)

(goog-define label "label-plain")

(defn checked
  "The first element of a one-element array holding `x`, which must be
  positive: an assertion and an array access, for the compiler options."
  [x]
  (assert (pos? x) "x must be positive")
  (aget #js [x] 0))

(def other (str label " " (checked 1)))

other
