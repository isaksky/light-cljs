//! Run Node on a program sent through stdin rather than the command line,
//! which Windows caps at 32 KiB: compiled programs are often longer.
use std::io::Write;
use std::process::{Command, Output, Stdio};

/// `node --eval PROGRAM`.
pub fn eval(program: &str) -> std::io::Result<Output> {
    run(&["-"], program)
}

/// `node --print PROGRAM`: runs the program and prints its completion value.
pub fn print(program: &str) -> std::io::Result<Output> {
    run(
        &["--print", "eval(require('fs').readFileSync(0, 'utf8'))"],
        program,
    )
}

fn run(args: &[&str], program: &str) -> std::io::Result<Output> {
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Node reads the whole program before it runs any of it.
    child.stdin.take().unwrap().write_all(program.as_bytes())?;
    child.wait_with_output()
}
