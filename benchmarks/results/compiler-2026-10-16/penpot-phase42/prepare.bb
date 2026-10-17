(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io] '[clojure.string :as str])
(def here (fs/parent *file*))
(fs/copy "/private/tmp/light-cljs-phase33/validate.bb" (fs/file here "validate.bb") {:replace-existing true})
(spit (fs/file here "negative.rs")
  "fn main() { let source=\"{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}\"; let p=light_cljs::analyze(source,&light_cljs::parse(source).unwrap()).unwrap(); println!(\"names={} strings={} expected=9/9\", p.named_literals.len(), p.strings.len()); }\n")
(def deps "/private/tmp/light-cljs-phase13-storage-profile/target/release/deps")
;; The shared target directory is subsequently rebuilt. Reproduction requires
;; an explicit phase33 baseline library, not whichever library is newest now.
(def library (or (System/getenv "LIGHT_BASELINE_LIBRARY")
                 (throw (ex-info "Set LIGHT_BASELINE_LIBRARY to a phase33 baseline rlib" {}))))
(spit (fs/file here "negative-library.txt") (str library "\n"))
(p/shell "rustc" "--edition=2024" "-O" (str (fs/file here "negative.rs"))
  "--extern" (str "light_cljs=" library) "-L" (str "dependency=" deps) "-o" (str (fs/file here "negative")))
(with-open [out (io/output-stream (fs/file here "negative.log"))]
  (p/shell {:out out :err :out} (str (fs/file here "negative"))))
(assert (= "names=27 strings=27 expected=9/9" (str/trim (slurp (fs/file here "negative.log")))))
(println (slurp (fs/file here "negative.log")))
