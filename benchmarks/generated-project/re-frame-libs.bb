#!/usr/bin/env bb
;; Makes the library sources the re-frame shape vendors (generate.bb --shape re-frame --libs DIR):
;;   bb benchmarks/generated-project/re-frame-libs.bb [OUT] [--re-frame DIR] [--porter DIR]
;; OUT (default research/shadow-credibility/re-frame-libs) gets
;;   shadow/re-frame  re-frame's own sources, unmodified (reagent comes from Maven)
;;   light/           the light-cljs port of re-frame and reagent 1.2.0, made by the
;;                    porter (research/shadow-port-tool) from a small probe app that uses
;;                    the same library API as the generated apps, plus the edits below
;;   libs.edn         versions and every edit with its cause
;; Needs the Clojure CLI, Java, Node and npm; the probe is built in a temporary directory.
(ns generated-project.re-frame-libs
  (:require [babashka.fs :as fs]
            [babashka.process :as process]
            [clojure.pprint :as pprint]
            [clojure.string :as str]))

(def kit (fs/parent (fs/absolutize *file*)))
(def repo (fs/normalize (fs/path kit ".." "..")))

(def probe
  "(ns probe.main
  (:require [reagent.core :as r]
            [reagent.dom.client :as rdc]
            [\"react-dom\" :as rdom]
            [re-frame.alpha :as rf]
            [re-frame.db :as rdb]))

(rf/reg-cofx :probe/clock (fn [cofx _] (assoc cofx :clock 1)))
(rf/reg-fx :probe/log (fn [_] nil))
(rf/reg-event-db ::init (fn [_ _] {:items [] :n 0}))
(rf/reg-event-db ::bump [(rf/path :n) rf/trim-v (rf/enrich (fn [db _] db))
                         (rf/after (fn [_ _] nil))] (fn [n [x]] (+ n x)))
(rf/reg-event-fx ::add [(rf/inject-cofx :probe/clock)
                        (rf/->interceptor :id ::count :after (fn [ctx] (rf/assoc-effect ctx :db (rf/get-effect ctx :db))))]
                 (fn [{:keys [db clock]} [_ x]] {:db (update db :items conj x) :fx [[:probe/log clock]]}))
(rf/reg-sub ::items (fn [db _] (:items db)))
(rf/reg-sub ::n :<- [::items] (fn [items _] (count items)))
(rf/reg-sub ::both (fn [_ _] [(rf/subscribe [::items]) (rf/subscribe [::n])]) (fn [[a b] _] [a b]))
(defn two [] (let [n (rf/subscribe [::n])] (fn [] [:div.n @n])))
(defn with [] (r/with-let [b (rf/subscribe [::both])] [:i (str @b)]))
(defn three [] (r/create-class {:display-name \"three\"
                                :component-did-mount (fn [this] (rdom/findDOMNode this))
                                :reagent-render (fn [] [:p {:class \"x\"} \"p\"])}))
(defn root [] [:main [two] [with] [three] [:ul (for [x @(rf/subscribe [::items])] ^{:key x} [:li x])]])
(defn main []
  (rf/dispatch-sync [::init])
  (rf/dispatch-sync [::add 1])
  (let [el (.createElement js/document \"div\") root-el (rdc/create-root el)]
    (rdom/flushSync #(rdc/render root-el [root]))
    (rdom/flushSync r/flush)
    (js/console.log (.-innerHTML el) (pr-str @rdb/app-db))))
")

(def edits
  "Edits to the porter's output, each with its cause."
  [{:file "light/vendor/reagent/core.cljs" :from "`(ra/with-let" :to "`(reagent.ratom/with-let"
    :cause "The porter splices reagent/core.clj's with-let macro into core.cljs; its syntax quote names the alias ra, which only the .clj namespace has."}
   {:file "light/vendor/reagent/ratom.cljs" :from "(d/error " :to "(reagent.debug/error "
    :cause "The porter splices reagent/ratom.clj's with-let macro into ratom.cljs; its syntax quote names the alias d, which only the .clj namespace has."}
   {:file "light/compat/goog.cljs" :from "(defn typeOf [x] (js/typeof x))"
    :to "(defn typeOf [x]\n  (cond (nil? x) \"null\" (array? x) \"array\" (fn? x) \"function\" (string? x) \"string\"\n        (number? x) \"number\" (boolean? x) \"boolean\" :else \"object\"))"
    :cause "The compat shim calls js/typeof as a function, which throws a ReferenceError; reagent.impl.util calls goog/typeOf while rendering."}
   {:file "light/compat/goog/async/nextTick.cljs" :from nil
    :to "(ns goog.async.nextTick\n  \"re-frame.interop reads goog.async.nextTick as a JavaScript global.\")\n\n(when-not (exists? js/goog) (set! (.-goog js/globalThis) #js {}))\n(when-not (.-async js/goog) (set! (.-async js/goog) #js {}))\n(set! (.-nextTick (.-async js/goog)) (fn [f] (js/setTimeout f 0)))\n"
    :cause "re-frame.interop reads goog.async.nextTick as a JavaScript global and light-cljs has no Closure Library; re-frame's router uses it for queued dispatches."}])

(defn sh! [dir & cmd]
  (let [r (apply process/sh {:dir (str dir)} cmd)]
    (when-not (zero? (:exit r))
      (throw (ex-info (str "Command failed: " (str/join " " cmd)) {:err (subs (:err r) (max 0 (- (count (:err r)) 3000)))})))
    r))

(defn git-head [dir] (str/trim (:out (sh! dir "git" "rev-parse" "HEAD"))))

(defn shown [path] (let [p (fs/absolutize path)] (if (str/starts-with? (str p) (str repo)) (str (fs/relativize repo p)) (str p))))

(defn main [args]
  (let [opts (loop [[a & more] args o {}]
               (case a
                 nil o
                 "--re-frame" (recur (rest more) (assoc o :re-frame (first more)))
                 "--porter" (recur (rest more) (assoc o :porter (first more)))
                 (recur more (assoc o :out a))))
        out (fs/absolutize (or (:out opts) (fs/path repo "research" "shadow-credibility" "re-frame-libs")))
        re-frame (fs/absolutize (or (:re-frame opts) (fs/path repo "research" "re-frame")))
        porter (fs/absolutize (or (:porter opts) (fs/path repo "research" "shadow-port-tool")))
        commit (git-head re-frame)
        work (fs/create-temp-dir {:prefix "re-frame-libs-"})
        dir (fs/path work "probe")]
    (when (fs/exists? out) (throw (ex-info (str out " exists; remove it first") {})))
    (try
      (fs/create-dirs (fs/path dir "src" "probe"))
      (fs/copy-tree (fs/path re-frame "src") (fs/path dir "lib"))
      (spit (str (fs/path dir "src" "probe" "main.cljs")) probe)
      (spit (str (fs/path dir "package.json"))
            "{\"private\":true,\"dependencies\":{\"react\":\"18.2.0\",\"react-dom\":\"18.2.0\"}}\n")
      (spit (str (fs/path dir "deps.edn"))
            (pr-str {:paths ["src" "lib"]
                     :deps {'thheller/shadow-cljs {:mvn/version "3.5.3"} 'reagent/reagent {:mvn/version "1.2.0"}
                            'net.cgrand/macrovich {:mvn/version "0.2.1"} 'org.clojure/tools.logging {:mvn/version "1.2.4"}}
                     :aliases {:shadow {:main-opts ["-m" "shadow.cljs.devtools.cli"]}
                               :port {:extra-deps {'light-cljs/port {:local/root (str porter)}}}}}))
      ;; A :browser build, so shadow resolves "react" as an npm package the porter keeps as a string require.
      (spit (str (fs/path dir "shadow-cljs.edn"))
            (pr-str {:deps {:aliases [:shadow]}
                     :builds {:port {:target :browser :output-dir "out" :modules {:main {:init-fn 'probe.main/main}}}}}))
      (println "npm install") (flush)
      (sh! dir "npm" "install" "--no-audit" "--no-fund")
      (println "porting with" (str porter)) (flush)
      (sh! dir "clojure" "-M:shadow:port" "run" "light-cljs.port/main" ":port" "../port")
      (let [port (fs/path work "port")]
        (fs/create-dirs (fs/path out "light"))
        (fs/copy-tree (fs/path port "vendor") (fs/path out "light" "vendor"))
        (fs/copy-tree (fs/path port "compat") (fs/path out "light" "compat"))
        (fs/copy-tree (fs/path port "src" "re_frame") (fs/path out "light" "re-frame" "re_frame"))
        (fs/copy-tree (fs/path re-frame "src" "re_frame") (fs/path out "shadow" "re-frame" "re_frame"))
        (doseq [{:keys [file from to]} edits
                :let [f (fs/path out file)]]
          (if from
            (let [text (slurp (str f))]
              (when-not (= 1 (count (re-seq (re-pattern (java.util.regex.Pattern/quote from)) text)))
                (throw (ex-info (str "Edit target not found once in " file) {:from from})))
              (spit (str f) (str/replace text from to)))
            (do (fs/create-dirs (fs/parent f)) (spit (str f) to))))
        (spit (str (fs/path out "libs.edn"))
              (with-out-str
                (pprint/pprint
                 {:re-frame {:source (shown re-frame) :commit commit}
                  :maven {'reagent/reagent "1.2.0" 'net.cgrand/macrovich "0.2.1" 'org.clojure/tools.logging "1.2.4"}
                  :npm {"react" "18.2.0" "react-dom" "18.2.0"}
                  :porter {:source (shown porter)
                           :port-report (select-keys (read-string (slurp (str (fs/path port "port-report.edn"))))
                                                     [:report])}
                  :edits (mapv #(-> % (dissoc :to :from) (assoc :kind (if (:from %) :replace :add))) edits)})))
        (println "Wrote" (str out)))
      (finally (fs/delete-tree work)))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
