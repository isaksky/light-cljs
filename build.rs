//! Writes the embedded runtime: `src/js_runtime.js` without its unit manifest
//! header lines (lines whose first non-blank characters are `//@`; see
//! `src/runtime_manifest.rs`). Output, the evaluator and the REPL embed the
//! result, so the headers cost no emitted bytes.

use std::{env, fs, path::Path};

fn main() {
    println!("cargo::rerun-if-changed=src/js_runtime.js");
    let source = fs::read_to_string("src/js_runtime.js").expect("read src/js_runtime.js");
    let runtime = source
        .split_inclusive('\n')
        .filter(|line| !line.trim_start().starts_with("//@"))
        .collect::<String>();
    let output = Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR")).join("js_runtime.js");
    fs::write(output, runtime).expect("write the embedded runtime");
}
