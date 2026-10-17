(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(def before (slurp "src/emitter.rs"))
(def helper
"// Search the concatenated UTF-16 table without allocating its UTF-8 copy.
// The candidate is ASCII, so this preserves lossy conversion's old answer,
// including matches spanning adjacent strings and overlapping prefixes.
fn unit_parameter_occurs(program: &Program, candidate: &str) -> bool {
    let needle = candidate.as_bytes();
    let mut fallback = vec![0; needle.len()];
    for i in 1..needle.len() {
        let mut matched = fallback[i - 1];
        while matched > 0 && needle[i] != needle[matched] {
            matched = fallback[matched - 1];
        }
        if needle[i] == needle[matched] { matched += 1; }
        fallback[i] = matched;
    }
    let mut matched = 0;
    for &unit in program.strings.iter().flatten() {
        while matched > 0 && unit != u16::from(needle[matched]) {
            matched = fallback[matched - 1];
        }
        if unit == u16::from(needle[matched]) {
            matched += 1;
            if matched == needle.len() { return true; }
        }
    }
    false
}

")
(def raw "    let raw = program\n        .strings\n        .iter()\n        .map(|s| String::from_utf16_lossy(s))\n        .collect::<String>();\n")
(assert (str/includes? before raw))
(assert (str/includes? before "if !raw.contains(&candidate) {"))
(def after (-> before
               (str/replace "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {"
                            (str helper "fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {"))
               (str/replace raw "")
               (str/replace "if !raw.contains(&candidate) {" "if !unit_parameter_occurs(program, &candidate) {")))
(spit (fs/file here "before.rs") before)
(spit (fs/file here "after.rs") after)
(spit (fs/file project "src/emitter.rs") after)
(println "Prepared isolated UTF-16 name scan")
