//! Static compiler macro/name classification, independent of evaluator state.
pub fn core_macro(name: &str) -> bool {
    matches!(
        name,
        "let"
            | "doto"
            | "js-obj"
            | "memfn"
            | "amap"
            | "areduce"
            | ".."
            | "fn"
            | "defn"
            | "defn-"
            | "defmacro"
            | "if-not"
            | "when"
            | "when-not"
            | "cond"
            | "assert"
            | "condp"
            | "while"
            | "dotimes"
            | "for"
            | "lazy-cat"
            | "selector"
            | "ns-publics"
            | "ns-interns"
            | "ns-imports"
            | "doseq"
            | "when-first"
            | "and"
            | "or"
            | "->"
            | "->>"
            | "as->"
            | "some->"
            | "some->>"
            | "cond->"
            | "cond->>"
            | "if-let"
            | "when-let"
            | "if-some"
            | "when-some"
            | "loop"
            | "case"
            | "letfn"
            | "vswap!"
    )
}

// These forms are lowered directly by the runtime analyzer; the native host's
// similarly named forms have interpreter-specific semantics.
pub fn is_definition_macro(namespace: &str, name: &str) -> bool {
    matches!(namespace, "cljs.core" | "clojure.core")
        && matches!(
            name,
            "delay"
                | "lazy-seq"
                | "defonce"
                | "declare"
                | "binding"
                | "with-redefs"
                | "resolve"
                | "exists?"
                | "with-out-str"
                | "this-as"
                | "js-arguments"
                | "copy-arguments"
        )
}
