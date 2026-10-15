//! Embedded first-party libraries enter the ordinary namespace compiler.
//! Explicit source roots may override a library, exactly like other sources.
pub(crate) fn source(namespace: &str) -> Option<&'static str> {
    Some(match namespace {
        "clojure.string" => include_str!("compiler_libraries/clojure/string.cljs"),
        "clojure.set" => include_str!("compiler_libraries/clojure/set.cljs"),
        "clojure.walk" => include_str!("compiler_libraries/clojure/walk.cljs"),
        "cljs.reader" => include_str!("compiler_libraries/cljs/reader.cljs"),
        "clojure.edn" => include_str!("compiler_libraries/clojure/edn.cljs"),
        "light.lazy" => include_str!("compiler_libraries/light/lazy.cljs"),
        _ => return None,
    })
}
