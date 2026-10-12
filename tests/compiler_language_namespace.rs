//! Execute the report's complete source observation in a fresh QuickJS heap.
#[test]
#[ignore = "explicit namespace report audit; Python checks the complete raw outcome"]
fn complete_namespace_observation_executes_in_quickjs() {
    let source = std::fs::read(std::env::var("LIGHT_CLJS_NAMESPACE_SCRIPT").unwrap()).unwrap();
    let runtime = rquickjs::Runtime::new().unwrap();
    runtime.set_memory_limit(128 * 1024 * 1024);
    runtime.set_max_stack_size(4 * 1024 * 1024);
    let context = rquickjs::Context::full(&runtime).unwrap();
    let observed = context.with(|ctx| {
        ctx.eval::<String, _>(source).unwrap_or_else(|error| {
            panic!("namespace observation failed: {error:?}: {:?}", ctx.catch())
        })
    });
    std::fs::write(
        std::env::var("LIGHT_CLJS_NAMESPACE_OUTPUT").unwrap(),
        format!("{observed}\n"),
    )
    .unwrap();
}
