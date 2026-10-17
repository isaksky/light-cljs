(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as s])
(def project (fs/file (fs/parent *file*) "project"))
(def v8source (fs/file "/private/tmp/light-cljs-phase109/v8"))
(def vendor (fs/file project "vendor/v8"))
(fs/create-dirs vendor)
(doseq [name ["src" "gen" "examples" "tests" "benches"]]
  (fs/copy-tree (fs/file v8source name) (fs/file vendor name) {:replace-existing true}))
(doseq [name ["Cargo.toml" "Cargo.toml.orig" ".cargo_vcs_info.json"]]
  (fs/copy (fs/file v8source name) (fs/file vendor name) {:replace-existing true}))
(spit (fs/file vendor "upstream_build.rs")
  (-> (slurp (fs/file v8source "build.rs"))
      (s/replace-first "fn main()" "pub fn main()")
      (s/replace "println!(\"cargo:rerun-if-changed=.gn\");" "")
      (s/replace "println!(\"cargo:rerun-if-changed=BUILD.gn\");" "")
      (s/replace "println!(\"cargo:rerun-if-changed=src/binding.cc\");" "")
      (s/replace #"(?s)pub fn is_debug\(\) -> bool \{.*?\n\}"
                 "pub fn is_debug() -> bool { env_bool(\"V8_FORCE_DEBUG\") }")))
(fs/copy "/private/tmp/light-cljs-phase109/engine-budget.patch" (fs/file vendor "engine-budget.patch") {:replace-existing true})
;; Force the linker to reject an unpatched archive, whose allocator vtable is
;; incompatible with our extended public header.
(spit (fs/file vendor "engine-budget.patch")
  (str (slurp (fs/file vendor "engine-budget.patch"))
    "--- a/src/binding.cc\n+++ b/src/binding.cc\n@@ -1,1 +1,3 @@\n // Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.\n+// Required by light-cljs's allocator adapter; absent in upstream archives.\n+extern \"C\" void light_v8_backing_store_budget_abi_v1() {}\n"))
(println "Prepared thin vendored bindings")
