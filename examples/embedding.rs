//! Run with `cargo run --example embedding`; uses the embedded QuickJS engine.
use light_cljs::compiler_data::{Datum, DatumKind};
use light_cljs::source_provider::SourceInput;
use light_cljs::{EvalOptions, EvalSession, HostArity, HostFunction};
use std::rc::Rc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = EvalSession::new(EvalOptions {
        functions: vec![HostFunction {
            name: "app/greet".into(),
            arity: HostArity::Exact(1),
            callback: Rc::new(|call, args| {
                let name = call.string(args[0])?;
                call.value(&Datum::new(DatumKind::String(
                    format!("Hello, {name}").encode_utf16().collect(),
                )))
            }),
        }],
        ..Default::default()
    })?;
    let function = session.eval_string(
        "(def calls (atom 0)) (defmacro twice [x] (list 'do x x)) (fn [name] (twice (swap! calls inc)) (app/greet name))",
    )?.value;
    let name = session.value(&Datum::new(DatumKind::String(
        "Rust".encode_utf16().collect(),
    )))?;
    let greeting = session.invoke(function, &[name])?;
    println!("{}", session.string(greeting)?);
    if let Err(error) = session.eval_string("(app/greet 42)") {
        println!("Caught {error}");
    }
    let count = session.eval_string("@calls")?.value;
    println!("Still usable; calls = {}", session.number(count)?);
    let form = session
        .read_forms(&SourceInput::new("form.cljs", "(+ @calls 1)"))?
        .remove(0);
    let result = session.eval_form(&form)?.value;
    println!("Evaluated form = {}", session.number(result)?);
    for id in [function, name, greeting, count, result] {
        session.release(id)?;
    }
    Ok(())
}
