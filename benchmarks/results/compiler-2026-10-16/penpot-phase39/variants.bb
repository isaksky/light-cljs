(require '[babashka.fs :as fs] '[clojure.string :as str] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def runtime-text (slurp "src/js_runtime.js"))
(def replay-text (slurp "/private/tmp/light-cljs-phase31/replay.js"))
(def anchor "  function setDeclarationDemand(demand) {\n")
(assert (str/includes? runtime-text anchor))
(spit (fs/file here "runtime-demand-upper.js")
  (str/replace runtime-text anchor
    (str anchor "    if (demand === null) { demandProtocol = demandMethod = demandMulti = () => {}; return; }\n")))
(spit (fs/file here "replay-demand-upper.js")
  (str/replace replay-text "const start = Date.now();" "if (i === 1) rt.setDeclarationDemand(null);\nconst start = Date.now();"))
(def begin (str/index-of runtime-text "  const nativeImplementations = new Map("))
(def end (+ (str/index-of runtime-text ";" begin) 1))
(spit (fs/file here "runtime-native-upper.js")
  (str (subs runtime-text 0 end)
    "\n  const nativeMemoUpper = new WeakMap();\n"
    "  let nativeMemoBit = 0;\n"
    "  for (const [protocol, test] of nativeImplementations) {\n"
    "    const index = nativeMemoBit++, word = index >>> 5, bit = 1 << (index & 31);\n"
    "    nativeImplementations.set(protocol, value => {\n"
    "      if (value === null || typeof value !== 'object' && typeof value !== 'function') return test(value);\n"
    "      let memo = nativeMemoUpper.get(value);\n"
    "      if (!memo) nativeMemoUpper.set(value, memo = [0, 0, 0, 0]);\n"
    "      if (memo[word] & bit) return !!(memo[word + 2] & bit);\n"
    "      const result = test(value); memo[word] |= bit; if (result) memo[word + 2] |= bit;\n"
    "      return result;\n"
    "    });\n  }\n"
    (subs runtime-text end)))
(doseq [[name rt replay] [["demand-upper" "runtime-demand-upper.js" "replay-demand-upper.js"]
                        ["native-upper" "runtime-native-upper.js" nil]
                        ["baseline" "runtime.js" nil]]]
  (println "Replay diagnostic" name) (flush)
  (with-open [out (io/output-stream (fs/file here (str name ".log")))]
    (p/shell {:out out :err :out} "/usr/bin/time" "-l" "/private/tmp/light-cljs-phase38/probe-before"
      (str (fs/file here rt)) (if replay (str (fs/file here replay)) "/private/tmp/light-cljs-phase31/replay.js"))))
