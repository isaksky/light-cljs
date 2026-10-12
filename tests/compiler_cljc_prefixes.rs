//! Reader prefixes own the next selected form in their original CLJC stream.
use light_cljs::{EvalOptions, EvalSession, MacroHost, source_provider::SourceInput};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sources(PathBuf);
impl Sources {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "light-cljc-prefix-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn compile(
        &self,
        source: &str,
        host: &mut MacroHost,
    ) -> Result<String, light_cljs::NamespaceCompileError> {
        fs::write(self.0.join("user.cljc"), source).unwrap();
        light_cljs::compile_namespace_with_macros("user", std::slice::from_ref(&self.0), host)
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
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
            std::slice::from_ref(&sources.0),
            &mut host,
        )
        .unwrap();
        both(&js, "[[1 2 3]]");
    }
}
