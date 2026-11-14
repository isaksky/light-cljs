#!/usr/bin/env bb
(ns generated-project.re-frame-oracle
  "An interpreter for a re-frame-shaped project's model, independent of its
  CLJS renderer: it replays the seeded event sequence on a model of app-db and
  renders the views' markup as React writes it, then prints the checksums the
  app prints. Worker calls go to the plain shape's oracle."
  (:require [babashka.fs :as fs]
            [clojure.edn :as edn]
            [clojure.string :as str]))

;; In a generated project this file is oracle.bb and the plain oracle is
;; workload-oracle.bb; in the kit they are re-frame-oracle.bb and oracle.bb.
(let [dir (fs/parent (fs/absolutize *file*))
      project (fs/path dir "workload-oracle.bb")]
  (load-file (str (if (fs/exists? project) project (fs/path dir "oracle.bb")))))
(alias 'workload 'generated-project.oracle)

(def modulus 1000003)
(defn add [a b] (mod (+ a b) modulus))
(defn mix [h x] (mod (+ (* h 31) x) modulus))
(defn digest-text [h s] (reduce (fn [h c] (mix h (int c))) (mix h (count s)) s))
(defn digest [h x]
  (cond (nil? x) (mix h 1)
        (true? x) (mix h 2)
        (false? x) (mix h 3)
        (integer? x) (mix (mix h 4) x)
        (string? x) (digest-text (mix h 5) x)
        (keyword? x) (digest-text (mix h 6) (subs (str x) 1))
        (map? x) (reduce (fn [h [k v]] (digest (digest h k) v)) (mix (mix h 8) (count x))
                         (sort-by (fn [[k _]] (str k)) x))
        (sequential? x) (reduce digest (mix (mix h 7) (count x)) x)
        :else (throw (ex-info "digest: unexpected value" {:value x}))))

;;; Events

(defn default-slice [{:keys [fields items]}]
  (merge {:items items :filter :all :next-id (count items)
          :total (reduce add 0 (map :n items)) :events 0 :score 0}
         (into {} (map (fn [{:keys [name init]}] [(keyword name) init]) fields))))

(defn worker-run [model ns x]
  (workload/run-worker x (first (filter #(= ns (:ns %)) (:workers model)))))

(defn event
  "One event of feature F with ARG on state {:db :log :clock}."
  [model state f kind arg]
  (let [k (keyword (:key f))
        s (get-in state [:db k])
        n (count (:items s))
        [s state]
        (case kind
          "add" (let [clock (inc (:clock state))
                      items (if (>= n 8) (subvec (:items s) 1) (:items s))]
                  [(assoc s :items (conj items {:id (:next-id s) :label (str (:prefix f) (mod (+ arg clock) 97))
                                                :n (add (* arg (:salt f)) clock) :done false})
                          :next-id (inc (:next-id s)))
                   (assoc state :clock clock
                          :log (mod (+ (* (:log state) 31) arg (count (:key f))) modulus))])
          "toggle" [(if (pos? n) (update-in s [:items (mod arg n) :done] not) s) state]
          "remove" [(if (> n 1)
                      (let [i (mod arg n) items (:items s)]
                        (assoc s :items (into (subvec items 0 i) (subvec items (inc i)))))
                      s)
                    state]
          "filter" [(assoc s :filter (nth [:all :active :done] (mod arg 3))) state]
          "compute" [(assoc s :score (worker-run model (:worker f)
                                                 (add arg (get s (keyword (:name (first (:fields f))))))))
                     state]
          "rename" [(if (pos? n)
                      (let [i (mod arg n) label (str (mod arg 10) (get-in s [:items i :label]))]
                        (assoc-in s [:items i :label] (subs label 0 (min 10 (count label)))))
                      s)
                    state]
          "clear" [(update s :items #(filterv (complement :done) %)) state]
          (let [field (subs kind 5)
                {fk :k} (first (filter #(= field (:name %)) (:fields f)))]
            [(update s (keyword field) #(add (* fk arg) (inc %))) state]))
        ;; Interceptors: enrich recomputes the total, then the event is counted.
        s (-> s (assoc :total (reduce add 0 (map :n (:items s)))) (update :events inc))]
    (assoc-in state [:db k] s)))

;;; Views, as React's innerHTML shows them

(defn esc [s] (-> (str s) (str/replace "&" "&amp;") (str/replace "<" "&lt;") (str/replace ">" "&gt;")
                  (str/replace "\"" "&quot;")))
(defn el [tag attrs & kids]
  (str "<" tag (str/join (for [[a v] attrs] (str " " a "=\"" (esc v) "\""))) ">"
       (str/join (map #(if (string? %) % (esc %)) kids)) "</" tag ">"))
(defn text [& parts] (str/join (map esc parts)))

(defn visible [s]
  (case (:filter s)
    :all (:items s)
    :done (filterv :done (:items s))
    (filterv (complement :done) (:items s))))
(defn summary [f s] (reduce add (get s (keyword (:name (first (:fields f))))) (map :n (visible s))))

(defn feature-html [db features f]
  (let [s (get db (keyword (:key f)))]
    (el "section" [["id" (str "f-" (:key f))] ["class" "feature"]]
        (el "h2" [] (text (:title f)))
        (str/join (for [{:keys [name]} (:fields f)]
                    (el "div" [["class" "field"]] (text (str name " ") (get s (keyword name))))))
        (el "p" [["class" "summary"]] (text "Summary " (summary f s)))
        (el "div" [["class" "stats"] ["title" (str "score " (:score s))] ["data-mounted" "yes"]]
            (text "Total " (:total s)))
        (el "ul" [["class" "items"]]
            (str/join (for [{:keys [label n done]} (visible s)]
                        (el "li" [["class" (if done "item done" "item")]]
                            (el "span" [["class" "label"]] (text label)) (text " ") (el "b" [] (text n))))))
        (if-some [d (:dep f)]
          (let [g (nth features d)]
            (el "footer" [] (text "Cross " (add (* 3 (summary f s)) (summary g (get db (keyword (:key g))))))))
          ""))))

(defn expected
  "The app's output line for INPUT and ITERATIONS: app-db's checksum and the markup's."
  [model input iterations]
  (when-not (and (= "re-frame" (:shape model)) (seq (:features model)))
    (throw (ex-info "Not a re-frame-shaped model" {})))
  (when-not (and (integer? input) (<= 0 input (dec modulus))
                 (integer? iterations) (<= 1 iterations 1000000))
    (throw (ex-info "INPUT must be 0..1000002; ITERATIONS must be 1..1000000" {})))
  (let [features (:features model)
        n (count features)
        db (into {:core (workload/expected model input 1)}
                 (map (fn [f] [(keyword (:key f)) (default-slice f)]) features))
        final (binding [workload/*model* model]
                (loop [step 0 x (mod (+ (* input 7) iterations) modulus) state {:db db :log 0 :clock 0}]
                  (if (< step (* 3 iterations n))
                    (let [x (mod (+ (* x 7919) 12345) modulus)
                          f (nth features (mod x n))
                          kinds (:events f)]
                      (recur (inc step) x
                             (event model state f (nth kinds (mod (quot x n) (count kinds))) (mod (quot x 7) 997))))
                    state)))
        markup (el "main" [["id" "app-root"]]
                   (el "header" [] (text "Core " (:core (:db final))))
                   (str/join (map #(feature-html (:db final) features %) features)))]
    (str (digest 0 [(:db final) (:log final)]) " " (digest-text 0 markup))))

(defn main [args]
  (when (> (count args) 2)
    (throw (ex-info "Usage: bb oracle.bb [INPUT=1 [ITERATIONS=1]]" {})))
  (let [dir (fs/parent (fs/absolutize *file*))
        model (edn/read-string (slurp (fs/file dir "model.edn")))]
    (println (expected model (if (first args) (parse-long (first args)) 1)
                       (if (second args) (parse-long (second args)) 1)))))

(when (= (fs/absolutize *file*)
         (some-> (System/getProperty "babashka.file") fs/absolutize))
  (main *command-line-args*))
