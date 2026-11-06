#!/usr/bin/env bb
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn] '[clojure.string :as str] '[cheshire.core :as json])
(import '[java.nio ByteBuffer ByteOrder])
(def directory (fs/parent (fs/canonicalize *file*)))
(def root (fs/parent directory))
(def goose (or (System/getenv "GOOSE") "goose"))
(def baseline (System/getenv "GOOSE_SERIAL_BASELINE"))
(assert (#{[] ["--measure"] ["--profile"]} (vec *command-line-args*)) "Usage: bb macros.bb [--measure|--profile]")
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
      (swap! g (fn [g] (-> g (update :sources conj [-1 (count (:bytes g)) (count (:utf8 source))]) (update :bytes into (:utf8 source))))))
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
  (let [buffer (doto (ByteBuffer/allocate (+ 32 (* 48 (count (:nodes g))) (* 4 (count (:children g))) (* 2 (count (:units g))) (* 12 (count (:sources g))) (count (:bytes g)) (* 4 (count (:roots g))))) (.order ByteOrder/LITTLE_ENDIAN))]
    (.put buffer (.getBytes "GFB2" "UTF-8")) (.putInt buffer 2)
    (doseq [key [:nodes :children :units :sources :bytes :roots]] (.putInt buffer (count (get g key))))
    (doseq [n (:nodes g)] (.putInt buffer (:tag n)) (.putDouble buffer (double (:number n))) (doseq [field fields] (.putInt buffer (int (get n field)))))
    (doseq [id (:children g)] (.putInt buffer id)) (doseq [u (:units g)] (.putShort buffer (unchecked-short u)))
    (doseq [s (:sources g) x s] (.putInt buffer x)) (doseq [b (:bytes g)] (.put buffer (unchecked-byte b)))
    (doseq [id (:roots g)] (.putInt buffer id)) (.array buffer)))
(defn decode [bytes]
  (let [b (doto (ByteBuffer/wrap bytes) (.order ByteOrder/LITTLE_ENDIAN)) magic (byte-array 4)]
    (.get b magic) (assert (= "GFB2" (String. magic "UTF-8"))) (assert (= 2 (.getInt b)))
    (let [[nodes children units sources text roots] (repeatedly 6 #(.getInt b))]
      {:nodes (vec (repeatedly nodes #(merge {:tag (.getInt b) :number (.getDouble b)} (zipmap fields (repeatedly 9 (fn [] (.getInt b)))))))
       :children (vec (repeatedly children #(.getInt b))) :units (vec (repeatedly units #(bit-and 65535 (.getShort b))))
       :sources (vec (repeatedly sources #(vector (.getInt b) (.getInt b) (.getInt b)))) :bytes (vec (repeatedly text #(bit-and 255 (.get b)))) :roots (vec (repeatedly roots #(.getInt b)))})))
(defn semantic [g id provenance?]
  (let [n (nth (:nodes g) id) tag (:tag n)
        value (case (long tag) 0 nil 1 (not (zero? (:number n))) 2 (:number n)
                3 (segment (:units g) [(:start n) (:count n)])
                (4 5) [(when (>= (:ns-count n) 0) (segment (:units g) [(:ns-start n) (:ns-count n)])) (segment (:units g) [(:start n) (:count n)])]
                (mapv #(semantic g % provenance?) (segment (:children g) [(:start n) (:count n)])))
        meta (when (>= (:metadata n) 0) (semantic g (:metadata n) provenance?))]
    (cond-> [tag value meta] provenance? (conj [(:origin n) (when (>= (:source n) 0) (hash (segment (:bytes g) (rest (nth (:sources g) (:source n)))))) (:span-start n) (:span-end n)]))))

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
    (println (str/trim (run! goose "build" (str (fs/path root "src/macro_run.goose")) "-o" executable)))
    (doseq [name ["churn" "closure" "branches" "context"]]
      (let [capture (edn/read-string (slurp (str (fs/path root "corpus" (str name ".pants.jobs.edn")))))
            definitions (:definitions capture)
            packets (mapv (fn [index job]
                            (let [name-units (segment (get-in capture [:data :units]) (get-in job [:macro :name]))
                                  macro-name (apply str (map char name-units))
                                  which (first (keep-indexed #(when (str/ends-with? (:name %2) (str "/" macro-name)) %1) definitions))
                                  input (packet capture job false which) path (str (fs/path temporary (str name "-" index ".gfb")))]
                              (fs/write-bytes path (encode input)) {:path path :input input :expected (packet capture job true which)})) (range) (:jobs capture))
            images (mapv #(str (fs/path root "corpus" (str/replace (:factory %) #"\.js$" ".gsp"))) definitions)
            helpers (mapv #(str (fs/path root "corpus" (str name "-helper-" % ".pants.gsp"))) (range (:helper-factories capture)))
            ;; Repeated packets run on the same initialized session/closures.
            arguments (concat images ["--helpers"] helpers ["--jobs"] (map :path packets) (map :path packets))
            _ (when baseline
                (apply run! baseline (str temporary) arguments)
                (doseq [[i packet] (map-indexed vector (concat packets packets))]
                  (let [actual (decode (fs/read-all-bytes (fs/path temporary (str i ".gfb")))) expected (:expected packet)]
                    (assert (= (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true)) "Baseline expansion/provenance differs")
                    (when (= name "context") (assert-context-aliases! actual)))))
            output (apply run! executable (str temporary) arguments)
            request-path (str (fs/path temporary (str name ".json")))
            _ (spit request-path (json/generate-string {:images (mapv #(str/replace % #"\.pants\.gsp$" ".v8.js") images) :helpers (mapv #(str/replace % #"\.pants\.gsp$" ".v8.js") helpers) :inputs (mapv :input (concat packets packets))}))
            reference (json/parse-string (run! "node" "-e" reference-js request-path (str (fs/path directory "../../../src/js_runtime.js")) (str temporary)) true)]
        (println name (str/trim output))
        (doseq [[i p] (map-indexed vector (concat packets packets))]
          (let [actual (decode (fs/read-all-bytes (fs/path temporary (str i ".gfb")))) expected (:expected p)]
            (assert (= (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true))
                    (str name " compiler expansion/provenance differs at call " i " first-difference=" (first-difference (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true) [])))
            (assert (= (semantic expected (first (:roots expected)) false) (normalize-reference (nth (:values reference) i))) "V8 reference differs")
            (when (= name "context") (assert-context-aliases! actual))))
        (let [boot (edn/read-string (first (str/split-lines output)))]
          (assert (= 1 (:boot-count boot))) (assert (= (count definitions) (:initializers boot))) (assert (= (count helpers) (:producers boot))))
        (when (and (= name "churn") (#{["--measure"] ["--profile"]} (vec *command-line-args*)))
          (let [repository (str (fs/canonicalize (fs/path directory "../../..")))
                environment (assoc (into {} (System/getenv)) "CARGO_BUILD_JOBS" "3" "LIGHT_CLJS_V8_CACHE" "require" "CARGO_TARGET_DIR" (str (fs/path repository "target")))
                build (:out (p/shell {:dir repository :env environment :out :string} "cargo" "test" "--release" "--locked" "--lib" "--features" "bbp-macros" "serial_macro_reference" "--no-run" "--message-format=json"))
                binary (some #(let [v (json/parse-string % true)] (when (and (= "compiler-artifact" (:reason v)) (get-in v [:profile :test]) (:executable v)) (:executable v))) (str/split-lines build))
                _ (assert binary "Missing reference test executable")
                inputs (vec (concat packets packets packets))
                goose-args (concat images ["--helpers"] helpers ["--warmup"] (map :path packets) ["--jobs"] (map :path (concat packets packets)))
                verify! (fn [] (doseq [[i packet] (map-indexed vector inputs)]
                                  (let [actual (decode (fs/read-all-bytes (fs/path temporary (str i ".gfb")))) expected (:expected packet)]
                                    (assert (= (semantic expected (first (:roots expected)) true) (semantic actual (first (:roots actual)) true))))))
                reference! (fn [engine]
                             (let [target (str (fs/path temporary (str engine ".edn"))) started (System/nanoTime)]
                               (p/shell {:dir repository :env (assoc environment "GOOSE_SERIAL_ENGINE" (clojure.core/name engine) "GOOSE_SERIAL_OUTPUT" target) :out :string}
                                        binary "compiler_bootstrap::goose_capture::serial_macro_reference" "--ignored" "--exact" "--test-threads=1")
                               {:engine engine :process-wall-ns (- (System/nanoTime) started) :rows (mapv edn/read-string (str/split-lines (slurp target)))}))
                goose! (fn [binary diagnostic?]
                         (let [started (System/nanoTime) output (apply run! binary (str temporary) (concat goose-args (when diagnostic? ["--diagnostic"]))) wall (- (System/nanoTime) started)]
                           (verify!) {:engine :goose :process-wall-ns wall :rows (mapv edn/read-string (str/split-lines output))}))]
            (println "Protocol: one execution worker, one complete 0/16/100 warmup batch, then two reused-session batches. Fresh process/session per round. Correctness preflight precedes timing; no profiling in clean runs.")
            (when (= ["--measure"] (vec *command-line-args*))
              (goose! executable false) (reference! :v8) (reference! :pants)
            ;; Rotate first engine, reverse on odd rounds: no engine always follows itself.
            (doseq [round (range 5) engine (nth [[:goose :v8 :pants] [:pants :v8 :goose] [:v8 :goose :pants] [:pants :goose :v8] [:goose :pants :v8]] round)]
              (println "serial-sample" (pr-str (assoc (if (= engine :goose) (goose! executable false) (reference! engine)) :round (inc round)))))
              (when baseline
                (goose! baseline false) (goose! executable false)
                ;; Actual revisions alternate on identical already prepared packets.
                ;; Each fresh session still has its own warmup and two reused batches.
                (doseq [round (range 5) revision (if (even? round) [:baseline :changed] [:changed :baseline])]
                  (println "serial-pair" (pr-str (assoc (goose! (if (= revision :baseline) baseline executable) false)
                                                       :round (inc round) :revision revision))))))
            ;; A separate native build counts explicit record copies/comparisons.
            ;; Clean executable/source and timing rounds contain no such increments.
            (when (= ["--profile"] (vec *command-line-args*))
            (let [probe (fs/path temporary "profile") _ (fs/create-dirs probe)
                  _ (doseq [path (fs/glob directory "*.goose")] (fs/copy path (fs/path probe (fs/file-name path))))
                  vm-path (str (fs/path probe "source_vm.goose"))
                  runner-path (str (fs/path probe "macro_run.goose"))
                  replace! (fn [text from to] (assert (str/includes? text from) (str "Profile anchor changed: " from)) (str/replace text from to))
                  source (slurp vm-path)
                  updates (subs source (str/index-of source "fn form_assoc(") (str/index-of source "fn form_get("))
                  ;; Count each push and every element of each direct append.
                  ;; Reject scratch construction and unknown retained writes.
                  _ (assert (not (re-find #"Value\[|\.resize\(|form_collection\(|copy\(" updates)) "Unexpected assoc/conj scratch path")
                  _ (assert (= 5 (count (re-seq #"heap\.values\.push\(" updates))) "Direct-fill push anchors changed")
                  _ (assert (= 4 (count (re-seq #"\.append\(" updates))) "Direct-fill append anchors changed")
                  counted-updates (reduce (fn [text [anchor count]]
                                            (replace! text anchor (str "profile_heap_values += " count "; profile_direct_values += " count "; " anchor)))
                                          (str/replace updates "heap.values.push(" "profile_heap_values += 1; profile_direct_values += 1; heap.values.push(")
                                          [["heap.values.append(heap.values[n.start..(n.start + at)]);" "at"]
                                           ["heap.values.append(heap.values[(n.start + at + 1)..(n.start + n.count)]);" "n.count - at - 1"]
                                           ["heap.values.append(heap.values[n.start..(n.start + n.count)]);" "n.count"]])
                  vm (-> source
                         (replace! updates counted-updates)
                         (replace! "heap.values.append(children); return allocate" "profile_heap_values += children.len; heap.values.append(children); return allocate")
                         (replace! "recursive fn form_equal(heap: Heap&, a: Value, b: Value, depth: i64 = 0) -> bool {" "recursive fn form_equal(heap: Heap&, a: Value, b: Value, depth: i64 = 0) -> bool { profile_equal += 1;")
                         (replace! "for i in (n.count / 2) { if form_equal" "for i in (n.count / 2) { profile_map_keys += 1; if form_equal"))
                  runner (-> (slurp runner-path)
                             (replace! "let result, steps = invoke" "source::profile_reset(); let result, steps = invoke")
                             (replace! "let visits_before = c.visits;" "source::profile_report(index); let visits_before = c.visits;"))
                  profile-bin (str (fs/path probe "macro-profile"))]
              (spit vm-path (str vm "\nvar profile_heap_values: i64 = 0; var profile_direct_values: i64 = 0; var profile_equal: i64 = 0; var profile_map_keys: i64 = 0;\nfn profile_reset() { profile_heap_values = 0; profile_direct_values = 0; profile_equal = 0; profile_map_keys = 0; }\nfn profile_report(id: i64) { print(\"{:phase :copies :id \", id, \" :heap-values \", profile_heap_values, \" :direct-values \", profile_direct_values, \" :scratch-values 0 :equal-calls \", profile_equal, \" :map-key-probes \", profile_map_keys, \"}\"); }\n"))
              (spit runner-path runner)
              (run! goose "check" runner-path)
              (run! goose "build" runner-path "-o" profile-bin)
              (let [output (apply run! profile-bin (str temporary) (concat goose-args ["--diagnostic"]))
                    rows (mapv edn/read-string (str/split-lines output))
                    copies (filterv #(= :copies (:phase %)) rows)
                    diagnostics (filterv #(= :diagnostic (:phase %)) rows)]
                (verify!)
                (assert (= 9 (count copies) (count diagnostics)))
                (doseq [[copy diagnostic] (map vector copies diagnostics)]
                  (let [input (mod (:id copy) 3)]
                    (assert (= (:heap-values copy) (:new-values diagnostic) (nth [0 456 15450] input)))
                    (assert (= (:direct-values copy) (nth [0 408 15150] input)))
                    (assert (zero? (:scratch-values copy)))))
                (println "serial-profile" (pr-str {:engine :goose :rows rows})))))))
        (println name "compiler-captured reference expansion/provenance and reused session passed")))
    (finally (fs/delete-tree temporary))))
