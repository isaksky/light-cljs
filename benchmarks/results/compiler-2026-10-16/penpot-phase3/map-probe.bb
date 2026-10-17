(require '[babashka.fs :as fs] '[babashka.process :as p])
(def binary (or (first *command-line-args*) "/private/tmp/light-cljs-phase3/light-cljs"))
(doseq [[label keys] [["numbers" "Array.from({length:20000}, (_,i)=>i)"]
                      ["objects" "Array.from({length:20000}, ()=>({}))"]]]
  (let [js (str "(()=>{const keys=" keys "; const m=new Map(); const start=Date.now(); for(let i=0;i<keys.length;i++)m.set(keys[i],i); let total=0; for(let r=0;r<10;r++)for(let i=0;i<keys.length;i++)total+=m.get(keys[i]); return JSON.stringify({kind:'" label "',ms:Date.now()-start,total,size:m.size});})()")
        source (str "(js* " (pr-str js) ")")
        result (p/shell {:out :string :err :string :continue true} binary "--eval" "--timeout-ms" "120000" "--memory-limit" "512000000" "--poll-budget" "1000000000" "--text" source)]
    (assert (zero? (:exit result)) (:err result))
    (print (:out result)) (flush)))
