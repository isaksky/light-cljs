(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def original (slurp "/private/tmp/light-cljs-phase60/project/src/v8_evaluator.rs"))
(defn between [from to] (subs original (str/index-of original from) (str/index-of original to (str/index-of original from))))
(def file (fs/file here "project/src/v8_evaluator.rs"))
(def before (slurp file))
(assert (not (str/includes? before "struct CpuProfile")))
(spit file (-> before
  (str/replace "pub(crate) struct V8Evaluator {" "pub(crate) struct V8Evaluator {\n    profile: Option<CpuProfile>,")
  (str/replace "struct MemoryGuard {" (str (between "struct ProfileClient;" "struct MemoryGuard {") "struct MemoryGuard {"))
  (str/replace "        let context;" (str (between "        let profile = std::env::var_os" "        let context;") "        let context;"))
  (str/replace "            let scope = &mut v8::ContextScope::new(scope, local);" (str (between "            if let Some(profile) = &profile {" "            let scope = &mut v8::ContextScope::new(scope, local);") "            let scope = &mut v8::ContextScope::new(scope, local);"))
  (str/replace "        Ok(Self {\n            context:" "        Ok(Self {\n            profile,\n            context:")
  (str/replace "        if std::env::var_os(\"LIGHT_V8_STATS\").is_some() {" (str (between "        if let Some(profile) = self.profile.take() {" "        if std::env::var_os(\"LIGHT_V8_STATS\").is_some() {") "        if std::env::var_os(\"LIGHT_V8_STATS\").is_some() {"))))
(doseq [file ["run.bb" "summarize.bb" "line-ticks.bb"]]
  (fs/copy (fs/file "/private/tmp/light-cljs-phase60" file) (fs/file here file)))
(let [f (fs/file here "run.bb")]
  (spit f (str/replace (slurp f) "(str (fs/file project \"src/main.rs\")))" "(str (fs/file project \"src/main.rs\")) (str (fs/file project \"build.rs\")))")))
