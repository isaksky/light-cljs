fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = format!(
        "const rt = {};\n{}",
        std::fs::read_to_string(&args[1]).expect("runtime"),
        std::fs::read_to_string(&args[2]).expect("expression")
    );
    if let Ok(flags) = std::env::var("V8_PROBE_FLAGS") {
        v8::V8::set_flags_from_string(&flags);
    }
    let heap_bytes = std::env::var("V8_PROBE_HEAP_BYTES")
        .map(|value| value.parse::<usize>().expect("heap bytes"))
        .unwrap_or(4_000_000_000);
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    {
        let isolate = &mut v8::Isolate::new(v8::CreateParams::default().heap_limits(0, heap_bytes));
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let source = v8::String::new(scope, &source).expect("source");
        let result = v8::Script::compile(scope, source, None)
            .and_then(|script| script.run(scope));
        match result {
            Some(result) => println!("{}", result.to_rust_string_lossy(scope)),
            None => {
                let error = scope.exception().expect("exception");
                eprintln!("{}", error.to_rust_string_lossy(scope));
                if let Some(stack) = scope.stack_trace() {
                    eprintln!("{}", stack.to_rust_string_lossy(scope));
                }
                std::process::exit(1);
            }
        }
    }
    unsafe { v8::V8::dispose(); }
    v8::V8::dispose_platform();
}
