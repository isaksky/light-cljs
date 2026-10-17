(require '[babashka.fs :as fs] '[clojure.string :as str])
(def profile-dir "/private/tmp/light-cljs-phase13-storage-profile")
(def path (fs/file profile-dir "src/profile.rs"))
(def original (slurp path))
(spit (fs/file profile-dir "profile-before-shared.rs") original)
(assert (not (str/includes? original "shared_program")))
(assert (str/includes? original "pub fn program(&mut self, p: &crate::Program) {"))
(spit path (str (str/replace original
                             "pub fn program(&mut self, p: &crate::Program) {"
                             "pub fn program(&mut self, p: &crate::Program) { self.shared_program(p);")
                "\n" (slurp (fs/file (fs/parent *file*) "shared-storage.rs"))))
(fs/copy (fs/file profile-dir "target/release/light-cljs")
         (fs/file profile-dir "light-cljs-before-shared-profile") {:replace-existing true})
