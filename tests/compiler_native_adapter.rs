//! Temporary native embedding adapter contracts; removed with light-cljs-115.
//! This target deliberately requires native evaluation and is outside the compiler gate.
use light_cljs::interpreter::{Arity, Name, NativeDefinition, NativeEffects, NativeMetadata};
use light_cljs::{
    MacroHost, MacroImport, analyze_with_macros, compile_with_macros, emit_js, parse,
};
use std::process::Command;
fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", js])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn result(js: &str) -> String {
    execute(&format!("console.log({js});"))
}
fn host(source: &str, refers: &[&str]) -> MacroHost {
    let mut host = MacroHost::new();
    host.context_mut().eval_string(source).unwrap();
    host.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "macros".into(),
            alias: Some("m".into()),
            refers: refers.iter().map(|s| (*s).into()).collect(),
        }],
    )
    .unwrap();
    host
}
#[test]
fn native_host_macros_use_the_same_form_env_and_arity_abi() {
    static MACRO: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "answer",
            arity: Arity::Exact(3),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, args| {
            let env = args[1].get(&c.keyword(Name::new(None, "locals")))?.unwrap();
            let binding = env.get(&c.symbol(Name::new(None, "x")))?.unwrap();
            assert!(binding.get(&c.keyword(Name::new(None, "name")))?.is_some());
            Ok(args[2].clone())
        },
    };
    let mut h = MacroHost::new();
    h.register_macro(&MACRO);
    h.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "host".into(),
            alias: Some("h".into()),
            refers: vec![],
        }],
    )
    .unwrap();
    let source = "(let* [x 17] (h/answer x))";
    let syntax = parse(source).unwrap();
    let original = syntax.clone();
    let program = analyze_with_macros(source, &syntax, &mut h).unwrap();
    assert_eq!(
        syntax, original,
        "analysis must not mutate caller-owned syntax"
    );
    assert_eq!(result(&emit_js(&program)), "17\n");
    assert_eq!(
        result(&compile_with_macros("(let [x 19] (h/answer x))", &mut h).unwrap()),
        "19\n"
    );
    assert!(
        compile_with_macros("(h/answer)", &mut h)
            .unwrap_err()
            .to_string()
            .contains("Arity")
    );
}

#[test]
fn macro_statistics_attribute_native_and_interpreted_expansion_work() {
    let mut h = host("(ns macros) (defmacro value [] '(let [x 3] x))", &["value"]);
    h.context_mut().set_macro_diagnostics(true);
    compile_with_macros("(value)", &mut h).unwrap();
    let stats = h.context_mut().macro_expansion_stats();
    let custom = stats
        .iter()
        .find(|s| s.name == Name::new(Some("macros"), "value"))
        .unwrap();
    assert_eq!(custom.expansions, 1);
    assert!(custom.generated_forms > 1);
    assert!(custom.definition.is_some());
    assert!(
        stats
            .iter()
            .any(|s| s.name == Name::new(Some("clojure.core"), "let"))
    );
}

#[test]
fn macro_call_namespace_binding_is_restored_and_do_context_matches_cljs() {
    let mut h = host(
        "(ns macros) (defmacro context [] (str (ns-name *ns*) \"/\" (:context &env)))",
        &["context"],
    );
    let before = h.context_mut().current_namespace();
    assert_eq!(
        execute(
            &compile_with_macros("(js/console.log (do 1 (context)) (do (context)))", &mut h)
                .unwrap()
        ),
        "cljs.user/:return cljs.user/:expr\n"
    );
    assert!(before.same_identity(&h.context_mut().current_namespace()));
    assert!(compile_with_macros("(context 1)", &mut h).is_err());
    assert!(before.same_identity(&h.context_mut().current_namespace()));
}
