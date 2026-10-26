(ns options-use (:require [reader-macros :refer [missing-readers-probe]]))
(js/console.log (pr-str (missing-readers-probe nil {} {:eof :done})))
