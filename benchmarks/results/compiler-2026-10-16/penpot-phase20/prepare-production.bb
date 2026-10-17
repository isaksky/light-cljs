(require '[clojure.string :as str])
(def here "/private/tmp/light-cljs-phase20/")
(defn unprofile [text]
  (str/replace text #"(?m)^.*crate::profile::count.*\n" ""))
(defn replace-checked [text before after]
  (assert (str/includes? text before) before)
  (str/replace text before after))
(assert (= (slurp "src/form_bridge.rs") (unprofile (slurp (str here "bridge-before.rs")))))
(spit (str here "bridge-production.rs")
      (-> (slurp (str here "bridge-after.rs")) unprofile
          (replace-checked "FORM_BRIDGE_VERSION: u32 = 3;"
                           "FORM_BRIDGE_VERSION: u32 = 4;\nconst ENVIRONMENT_SLOTS: usize = 64;\nconst ENVIRONMENT_CHILD_SLOTS: usize = 128;")
          (replace-checked "self.environments.len() < 64" "self.environments.len() < ENVIRONMENT_SLOTS")
          (replace-checked "self.environment_children.len() < 128" "self.environment_children.len() < ENVIRONMENT_CHILD_SLOTS")
          (replace-checked "64 + self.environment_children.len()" "ENVIRONMENT_SLOTS + self.environment_children.len()")))
(defn local-block [text]
  (let [start (str/index-of text "            let local_map = {")
        end (str/index-of text "            c.map(&[" start)]
    (assert (and start end))
    (subs text start end)))
(def before-local (unprofile (local-block (slurp (str here "macro-host-before.rs")))))
(def after-local (unprofile (local-block (slurp (str here "macro-host-after.rs")))))
(spit (str here "macro-host-production.rs")
      (-> (slurp "src/macro_host.rs")
          (replace-checked before-local after-local)
          (replace-checked "struct LocalEnvironment {\n    locals: Vec<LocalDescription>,"
                           "struct LocalEnvironment {\n    locals: Vec<LocalDescription>,\n    // Per binding, including shadowed ones, so extending or leaving a lexical\n    // scope can reuse the unchanged prefix without retaining a map per prefix.\n    rows: Vec<(crate::compiler_data::Datum, crate::compiler_data::Datum)>,")))
(spit (str here "runtime-production.js")
      (-> (slurp "src/js_runtime.js")
          (replace-checked "      const value = values.get(root);"
                           "      const value = values.get(root);\n      // Child references always name the previous completed import. Commit\n      // replacements only now, in the Rust encoder's order, after resolving\n      // every reference (metadata and children can decode in a different order).\n      for (const [slot, descriptor] of root[6] ?? []) environments.set(slot, values.get(descriptor));")
          (replace-checked "return Object.freeze({version: 3, decode, encode,"
                           "return Object.freeze({version: 4, decode, encode,")))
