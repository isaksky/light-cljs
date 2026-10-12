//! Unchanged original reader failures, located diagnostics and actual host recovery.
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
fn unhex(s: &str) -> String {
    String::from_utf8(
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn observe(js: &str) -> [(&'static str, String); 2] {
    let script = format!("JSON.stringify({js})");
    let rt = rquickjs::Runtime::new().unwrap();
    let ctx = rquickjs::Context::full(&rt).unwrap();
    let q = ctx.with(|cx| cx.eval::<String, _>(script.as_bytes()).unwrap());
    let mut child = Command::new("node")
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
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    [
        ("QuickJS", q),
        ("Node", String::from_utf8(out.stdout).unwrap().trim().into()),
    ]
}

#[test]
fn unchanged_original_reader_rejections_preserve_source_and_host_recovery() {
    let supplied = std::env::var("LIGHT_CLJC_REJECTIONS_AFTER191_ROOT").ok();
    let root = supplied
        .as_ref()
        .map_or_else(
            || {
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let directory = std::env::temp_dir().join(format!(
                    "light-cljs-reader-rejections-after191-{}-{nonce}",
                    std::process::id()
                ));
                fs::create_dir(&directory).unwrap();
                fs::write(
                    directory.join("input.tsv"),
                    include_str!("fixtures/cljc-rejections-after191.tsv"),
                )
                .unwrap();
                directory
            },
            PathBuf::from,
        )
        .canonicalize()
        .unwrap();
    let input = fs::read_to_string(root.join("input.tsv")).unwrap();
    let original_root = std::env::var("LIGHT_CLJC_REJECTIONS_AFTER191_SOURCE_ROOT")
        .ok()
        .map(|path| PathBuf::from(path).canonicalize().unwrap());
    let mut output = fs::File::create(root.join("observations.tsv")).unwrap();
    let mut reused = MacroHost::new();
    let warm_source = "(defmacro before-reader-rejection [] (+ 20 22)) (before-reader-rejection)";
    let warm = light_cljs::compile_with_macros(warm_source, &mut reused).unwrap();
    fs::write(root.join("warmup.cljs"), warm_source).unwrap();
    fs::write(root.join("warmup.js"), &warm).unwrap();
    for (engine, raw) in observe(&warm) {
        assert_eq!(raw, "42");
        writeln!(output, "warmup\t{engine}\t{}", hex(&raw)).unwrap();
    }
    assert_eq!(input.lines().count(), 4);
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let id = fields[0];
        let original = unhex(fields[1]);
        let required = unhex(fields[2]);
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let source = if phase {
                    format!(
                        "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [observe] (upstream))\nrun\n"
                    )
                } else {
                    format!("(ns user)\n(defn run [observe] (do {original}))\nrun\n")
                };
                let dir = root.join(id).join(&variant);
                fs::create_dir_all(&dir).unwrap();
                let source_dir = original_root
                    .as_ref()
                    .map_or_else(|| dir.clone(), |original| original.join(id).join(&variant));
                let path = source_dir.join("user.cljc");
                if original_root.is_some() {
                    assert_eq!(fs::read_to_string(&path).unwrap(), source);
                } else {
                    fs::write(&path, &source).unwrap();
                }
                fs::write(dir.join("input-source.txt"), &source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if reuse { &mut reused } else { &mut fresh };
                let error = light_cljs::compile_namespace_with_macros("user", &[source_dir], host)
                    .unwrap_err();
                assert!(
                    error.message.contains(&required),
                    "{id}/{variant}: {error:?}"
                );
                assert_eq!(error.path.as_ref(), Some(&path));
                let span = error.span.as_ref().unwrap();
                assert!(span.start < span.end && span.end <= source.len());
                let (line, column) = error.line_column.unwrap();
                assert!(line > 0 && column > 0);
                fs::write(dir.join("diagnostic.txt"), error.to_string()).unwrap();
                writeln!(
                    output,
                    "rejected\t{id}\t{variant}\t{}\t{}\t{}\t{}\t{}\t{line}\t{column}\t{}",
                    hex(&source),
                    hex(&error.message),
                    span.start,
                    span.end,
                    hex(path.to_str().unwrap()),
                    hex(&error.to_string())
                )
                .unwrap();
                let recovery_source =
                    "(defmacro after-reader-rejection [] (+ 20 22)) (after-reader-rejection)";
                let recovery = light_cljs::compile_with_macros(recovery_source, host).unwrap();
                fs::write(dir.join("recovery.cljs"), recovery_source).unwrap();
                fs::write(dir.join("recovery.js"), &recovery).unwrap();
                for (engine, raw) in observe(&recovery) {
                    assert_eq!(raw, "42");
                    writeln!(
                        output,
                        "recovery\t{id}\t{variant}\t{engine}\t{}\t{}",
                        hex(&recovery),
                        hex(&raw)
                    )
                    .unwrap();
                }
            }
        }
    }
    output.flush().unwrap();
    if supplied.is_none() {
        drop(output);
        fs::remove_dir_all(&root).unwrap();
    }
}
