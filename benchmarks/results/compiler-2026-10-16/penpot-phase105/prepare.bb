(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file here "project"))
(when-not (fs/exists? project)
  (p/shell "cp" "-a" "/private/tmp/light-cljs-phase102/project" (str project)))
(def original (slurp "/private/tmp/light-cljs-phase102/project/src/js_runtime.js"))
(def start (str/index-of original "    function encode(value, limit, depthLimit, unseen = null) {"))
(def end (str/index-of original "    return Object.freeze({version: 5" start))
(assert (and start end))
(def body (atom (subs original start end)))
(defn replace! [before after]
  (assert (str/includes? @body before) before)
  (swap! body str/replace before after))
(replace! "visit(e.key, path + \".key[\" + i + \"]\", depth + 1)"
          "visit(e.key, \".key\", depth + 1, i)")
(replace! "visit(e.value, path + \".value[\" + i++ + \"]\", depth + 1)"
          "visit(e.value, \".value\", depth + 1, i++)")
(replace! "path + \"[\" + i + \"]\"" "i")
(replace! "path + \"[\" + a.length + \"]\"" "a.length")
(doseq [suffix [".payload" ".meta" ".key"]]
  (replace! (str "path + \"" suffix "\"") (str "\"" suffix "\"")))
(replace! "path + \".\" + key" "\".\" + key")
(swap! body str/replace #"fail\(([^,\n]+), path\)" "fail($1)")
(swap! body str/replace "fail(" "failAt(")
(replace!
  (str/join "\n" ["      let remaining = limit;"
    "      const active = new WeakSet();"
    "      function visit(v, path, depth) {"
    "        if (--remaining < 0) failAt(\"node limit\");"
    "        if (depth > depthLimit) failAt(\"depth limit\");"
    "        const kind = typeof v;"
    "        const object = v != null && (kind === \"object\" || kind === \"function\");"
    "        if (object && active.has(v)) failAt(\"cyclic data\");"
    "        if (object) active.add(v);"
    "        try {"])
  (str/join "\n" ["      let remaining = limit, pathLength = 0;"
    "      const active = new WeakSet();"
    "      // Reuse path components; format a diagnostic only when conversion fails."
    "      // A null prototype prevents inherited setters from observing this stack."
    "      const path = {__proto__: null};"
    "      const pathStep = (part, index) => part === undefined ? \"\" : typeof part === \"number\""
    "        ? \"[\" + part + \"]\" : part + (index === undefined ? \"\" : \"[\" + index + \"]\");"
    "      function failAt(category, part, index) {"
    "        let text = \"$\";"
    "        for (let i = 0; i < pathLength; i += 2) text += pathStep(path[i], path[i + 1]);"
    "        fail(category, text + pathStep(part, index));"
    "      }"
    "      function visit(v, part, depth, index) {"
    "        const at = pathLength; path[at] = part; path[at + 1] = index; pathLength += 2;"
    "        const kind = typeof v;"
    "        const object = v != null && (kind === \"object\" || kind === \"function\");"
    "        let entered = false;"
    "        try {"
    "          if (--remaining < 0) failAt(\"node limit\");"
    "          if (depth > depthLimit) failAt(\"depth limit\");"
    "          if (object && active.has(v)) failAt(\"cyclic data\");"
    "          if (object) { active.add(v); entered = true; }"]))
(replace! "} finally { if (object) active.delete(v); }"
          "} finally { if (entered) active.delete(v); pathLength = at; }")
(replace! "return visit(value, \"$\", 0);" "return visit(value, undefined, 0);")
(assert (not (str/includes? @body "path +")))
(spit (fs/file project "src/js_runtime.js") (str (subs original 0 start) @body (subs original end)))
(spit (fs/file here "build-only.bb")
  (str/replace (slurp "/private/tmp/light-cljs-phase102/build-only.bb")
    "light-cljs-v8-lexical-js-vars" "light-cljs-lazy-error-paths"))
(spit (fs/file here "replay-differential.js")
  (str/replace
    (slurp "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase61/replay-differential.js")
    "/private/tmp/light-cljs-phase58/project" "/private/tmp/light-cljs-phase102/project"))
