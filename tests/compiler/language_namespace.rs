//! Execute the report's complete source observation in a fresh embedded heap.
#![cfg(any(light_v8, light_quickjs))]

#[test]
#[ignore = "explicit namespace report audit; Python checks the complete raw outcome"]
fn complete_namespace_observation_executes_in_embedded() {
    let source =
        std::fs::read_to_string(std::env::var("LIGHT_CLJS_NAMESPACE_SCRIPT").unwrap()).unwrap();
    let observed = crate::embedded_js::EmbeddedJs::new()
        .string(&source)
        .unwrap_or_else(|error| panic!("namespace observation failed: {error}"));
    std::fs::write(
        std::env::var("LIGHT_CLJS_NAMESPACE_OUTPUT").unwrap(),
        format!("{observed}\n"),
    )
    .unwrap();
}
