(require '[babashka.fs :as fs] '[cheshire.core :as json] '[clojure.string :as str])
(def here (fs/parent *file*))
(def capture-text (slurp (fs/file here "capture.json")))
(def registry (slurp (fs/file here "compiler_roots.js")))
(def record (slurp (fs/file here "record.js")))
(def clean-fn (subs record (.indexOf record "function cleanCaptureDescriptor") (.indexOf record "function captureValue")))
(def expression
  (str "(() => {\nconst capture = " capture-text ";\n"
       "globalThis.__lightRuntimeForCompiler = rt;\n" registry "\n"
       "globalThis.__lightCompileUnit = id => { const source = capture.units[String(id)]; if (source === undefined) throw Error('Missing captured unit ' + id); return source; };\n"
       "for (const source of capture.sources) (0, eval)(source)(rt);\n"
       "globalThis.__restoreCaptureFrontiers(...capture.frontiers);\n"
       "const fn = (0, eval)(capture.functionSource)(rt);\n"
       "const args = capture.arguments.map(d => rt.compilerForms.decode(d));\n"
       clean-fn "\n"
       "const trials = [];\nfor (let i = 0; i < 2; i++) {\n"
       "const start = Date.now(); const value = Reflect.apply(fn, null, args); const elapsed = Date.now() - start;\n"
       "const actual = cleanCaptureDescriptor(rt.compilerForms.encode(value, 4000000000, 128));\n"
       "if (JSON.stringify(actual) !== JSON.stringify(capture.expected)) throw Error('Replay output mismatch');\n"
       "trials.push({milliseconds: elapsed, equivalent: true});\n}\n"
       "return JSON.stringify({recordedMilliseconds: capture.milliseconds, trials});\n})()"))
(spit (fs/file here "replay.js") expression)
(spit (fs/file here "replay-node.cjs")
      (str "const rt = " (slurp "src/js_runtime.js") ";\nconsole.log(" expression ");\n"))
(println "Prepared QuickJS and Node replay scripts")
