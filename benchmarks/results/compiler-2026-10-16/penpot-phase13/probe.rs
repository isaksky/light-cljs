fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let runtime = rquickjs::Runtime::new().unwrap();
    runtime.set_max_stack_size(16 * 1024 * 1024);
    let context = rquickjs::Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let source = format!("const rt = {};\n{}", std::fs::read_to_string(&args[1]).unwrap(), std::fs::read_to_string(&args[2]).unwrap());
        match ctx.eval::<String, _>(source) {
            Ok(result) => println!("{result}"),
            Err(error) => { eprintln!("{error}: {:?}", ctx.catch()); std::process::exit(1); }
        }
    });
}
