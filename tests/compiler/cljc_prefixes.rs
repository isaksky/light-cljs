//! Reader prefixes own the next selected form in their original CLJC stream.
use crate::temp_dir::TempDir;
use light_cljs::{EvalOptions, EvalSession, MacroHost, source_provider::SourceInput};
use std::{fs, process::Command};
struct Sources(TempDir);
impl Sources {
    fn new() -> Self {
        let root = TempDir::new("cljc-prefix");
        Self(root)
    }
    fn compile(
        &self,
        source: &str,
        host: &mut MacroHost,
    ) -> Result<String, light_cljs::NamespaceCompileError> {
        fs::write(self.0.join("user.cljc"), source).unwrap();
        light_cljs::compile_namespace_with_macros("user", std::slice::from_ref(self.0.path()), host)
    }
}
fn both(js: &str, expected: &str) {
    let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    assert_eq!(e.string(e.evaluate_js(js).unwrap()).unwrap(), expected);
    let out = Command::new("node")
        .args(["-e", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), expected);
}
const CASES: &[(&str, &str)] = &[
    ("[#_ #?@(:clj [1 2]) 3]", "[]"),
    ("'[0 ^:private #?@(:clj [x y]) z]", "[0 z]"),
    ("'[0 '#?@(:clj [1 2]) 3]", "[0 (quote 3)]"),
    ("[#_ #?(:clj 1) 2 3]", "[3]"),
    ("[#_ #?(:cljs 1) 2 3]", "[2 3]"),
    ("[#_ #?@(:cljs [1 2]) 3]", "[2 3]"),
    ("[#_ #?@(:cljs []) 3 4]", "[4]"),
    ("'[0 '#?(:clj 1) 2 3]", "[0 (quote 2) 3]"),
    ("'[0 '#?(:cljs 1) 2 3]", "[0 (quote 1) 2 3]"),
    ("'[0 '#?@(:cljs [1 2]) 3]", "[0 (quote 1) 2 3]"),
    ("'[0 ''#?@(:clj [1 2]) 3 4]", "[0 (quote (quote 3)) 4]"),
    ("[#_ '#?@(:clj [1 2]) 3 4]", "[4]"),
    ("[#_ #_ #?@(:clj [1 2]) 3 4 5]", "[5]"),
    ("[#_ #_ #?@(:clj [1 2]) '#?@(:clj [3]) 4 5 6]", "[6]"),
    (
        "[(meta (first '[^:private #?@(:clj [x y]) z]))]",
        "[{:private true}]",
    ),
    (
        "[(meta (first '[^:private #?@(:cljs [x y]) z]))]",
        "[{:private true}]",
    ),
    ("'[^:private #?@(:cljs [x y]) z]", "[x y z]"),
    ("[#?(:clj [#_ #?(:clj 1) 2] :cljs 7) 8]", "[7 8]"),
    ("[#?(:clj #_ #unknown/tag 1 2 :cljs 7) 8]", "[7 8]"),
    ("[#_ [#_ #?@(:clj [1]) 2 3] 4]", "[4]"),
    ("'#?(:clj 1) 2", "2"),
    ("''#?(:clj 1) 2", "(quote 2)"),
    ("[(meta '^:private #?(:clj x) z)]", "[{:private true}]"),
];
#[test]
fn scalar_splice_and_nested_prefix_controls_execute_both_phases_and_lifecycles() {
    let sources = Sources::new();
    let mut reused = MacroHost::new();
    light_cljs::compile_with_macros("(defmacro warm [] 42) (warm)", &mut reused).unwrap();
    for &(original, expected) in CASES {
        for reuse in [false, true] {
            for phase in [false, true] {
                let mut fresh = MacroHost::new();
                let body = if phase {
                    format!(
                        "(defmacro upstream [] (list 'quote (do {original}))) (pr-str (upstream))"
                    )
                } else {
                    format!("(pr-str (do {original}))")
                };
                let source = format!("(ns user)\n{body}");
                let js = sources
                    .compile(&source, if reuse { &mut reused } else { &mut fresh })
                    .unwrap_or_else(|e| panic!("{original}, phase={phase}, reuse={reuse}: {e}"));
                both(&js, expected);
            }
        }
    }
}
#[test]
fn located_active_and_missing_target_errors_recover_in_hosts_and_sessions() {
    let sources = Sources::new();
    let mut host = MacroHost::new();
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    for (original, location) in [
        ("[0 '#?@(:cljs 1) 2]", "#?@(:cljs 1)"),
        ("[0 ^:private #?@(:cljs 1) z]", "#?@(:cljs 1)"),
        ("[#_ #?@(:cljs 1) 2]", "#?@(:cljs 1)"),
        ("#_ #?(:clj 1)", "#?(:clj 1)"),
        ("'#?(:clj 1)", "#?(:clj 1)"),
        ("'[0 '#?@(:clj [1])]", "#?@(:clj [1])"),
        ("'[0 ^:private #?@(:clj [x])]", "#?@(:clj [x])"),
    ] {
        for phase in [false, true] {
            let body = if phase {
                format!("(defmacro upstream [] (do {original})) (upstream)")
            } else {
                original.into()
            };
            let error = sources
                .compile(&format!("(ns user)\n{body}"), &mut host)
                .unwrap_err()
                .to_string();
            assert!(error.contains("user.cljc:2:"), "{error}");
            assert!(error.contains("reader"), "{error}");
            both(
                &sources
                    .compile("(ns user) (pr-str [42])", &mut host)
                    .unwrap(),
                "[42]",
            );
        }
        let error = session
            .eval_source(&SourceInput::new("bad.cljc", format!("\n{original}")))
            .unwrap_err();
        assert_eq!(error.location.as_ref().unwrap().text(), location);
        assert!(error.to_string().starts_with("bad.cljc:2:"));
        let value = session
            .eval_source(&SourceInput::new("recovered.cljc", "42"))
            .unwrap()
            .value;
        assert_eq!(session.print(value).unwrap(), "42");
        session.release(value).unwrap();
    }
    let error = sources.compile("(ns user) #?(:cljs 1)", &mut host);
    assert!(error.is_ok());
    assert!(
        session
            .eval_source(&SourceInput::new("plain.cljs", "[#?(:cljs 1)]"))
            .is_err()
    );
}
#[test]
fn selected_targets_keep_metadata_quote_ownership_and_source_locations() {
    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    for &(original, expected) in CASES {
        let value = session
            .eval_source(&SourceInput::new("controls.cljc", original))
            .unwrap_or_else(|e| panic!("{original}: {e}"))
            .value;
        assert_eq!(session.print(value).unwrap(), expected, "{original}");
        session.release(value).unwrap();
    }
    let source = SourceInput::new("ownership.cljc", "\n  '[^:private #?@(:clj [x]) z tail]");
    let forms = session.read_forms(&source).unwrap();
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].span().text(), "'[^:private #?@(:clj [x]) z tail]");
    let quoted = forms[0].children();
    let selected = quoted[1].children();
    assert_eq!(selected[0].span().text(), "^:private #?@(:clj [x]) z");
    assert_eq!(selected[1].span().text(), "tail");
    assert_eq!(selected[0].span().source().name(), "ownership.cljc");
}

#[test]
fn nested_discarded_readers_keep_canonical_effect_order() {
    use light_cljs::compiler_data::Name;
    let mut session = EvalSession::new(EvalOptions {
        data_readers: vec![(
            Name::new(None, "counter"),
            "(let [seen (atom [])] (fn [x] (swap! seen conj x)))".into(),
        )],
        ..Default::default()
    })
    .unwrap();
    for (source, expected) in [
        (
            "[#_ #_ #?@(:clj [1]) #counter 3 #counter 4 #counter 5]",
            "[[3 4 5]]",
        ),
        (
            "[#_ #_ #counter 6 #counter 7 #counter 8]",
            "[[3 4 5 6 7 8]]",
        ),
    ] {
        let value = session
            .eval_source(&SourceInput::new("effects.cljc", source))
            .unwrap()
            .value;
        assert_eq!(session.print(value).unwrap(), expected);
        session.release(value).unwrap();
    }
}

#[test]
fn nested_discarded_readers_execute_once_in_plain_and_shared_source_hosts() {
    use light_cljs::compiler_data::Name;
    for filename in ["user.cljs", "user.cljc"] {
        let sources = Sources::new();
        let source = "(ns user) (pr-str [#_ #_ #counter 1 #counter 2 #counter 3])";
        fs::write(sources.0.join(filename), source).unwrap();
        let mut host = MacroHost::new();
        host.register_compiler_data_reader(
            Name::new(None, "counter"),
            "(let [seen (atom [])] (fn [x] (swap! seen conj x)))",
        )
        .unwrap();
        let js = light_cljs::compile_namespace_with_macros(
            "user",
            std::slice::from_ref(sources.0.path()),
            &mut host,
        )
        .unwrap();
        both(&js, "[[1 2 3]]");
    }
}

/// Unchanged original reader failures, located diagnostics and actual host recovery.
mod original_reader_rejections {
    use crate::temp_dir::TempDir;
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
        let supplied = std::env::var("LIGHT_CLJC_REJECTIONS_ROOT").ok();
        let directory = match &supplied {
            Some(root) => TempDir::retained(PathBuf::from(root)),
            None => {
                let directory = TempDir::new("cljs-reader-rejections");
                fs::write(
                    directory.join("input.tsv"),
                    include_str!("../fixtures/cljc-reader-rejections.tsv"),
                )
                .unwrap();
                directory
            }
        };
        let root = directory.canonicalize().unwrap();
        let input = fs::read_to_string(root.join("input.tsv")).unwrap();
        let original_root = std::env::var("LIGHT_CLJC_REJECTIONS_SOURCE_ROOT")
            .ok()
            .map(|path| PathBuf::from(path).canonicalize().unwrap());
        let mut output = fs::File::create(root.join("observations.tsv")).unwrap();
        let mut reused = MacroHost::new();
        let warm_source =
            "(defmacro before-reader-rejection [] (+ 20 22)) (before-reader-rejection)";
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
                    let error =
                        light_cljs::compile_namespace_with_macros("user", &[source_dir], host)
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
    }
}
