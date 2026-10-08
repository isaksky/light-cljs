;; The closure captures the outer x. Evaluates to 10.
(let* [x 10
       f (fn* [] x)]
  (let* [x 20]
    (f)))
