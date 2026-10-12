//! Public printing helper frontier: exact source rejection and same-host recovery.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}
#[test]
#[ignore = "explicit public printing source diagnostic; zero semantic credit"]
fn public_print_helper_rejection_and_actual_macro_recovery() {
    let root = PathBuf::from(std::env::var("LIGHT_PUBLIC_PRINT_HELPER_ROOT").unwrap());
    let cases = [
        (
            "public-pr-str-star",
            include_str!("fixtures/public-print-helper-gap/public-pr-str-star.cljs"),
            "pr-str*",
        ),
        (
            "public-string-buffer-writer",
            include_str!("fixtures/public-print-helper-gap/public-string-buffer-writer.cljs"),
            "StringBufferWriter",
        ),
    ];
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro print-ready [] (+ 20 22)) (print-ready)",
        &mut reused,
    )
    .unwrap();
    fs::write(root.join("warmup.js"), warmup).unwrap();
    let mut frames = fs::File::create(root.join("observations.tsv")).unwrap();
    for (identity, original, missing) in cases {
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let folder = root.join(identity).join(&variant);
                fs::create_dir_all(&folder).unwrap();
                let source = if phase {
                    format!(
                    "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [] (upstream))\nrun\n"
                )
                } else {
                    format!("(ns user)\n(defn run [] (do {original}))\nrun\n")
                };
                fs::write(folder.join("user.cljs"), &source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if reuse { &mut reused } else { &mut fresh };
                let error =
                    light_cljs::compile_namespace_with_macros("user", &[folder.clone()], host)
                        .unwrap_err()
                        .to_string();
                assert!(
                    error.contains(&format!("unresolved symbol: {missing}")),
                    "{variant}: {error}"
                );
                fs::write(folder.join("rejection.txt"), &error).unwrap();
                let control = "(defmacro print-recovery [] (+ 20 22)) (print-recovery)";
                fs::write(folder.join("recovery.cljs"), control).unwrap();
                let js = light_cljs::compile_with_macros(control, host).unwrap();
                fs::write(folder.join("recovery.js"), &js).unwrap();
                let runtime = rquickjs::Runtime::new().unwrap();
                let context = rquickjs::Context::full(&runtime).unwrap();
                let value = context.with(|ctx| ctx.eval::<i32, _>(js.as_bytes()).unwrap());
                assert_eq!(value, 42);
                let input = format!("console.log({js});");
                fs::write(folder.join("node.stdin"), &input).unwrap();
                let mut child = Command::new(std::env::var("NODE").unwrap())
                    .arg("-")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(input.as_bytes())
                    .unwrap();
                let out = child.wait_with_output().unwrap();
                fs::write(folder.join("node.stdout"), &out.stdout).unwrap();
                fs::write(folder.join("node.stderr"), &out.stderr).unwrap();
                assert!(out.status.success());
                assert!(out.stderr.is_empty());
                assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "42");
                writeln!(
                    frames,
                    "{identity}\t{variant}\t{}\t{}\t{}\t42\t42",
                    hex(&source),
                    hex(&error),
                    hex(&js)
                )
                .unwrap();
            }
        }
    }
}
