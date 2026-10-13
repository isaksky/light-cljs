//! Complete original reader sources with actual .cljc selection and MacroHosts.
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
fn observe(script: &str, artifact: &std::path::Path) -> [(String, String); 2] {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| match ctx.eval::<String, _>(script.as_bytes()) {
        Ok(value) => value,
        Err(error) => format!("HARNESS_ERROR:{error:?}: {:?}", ctx.catch()),
    });
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
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    fs::write(artifact.join("script.js"), script).unwrap();
    fs::write(artifact.join("quickjs.raw"), &quickjs).unwrap();
    fs::write(
        artifact.join("node.stdin"),
        format!("console.log({script});"),
    )
    .unwrap();
    fs::write(artifact.join("node.stdout"), &out.stdout).unwrap();
    fs::write(artifact.join("node.stderr"), &out.stderr).unwrap();
    fs::write(
        artifact.join("node.exit"),
        out.status.code().unwrap_or(-1).to_string(),
    )
    .unwrap();
    [
        ("QuickJS".into(), quickjs),
        (
            "Node".into(),
            if out.status.success() && out.stderr.is_empty() {
                String::from_utf8(out.stdout).unwrap().trim().into()
            } else {
                format!("HARNESS_ERROR:{}", String::from_utf8_lossy(&out.stderr))
            },
        ),
    ]
}
#[test]
#[ignore = "explicit original .cljc filename and actual phase audit"]
fn accepted_original_cljc_sources_execute_in_fresh_and_reused_macro_hosts() {
    let root = PathBuf::from(std::env::var("LIGHT_SCI_CLJC_ACCEPTED_ROOT").unwrap());
    let input = fs::read_to_string(root.join("input.tsv")).unwrap();
    let mut out = fs::File::create(root.join("observations.tsv")).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro cljc-ready [] (+ 20 22)) (cljc-ready)",
        &mut reused,
    )
    .unwrap();
    fs::write(root.join("warmup.js"), &warmup).unwrap();
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 2);
        let (id, original) = (fields[0], unhex(fields[1]));
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
                let sources = root.join(id).join(&variant);
                fs::create_dir_all(&sources).unwrap();
                fs::write(sources.join("user.cljc"), &source).unwrap();
                let mut fresh = MacroHost::new();
                match light_cljs::compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&sources),
                    if reuse { &mut reused } else { &mut fresh },
                ) {
                    Err(e) => {
                        fs::write(sources.join("rejection.txt"), e.to_string()).unwrap();
                        writeln!(
                            out,
                            "{id}\t{variant}\trejected\t{}\t{}",
                            hex(&source),
                            hex(&e.to_string())
                        )
                        .unwrap();
                    }
                    Ok(js) => {
                        let anchor = " = // Original bounded runtime";
                        assert_eq!(js.matches(anchor).count(), 1);
                        let captured = js.replacen(
                            anchor,
                            " = globalThis.lightRuntime = // Original bounded runtime",
                            1,
                        );
                        let script = format!(
                            "(() => {{const run={captured};return ({})(globalThis.lightRuntime)('{id}',observe=>run(observe));}})()",
                            include_str!("../support/sci_execution_observation.js")
                        );
                        fs::write(sources.join("compiled.js"), &js).unwrap();
                        for (engine, raw) in observe(&script, &sources) {
                            writeln!(
                                out,
                                "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                                hex(&source),
                                hex(&js),
                                hex(&raw),
                                if reuse { hex(&warmup) } else { "-".into() }
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    assert_eq!(input.lines().count(), 30);
    out.flush().unwrap();
}

/// Complete original .cljc values observed after actual application/macro execution.
mod transport {
    use super::{hex, unhex};
    use light_cljs::MacroHost;
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
    };
    fn observe(script: &str, root: &std::path::Path) -> [(String, String); 2] {
        let rt = rquickjs::Runtime::new().unwrap();
        let ctx = rquickjs::Context::full(&rt).unwrap();
        let quickjs = ctx.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
        let mut child = Command::new(std::env::var("NODE").unwrap())
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = format!("console.log({script});");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        fs::write(root.join("script.js"), script).unwrap();
        fs::write(root.join("quickjs.raw"), &quickjs).unwrap();
        fs::write(root.join("node.stdin"), input).unwrap();
        fs::write(root.join("node.stdout"), &out.stdout).unwrap();
        fs::write(root.join("node.stderr"), &out.stderr).unwrap();
        fs::write(
            root.join("node.exit"),
            out.status.code().unwrap().to_string(),
        )
        .unwrap();
        [
            ("QuickJS".into(), quickjs),
            (
                "Node".into(),
                String::from_utf8(out.stdout).unwrap().trim().into(),
            ),
        ]
    }
    #[test]
    #[ignore = "explicit complete original source transport proof"]
    fn original_cljc_transport_values_execute_in_both_phases_and_host_lifecycles() {
        let root = PathBuf::from(std::env::var("LIGHT_SCI_CLJC_TRANSPORT_ROOT").unwrap());
        let input = fs::read_to_string(root.join("input.tsv")).unwrap();
        let mut out = fs::File::create(root.join("observations.tsv")).unwrap();
        let mut reused = MacroHost::new();
        let warmup = light_cljs::compile_with_macros(
            "(defmacro transport-ready [] (+ 20 22)) (transport-ready)",
            &mut reused,
        )
        .unwrap();
        fs::write(root.join("warmup.js"), &warmup).unwrap();
        for row in input.lines() {
            let f: Vec<_> = row.split('\t').collect();
            assert_eq!(f.len(), 3);
            let id = f[0];
            let original = unhex(f[1]);
            let body = unhex(f[2]);
            assert!(body.contains(&format!("(do {original})")));
            for reuse in [false, true] {
                for phase in [false, true] {
                    let variant = format!(
                        "{}-{}",
                        if phase { "macro" } else { "application" },
                        if reuse { "reused" } else { "fresh" }
                    );
                    let source = if phase {
                        format!(
                            "(ns user)\n(defmacro upstream [] {body})\n(defn run [] (upstream))\nrun\n"
                        )
                    } else {
                        format!("(ns user)\n(defn run [] {body})\nrun\n")
                    };
                    let folder = root.join(id).join(&variant);
                    fs::create_dir_all(&folder).unwrap();
                    fs::write(folder.join("user.cljc"), &source).unwrap();
                    let mut fresh = MacroHost::new();
                    let unit = light_cljs::compile_namespace_with_macros(
                        "user",
                        std::slice::from_ref(&folder),
                        if reuse { &mut reused } else { &mut fresh },
                    )
                    .unwrap();
                    fs::write(folder.join("compiled.js"), &unit).unwrap();
                    let script = format!("(() => {{const run={unit};return run();}})()");
                    for (engine, raw) in observe(&script, &folder) {
                        writeln!(
                            out,
                            "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                            hex(&source),
                            hex(&unit),
                            hex(&raw),
                            if reuse { hex(&warmup) } else { "-".into() }
                        )
                        .unwrap();
                    }
                }
            }
        }
        assert_eq!(input.lines().count(), 3);
        out.flush().unwrap();
    }
}
