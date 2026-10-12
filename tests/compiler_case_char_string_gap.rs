//! Exact original case contrast: compilation rejection plus same-host recovery.
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
#[ignore = "explicit narrow original case diagnostic; no semantic credit"]
fn original_case_char_string_rejection_and_actual_macro_recovery() {
    let root = PathBuf::from(std::env::var("LIGHT_CASE_CHAR_STRING_ROOT").unwrap());
    let original = fs::read_to_string(root.join("original.cljs")).unwrap();
    assert_eq!(
        original,
        r#"[(case \a "a" 1 \a 2 3) (case "a" \a 1 "a" 2 3)]"#
    );
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro case-ready [] (+ 20 22)) (case-ready)",
        &mut reused,
    )
    .unwrap();
    fs::write(root.join("warmup.js"), warmup).unwrap();
    let mut frames = fs::File::create(root.join("observations.tsv")).unwrap();
    for reuse in [false, true] {
        for phase in [false, true] {
            let variant = format!(
                "{}-{}",
                if phase { "macro" } else { "application" },
                if reuse { "reused" } else { "fresh" }
            );
            let folder = root.join(&variant);
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
            let error = light_cljs::compile_namespace_with_macros("user", &[folder.clone()], host)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("duplicate case test constant"),
                "{variant}: {error}"
            );
            fs::write(folder.join("rejection.txt"), &error).unwrap();
            let control = "(defmacro case-recovery [] (+ 20 22)) (case-recovery)";
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
                "{variant}\t{}\t{}\t{}\t42\t42",
                hex(&source),
                hex(&error),
                hex(&js)
            )
            .unwrap();
        }
    }
}
