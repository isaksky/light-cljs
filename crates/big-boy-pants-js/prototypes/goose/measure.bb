#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])

;; Small serial source diagnostic. No allocation profiler, worker scaling, or
;; macro timing. Each process loads once, warms 100 calls, then checks 1000.
(def directory (fs/parent (fs/canonicalize *file*)))
(def goose (or (System/getenv "GOOSE") "goose"))
(doseq [tool [goose "node"]]
  (when-not (fs/which tool)
    (binding [*out* *err*] (println (str "Missing required measurement tool: " tool)))
    (System/exit 1)))
(defn run! [& arguments]
  (:out (apply p/shell {:dir (str directory) :out :string} arguments)))
(println (str/trim (run! goose "version")))
(println (str/trim (run! "node" "-p" "JSON.stringify({node:process.version,v8:process.versions.v8,arch:process.arch,platform:process.platform})")))
(let [temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-measure-"})
      executable (str (fs/path temporary "execute"))
      js "const fs=require('fs'),vm=require('vm');const begin=process.hrtime.bigint();const source=fs.readFileSync(process.argv[1],'utf8');const f=new vm.Script('\"use strict\";('+source+')').runInThisContext();const prepared=process.hrtime.bigint();let result;for(let i=0;i<100;i++)result=f();const warmed=process.hrtime.bigint();let checksum=0;for(let i=0;i<1000;i++){result=f();checksum+=result;}const executed=process.hrtime.bigint();console.log('number:'+String(result));console.log('timing:repetitions=1000,checksum='+checksum+',load_boot_ns='+(prepared-begin)+',warmup_ns='+(warmed-prepared)+',execute_dispose_ns='+(executed-warmed)+',total_ns='+(process.hrtime.bigint()-begin));"]
  (try
    (run! goose "build" (str (fs/path directory "execute.goose")) "-o" executable)
    (println "Diagnostic only: one execution thread; 100 warmups then 1000 calls; raw nanoseconds.")
    (println "load_boot includes file I/O/validation or V8 parsing; execution includes job disposal; process wall includes launch/output/session teardown. Export/native build excluded.")
    (doseq [[name expected] [["arithmetic" "27.5"] ["control_flow" "7307"]]
            backend [:goose :v8] sample (range 5)]
      (let [arguments (if (= backend :goose)
                        [executable (str (fs/path directory "corpus/synthetic" (str name ".gsp"))) "1000"]
                        ["node" "-e" js (str (fs/path directory "corpus/synthetic" (str name ".function.js")))])
            before (System/nanoTime)
            output (apply run! arguments)
            elapsed (- (System/nanoTime) before)
            checksum (* 1000 (Double/parseDouble expected))]
        (assert (or (str/starts-with? output (str "number:" expected "\n"))
                    (str/starts-with? output (str "number:" expected ".0\n"))))
        (assert (= checksum (Double/parseDouble (second (re-find #",checksum=([^,]+)," output)))))
        (println name (clojure.core/name backend) (inc sample) (str/trim output) (str "process_wall_ns=" elapsed))))
    (finally (fs/delete-tree temporary))))
