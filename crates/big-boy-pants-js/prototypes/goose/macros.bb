#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn] '[clojure.string :as str] '[cheshire.core :as json])
(import '[java.nio ByteBuffer ByteOrder])
(def directory (fs/parent (fs/canonicalize *file*)))
(def goose (or (System/getenv "GOOSE") "goose"))
(assert (#{[] ["--measure"]} (vec *command-line-args*)) "Usage: bb macros.bb [--measure]")
(defn run! [& args] (:out (apply p/shell {:dir (str directory) :out :string} args)))
(def tags {:nil 0 :boolean 1 :integer 2 :double 2 :string 3 :symbol 4 :keyword 5 :list 6 :vector 7 :map 8 :set 9})
(def origin-tags {"Source" 0 "Metadata" 1 "SuppliedData" 2 "Quote" 3 "SyntaxQuote" 4 "Unquote" 5 "UnquoteSplicing" 6 "Deref" 7 "VarQuote" 8 "AnonymousFunction" 9 "AnonymousArgument" 10})
(def fields [:start :count :ns-start :ns-count :metadata :origin :source :span-start :span-end])
(def defaults {:tag 0 :number 0 :start 0 :count 0 :ns-start 0 :ns-count -1 :metadata -1 :origin -1 :source -1 :span-start 0 :span-end 0})
(defn segment [xs [start count]] (subvec xs start (+ start count)))
(defn packet [capture job expected? which]
  (let [g (atom {:nodes [] :children [] :units (get-in capture [:data :units]) :sources [] :bytes [] :roots []})
        data (:data capture) forms (get job (if expected? :expected-output-forms :input-forms)) memo (atom {})]
    (doseq [source (:sources capture)]
      (swap! g (fn [g] (-> g (update :sources conj [(count (:bytes g)) (count (:utf8 source))]) (update :bytes into (:utf8 source))))))
    (letfn [(node! [node] (let [id (count (:nodes @g))] (swap! g update :nodes conj (merge defaults node)) id))
            (children! [ids] (let [start (count (:children @g))] (swap! g update :children into ids) [start (count ids)]))
            (datum! [id]
              (let [n (nth (:nodes data) id) tag (:tag n) tag (if (= tag :collection) (:kind n) tag)
                    [start count] (or (:units n) (:name n) (when (:children n) (children! (mapv datum! (segment (:children data) (:children n))))) [0 0])
                    [ns-start ns-count] (or (:namespace n) [0 -1])]
                (node! {:tag (or (tags tag) (throw (ex-info "Unsupported captured datum" n))) :start start :count count :ns-start ns-start :ns-count ns-count
                        :metadata (if-let [id (:metadata n)] (datum! id) -1)
                        :number (case tag :integer (double (:value n)) :double (Double/longBitsToDouble (unchecked-long (:bits n))) :boolean (if (:value n) 1 0) 0)})))
            (form! [id]
              (if-let [existing (get @memo id)] existing
                (let [n (nth (:nodes forms) id) scalar? (= :scalar (:kind n))
                      result (if scalar? (datum! (:datum n))
                               (let [[start count] (children! (mapv form! (segment (:children forms) (:children n))))]
                                 (node! {:tag (tags (:kind n)) :start start :count count})))
                      meta (if-let [id (:metadata n)] (form! id) -1)]
                  (assert (nil? (:payload n)) "Selected corpus has no compiler literal payload")
                  (swap! memo assoc id result)
                  (swap! g update-in [:nodes result] merge {:metadata meta :source (:source n) :span-start (first (:span n)) :span-end (second (:span n)) :origin (get origin-tags (:origin n) -1)}) result)))]
      (let [call (form! (:root forms))]
        (if expected? (swap! g assoc :roots [call])
          (let [environment (datum! (:environment job))
                input-root (nth (:nodes forms) (:root forms))
                args (mapv form! (rest (segment (:children forms) (:children input-root))))
                [start count] (children! args) argv (node! {:tag 7 :start start :count count}) macro-id (node! {:tag 2 :number which})]
            (swap! g assoc :roots [call environment argv macro-id]))))) @g))
(defn encode [g]
  (let [buffer (doto (ByteBuffer/allocate (+ 32 (* 48 (count (:nodes g))) (* 4 (count (:children g))) (* 2 (count (:units g))) (* 8 (count (:sources g))) (count (:bytes g)) (* 4 (count (:roots g))))) (.order ByteOrder/LITTLE_ENDIAN))]
    (.put buffer (.getBytes "GFB1" "UTF-8")) (.putInt buffer 1)
    (doseq [key [:nodes :children :units :sources :bytes :roots]] (.putInt buffer (count (get g key))))
    (doseq [n (:nodes g)] (.putInt buffer (:tag n)) (.putDouble buffer (double (:number n))) (doseq [field fields] (.putInt buffer (int (get n field)))))
    (doseq [id (:children g)] (.putInt buffer id)) (doseq [u (:units g)] (.putShort buffer (unchecked-short u)))
    (doseq [s (:sources g) x s] (.putInt buffer x)) (doseq [b (:bytes g)] (.put buffer (unchecked-byte b)))
    (doseq [id (:roots g)] (.putInt buffer id)) (.array buffer)))
(defn decode [bytes]
  (let [b (doto (ByteBuffer/wrap bytes) (.order ByteOrder/LITTLE_ENDIAN)) magic (byte-array 4)]
    (.get b magic) (assert (= "GFB1" (String. magic "UTF-8"))) (assert (= 1 (.getInt b)))
    (let [[nodes children units sources text roots] (repeatedly 6 #(.getInt b))]
      {:nodes (vec (repeatedly nodes #(merge {:tag (.getInt b) :number (.getDouble b)} (zipmap fields (repeatedly 9 (fn [] (.getInt b)))))))
       :children (vec (repeatedly children #(.getInt b))) :units (vec (repeatedly units #(bit-and 65535 (.getShort b))))
       :sources (vec (repeatedly sources #(vector (.getInt b) (.getInt b)))) :bytes (vec (repeatedly text #(bit-and 255 (.get b)))) :roots (vec (repeatedly roots #(.getInt b)))})))
(defn semantic [g id provenance?]
  (let [n (nth (:nodes g) id) tag (:tag n)
        value (case (long tag) 0 nil 1 (not (zero? (:number n))) 2 (:number n)
                3 (segment (:units g) [(:start n) (:count n)])
                (4 5) [(when (>= (:ns-count n) 0) (segment (:units g) [(:ns-start n) (:ns-count n)])) (segment (:units g) [(:start n) (:count n)])]
                (mapv #(semantic g % provenance?) (segment (:children g) [(:start n) (:count n)])))
        meta (when (>= (:metadata n) 0) (semantic g (:metadata n) provenance?))]
    (cond-> [tag value meta] provenance? (conj [(:origin n) (when (>= (:source n) 0) (hash (segment (:bytes g) (nth (:sources g) (:source n))))) (:span-start n) (:span-end n)]))))

(defn first-difference [a b path] (when (not= a b) (if (and (vector? a) (vector? b) (= (count a) (count b))) (some identity (map-indexed (fn [i x] (first-difference x (nth b i) (conj path i))) a)) [path a b])))
(defn normalize-reference [[tag value metadata]]
  [tag (cond (= tag 2) (double value) (>= tag 6) (mapv normalize-reference value) :else value)
   (when metadata (normalize-reference metadata))])

(def reference-js
  (str/join "\n"
    ["const fs=require('fs'),vm=require('vm'),now=()=>Number(process.hrtime.bigint());const begin=now();"
     "const request=JSON.parse(fs.readFileSync(process.argv[1],'utf8'));const rt=vm.runInThisContext('('+fs.readFileSync(process.argv[2],'utf8')+')');"
     "const macros=request.images.map(p=>vm.runInThisContext('('+fs.readFileSync(p,'utf8')+')')(rt));globalThis.__lightCompilerRoots={};"
     "request.helpers.forEach((p,i)=>globalThis.__lightCompilerRoots[i===0?'cljs.user/left-result':'cljs.user/right-result']=vm.runInThisContext('('+fs.readFileSync(p,'utf8')+')')(rt));const booted=now(),rows=[],values=[];"
     "const text=(g,s,n)=>String.fromCharCode(...g.units.slice(s,s+n)),units=s=>Array.from({length:s.length},(_,i)=>s.charCodeAt(i));"
     "function input(g,id,memo){if(memo.has(id))return memo.get(id);const n=g.nodes[id],children=()=>g.children.slice(n.start,n.start+n.count).map(i=>input(g,i,memo));let v;switch(n.tag){case 0:v=null;break;case 1:v=!!n.number;break;case 2:v=n.number;break;case 3:v=text(g,n.start,n.count);break;case 4:case 5:v=rt[n.tag===4?'symbol':'keyword'](n['ns-count']<0?null:text(g,n['ns-start'],n['ns-count']),text(g,n.start,n.count));break;case 6:v=rt.listFromArray(children());break;case 7:v=rt.vectorFromArray(children());break;case 8:v=rt.mapFromArray(children());break;case 9:v=rt.setFromArray(children());break;default:throw Error('unsupported input');}if(n.metadata>=0)v=rt.core.withMeta(v,input(g,n.metadata,memo));memo.set(id,v);return v;}"
     "function output(v){const n=rt.inspect(v);let tag,value;if(v===null){tag=0;value=null;}else if(typeof v==='boolean'){tag=1;value=v;}else if(typeof v==='number'){tag=2;value=v;}else if(typeof v==='string'){tag=3;value=units(v);}else if(n?.kind==='symbol'||n?.kind==='keyword'){tag=n.kind==='symbol'?4:5;value=[n.namespace===null?null:units(n.namespace),units(n.name)];}else if(n?.kind==='map'){tag=8;value=n.entries.flat().map(output);}else if(n?.items){tag=n.kind==='vector'?7:n.kind==='set'?9:6;value=n.items.map(output);}else throw Error('unsupported result');const m=rt.core.meta(v);return [tag,value,m===null?null:output(m)];}"
     "for(const [index,g] of request.inputs.entries()){const begin=now(),memo=new Map(),args=g.roots.map(i=>input(g,i,memo)),ready=now();const result=macros[args[3]](args[0],args[1],...rt.inspect(args[2]).items),invoked=now();const value=output(result);fs.writeFileSync(process.argv[3]+'/'+index+'.json',JSON.stringify(value));values.push(value);rows.push({import_ns:ready-begin,invoke_ns:invoked-ready,export_ns:now()-invoked});}"
     "console.log(JSON.stringify({boot_ns:booted-begin,rows,values}));"]))

(defn assert-context-aliases! [g]
  (let [children #(segment (:children g) ((juxt :start :count) (nth (:nodes g) %)))
        payload (second (children (first (:roots g))))
        [x call _ meta] (children payload)
        x-children (children x)
        nested-map (last x-children)
        equal-vector (second (children nested-map))
        [a b] (children equal-vector)]
    (assert (= x (second (children call))) "&form's unchanged input subtree must alias returned x")
    (let [metadata (:metadata (nth (:nodes g) call))]
      (if (>= metadata 0) (assert (= meta metadata) "Returned &form metadata must retain its alias")
        (assert (= 0 (:tag (nth (:nodes g) meta))))))
    (assert (not= a b) "Equal scalar values from distinct positions must remain distinct occurrences")
    (assert (= (:number (nth (:nodes g) a)) (:number (nth (:nodes g) b))))))

(let [temporary (fs/create-temp-dir {:dir (str directory) :prefix ".goose-macros-"}) executable (str (fs/path temporary "macro-run"))]
  (try
    (println (str/trim (run! goose "version")))
    (println (str/trim (run! goose "build" (str (fs/path directory "macro_run.goose")) "-o" executable)))
    (doseq [name ["churn" "closure" "branches" "context"]]
      (let [capture (edn/read-string (slurp (str (fs/path directory "corpus" (str name ".pants.jobs.edn")))))
            definitions (:definitions capture)
            packets (mapv (fn [index job]
                            (let [name-units (segment (get-in capture [:data :units]) (get-in job [:macro :name]))
                                  macro-name (apply str (map char name-units))
                                  which (first (keep-indexed #(when (str/ends-with? (:name %2) (str "/" macro-name)) %1) definitions))
                                  input (packet capture job false which) path (str (fs/path temporary (str name "-" index ".gfb")))]
                              (fs/write-bytes path (encode input)) {:path path :input input :expected (packet capture job true which)})) (range) (:jobs capture))
            images (mapv #(str (fs/path directory "corpus" (str/replace (:factory %) #"\.js$" ".gsp"))) definitions)
            helpers (mapv #(str (fs/path directory "corpus" (str name "-helper-" % ".pants.gsp"))) (range (:helper-factories capture)))
            ;; Repeated packets run on the same initialized session/closures.
            arguments (concat images ["--helpers"] helpers ["--jobs"] (map :path packets) (map :path packets))
            output (apply run! executable (str temporary) arguments)
            request-path (str (fs/path temporary (str name ".json")))
            _ (spit request-path (json/generate-string {:images (mapv #(str/replace % #"\.pants\.gsp$" ".v8.js") images) :helpers (mapv #(str/replace % #"\.pants\.gsp$" ".v8.js") helpers) :inputs (mapv :input (concat packets packets))}))
            reference (json/parse-string (run! "node" "-e" reference-js request-path (str (fs/path directory "../../../../src/js_runtime.js")) (str temporary)) true)]
        (println name (str/trim output))
        (doseq [[i p] (map-indexed vector (concat packets packets))]
          (let [actual (decode (fs/read-all-bytes (fs/path temporary (str i ".gfb")))) expected (:expected p)]
            (assert (= (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true))
                    (str name " compiler expansion/provenance differs at call " i " first-difference=" (first-difference (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true) [])))
            (assert (= (semantic expected (first (:roots expected)) false) (normalize-reference (nth (:values reference) i))) "V8 reference differs")
            (when (= name "context") (assert-context-aliases! actual))))
        (let [boot (edn/read-string (first (str/split-lines output)))]
          (assert (= 1 (:boot-count boot))) (assert (= (count definitions) (:initializers boot))) (assert (= (count helpers) (:producers boot))))
        (when (and (= name "churn") (= ["--measure"] (vec *command-line-args*)))
          (doseq [sample (range 5)]
            (let [started (System/nanoTime) output (apply run! executable (str temporary) arguments) wall (- (System/nanoTime) started)]
              (doseq [[i packet] (map-indexed vector (concat packets packets))]
                (let [actual (decode (fs/read-all-bytes (fs/path temporary (str i ".gfb")))) expected (:expected packet)]
                  (assert (= (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true)))))
              (println "diagnostic goose" (inc sample) (str/trim output) "process-wall-ns" wall)
              (let [[boot & rows] (map edn/read-string (str/split-lines output))]
                (println "sample" :goose (inc sample) (+ (:load-ns boot) (:boot-ns boot)) (reduce + (map :import-ns rows)) (reduce + (map :invoke-ns rows)) (reduce + (map :export-ns rows)) wall)))
            (let [started (System/nanoTime) output (run! "node" "-e" reference-js request-path (str (fs/path directory "../../../../src/js_runtime.js")) (str temporary)) wall (- (System/nanoTime) started)
                  measured (json/parse-string output true)]
              (assert (= (:values reference) (:values measured)))
              (println "diagnostic node-v8" (inc sample) (dissoc measured :values) "process-wall-ns" wall)
              (println "sample" :node-v8 (inc sample) (:boot_ns measured) (reduce + (map :import_ns (:rows measured))) (reduce + (map :invoke_ns (:rows measured))) (reduce + (map :export_ns (:rows measured))) wall))))
        (println name "compiler-captured reference expansion/provenance and reused session passed")))
    (finally (fs/delete-tree temporary))))
