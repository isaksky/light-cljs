//! Namespace identity is resolved before lexical dotted property access.
use crate::temp_dir::TempDir;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::{path::Path, process::Command};

struct Sources(TempDir);
impl Sources {
    fn new() -> Self {
        let root = TempDir::new("namespace-shadow");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/compiler-namespace-shadowing");
        std::fs::create_dir_all(root.join("shadow")).unwrap();
        for path in ["baz.cljs", "shadow/other.cljs", "shadow/main.cljs"] {
            std::fs::copy(fixture.join(path), root.join(path)).unwrap();
        }
        Self(root)
    }
    fn write(&self, source: &str) {
        std::fs::write(self.0.join("shadow/main.cljs"), source).unwrap();
    }
    fn compile(&self, host: &mut MacroHost) -> Result<String, light_cljs::NamespaceCompileError> {
        compile_namespace_with_macros("shadow.main", std::slice::from_ref(self.0.path()), host)
    }
}
fn both(js: &str, expected: &str) {
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .string(evaluator.evaluate_js(js).unwrap())
            .unwrap(),
        expected
    );
    let output = Command::new("node")
        .args(["-e", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{expected}\n")
    );
}
const SOURCE: &str = include_str!("../fixtures/compiler-namespace-shadowing/shadow/main.cljs");
const EXPECTED: &str = "[3 7 1 7 7 17 [9 11 13 70 7 19 70] 90 [:before :after]]";

#[test]
fn qualified_vars_and_recursive_functions_survive_collisions_redefinition_and_phase_isolation() {
    let sources = Sources::new();
    let mut reused = MacroHost::new();
    for _ in 0..2 {
        for host in [&mut reused, &mut MacroHost::new()] {
            sources.write(&format!("{SOURCE}\n(run)"));
            both(&sources.compile(host).unwrap(), EXPECTED);
            let macro_source = SOURCE.replace("(defn bar [] 5)", "(defmacro snapshot [] (assert (= 3 before)) (assert (= 1 (saved))) nil) (snapshot)\n(defn bar [] 5)");
            sources.write(&format!("{macro_source}\n(defmacro observe [] (let [result (run)] (swap! effects conj :macro-only) result))\n(pr-str [(observe) (run)])"));
            both(
                &sources.compile(host).unwrap(),
                &format!("[{:?} {:?}]", EXPECTED, EXPECTED),
            );
        }
    }
}

#[test]
fn located_unknown_namespaces_and_vars_recover_in_the_same_macro_host() {
    let sources = Sources::new();
    let mut host = MacroHost::new();
    for symbol in ["shadow.absent/bar", "shadow.main/absent", "a.b/absent"] {
        for macro_phase in [false, true] {
            let body = format!("(let [shadow 1 a 2] ({symbol}))");
            let source = format!(
                "{SOURCE}\n{}",
                if macro_phase {
                    format!("(defmacro broken [] {body}) (broken)")
                } else {
                    body.clone()
                }
            );
            sources.write(&source);
            let error = sources.compile(&mut host).unwrap_err();
            assert!(
                error.message.contains("unresolved symbol"),
                "{symbol}: {error}"
            );
            if symbol.contains("absent/bar") {
                assert!(
                    error
                        .message
                        .contains("unknown namespace or alias shadow.absent"),
                    "{error}"
                );
            }
            assert!(error.path.unwrap().ends_with("shadow/main.cljs"));
            let start = source.rfind(symbol).unwrap();
            if macro_phase {
                let span = error.span.unwrap();
                assert_eq!(
                    &source[span.clone()],
                    &format!("(defmacro broken [] {body})")
                );
                assert!(span.start <= start && span.end >= start + symbol.len());
            } else {
                assert_eq!(error.span, Some(start..start + symbol.len()));
            }
            assert_eq!(error.line_column.unwrap().0, SOURCE.lines().count() + 2);
            let repaired = SOURCE.replace("(defn bar [] 5)", "(defmacro snapshot [] (assert (= 3 before)) (assert (= 1 (saved))) nil) (snapshot)\n(defn bar [] 5)");
            sources.write(&format!(
                "{repaired}\n(defmacro repaired [] (run))\n(repaired)"
            ));
            both(&sources.compile(&mut host).unwrap(), EXPECTED);
        }
    }
}
