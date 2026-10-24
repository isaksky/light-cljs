//! Thin binary over the `clj_quickjs` library.
//!
//! Runs one hand-assembled early VM test program (sum of 1..=10). This is not
//! JavaScript source support; no JavaScript frontend exists yet.

use clj_quickjs::{Assembler, Opcode, Primitive, RunOutcome, Runtime, ValueView};

fn main() {
    // locals: 0 = i, 1 = sum
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.constant(Primitive::Number(1.0)).set_local(0);
    asm.constant(Primitive::Number(0.0)).set_local(1);
    asm.bind(top);
    asm.constant(Primitive::Number(10.0))
        .get_local(0)
        .op(Opcode::Lt)
        .op(Opcode::Not);
    asm.jump_if_false(done);
    asm.get_local(1).get_local(0).op(Opcode::Add).set_local(1);
    asm.get_local(0)
        .constant(Primitive::Number(1.0))
        .op(Opcode::Add)
        .set_local(0);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(1).op(Opcode::Return);

    let exe = match asm.finish("sum-1-to-10", 0, 2) {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let rt = Runtime::new();
    let outcome = rt
        .spawn(&exe, &[])
        .map_err(|e| e.to_string())
        .and_then(|t| t.run().map_err(|e| e.to_string()));
    println!("early VM test (hand-assembled bytecode, not JavaScript source):");
    match outcome {
        Ok(RunOutcome::Complete(v)) => match v.view() {
            ValueView::Number(n) => println!("sum 1..=10 = {n}"),
            other => println!("unexpected result {other:?}"),
        },
        Ok(other) => println!("{other:?}"),
        Err(e) => {
            eprintln!("run error: {e}");
            std::process::exit(1);
        }
    }
}
