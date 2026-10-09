//! Run with `cargo run --example embedding`; no JVM/JS runtime is involved.
use light_cljs::interpreter::*;

static GREET: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "app",
        name: "greet",
        arity: Arity::Exact(1),
        effects: NativeEffects::Pure,
        errors: &[NativeErrorClass::Type],
    },
    callback: |context, args| {
        let ValueView::String(name) = args[0].inspect() else {
            return Err(NativeError::Type {
                argument: 0,
                expected: "string",
            });
        };
        let mut text: Vec<u16> = "Hello, ".encode_utf16().collect();
        text.extend_from_slice(&name);
        Ok(context.string(text))
    },
};

// Native macros receive &form, &env, and the unevaluated arguments as data.
static TWICE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "app",
        name: "twice",
        arity: Arity::Exact(3),
        effects: NativeEffects::Context,
        errors: &[NativeErrorClass::Collection],
    },
    callback: |context, args| {
        Ok(context.list(&[
            context.symbol(Name::new(None, "do")),
            args[2].clone(),
            args[2].clone(),
        ])?)
    },
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut context = Context::init(ContextOptions {
        natives: vec![&GREET],
        macros: vec![&TWICE],
        ..Default::default()
    })?;
    let user = context.string("Rust".encode_utf16().collect::<Vec<_>>());
    context.merge_opts(ContextOptions {
        bindings: vec![(Name::new(None, "user-name"), user)],
        ..Default::default()
    })?;
    let result = context.eval_string_plus(
        "(def calls (atom 0)) (fn [name] (app/twice (swap! calls inc)) (app/greet name))",
    )?;
    let name = context.eval_string("user-name")?;
    let greeting = context.invoke(&result.value, &[name])?;
    let ValueView::String(text) = greeting.inspect() else {
        unreachable!()
    };
    println!("{}", String::from_utf16(&text)?);
    if let Err(error) = context.eval_string_plus("(app/greet 42)") {
        println!(
            "Caught {:?} error in {:?}",
            error.diagnostic.phase,
            error.namespace.inspect()
        );
    }
    let count = context.eval_string("@calls")?;
    println!("Still usable; calls = {:?}", count.inspect());
    let source = context.source("form.clj", "(+ @calls 1)");
    let form = context.reader(&source)?.read_form()?.expect("one form");
    println!(
        "Evaluated form = {:?}",
        context.eval_form_plus(&form)?.value.inspect()
    );
    Ok(())
}
