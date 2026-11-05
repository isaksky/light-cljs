//! Export one JavaScript file as a Goose program image (`.gsp`), for the
//! conformance harness (`scripts/conformance.bb` beside `rust/`).
//!
//! `cargo run --release --example goose_export -- [--sloppy | --module] IN.js OUT.gsp`
//!
//! The file is compiled by this crate's frontend and encoded by
//! `frontend::goose::export_source_with`; nothing runs. One line goes to
//! standard output and the exit status says what happened:
//!
//! * 0, `exported:FUNCTIONS:INSTRUCTIONS`: OUT.gsp was written.
//! * 3, `unsupported:MESSAGE`: the frontend or the exporter refused the file
//!   by name (direct eval, for example; every opcode exports). `--module` is always
//!   refused here: the exporter compiles scripts only.
//! * 2, `syntax:LINE:COLUMN: MESSAGE`: any other compile error.
//! * 1, `error:MESSAGE`: usage or file input/output.

use big_boy_pants_js::frontend::goose::export_source_with;
use big_boy_pants_js::frontend::{ScriptOptions, SyntaxErrorKind};

fn finish(status: i32, line: &str) -> ! {
    // One line, so the harness can read it as a single field.
    println!("{}", line.replace('\n', " "));
    std::process::exit(status);
}

fn main() {
    let mut options = ScriptOptions::STRICT;
    let mut module = false;
    let mut paths = Vec::new();
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--sloppy" => options = ScriptOptions::SLOPPY,
            "--module" => module = true,
            _ => paths.push(argument),
        }
    }
    let [input, output] = paths.as_slice() else {
        finish(
            1,
            "error:usage: goose_export [--sloppy | --module] IN.js OUT.gsp",
        );
    };
    let source = std::fs::read_to_string(input)
        .unwrap_or_else(|e| finish(1, &format!("error:{input}: {e}")));
    if module {
        finish(
            3,
            "unsupported:module: the Goose exporter compiles scripts, not modules",
        );
    }
    match export_source_with(input, &source, options) {
        Ok(export) => {
            std::fs::write(output, &export.bytes)
                .unwrap_or_else(|e| finish(1, &format!("error:{output}: {e}")));
            finish(
                0,
                &format!("exported:{}:{}", export.functions, export.instructions),
            );
        }
        Err(e) if e.kind == SyntaxErrorKind::Unsupported => finish(3, &format!("unsupported:{e}")),
        Err(e) => finish(2, &format!("syntax:{e}")),
    }
}
