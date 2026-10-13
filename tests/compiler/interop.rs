//! Source-language JavaScript forms, diagnostics and direct host observations.
use light_cljs::{compile, compile_namespace};
use std::{fs, process::Command};

fn run(source: &str) -> String {
    let javascript = compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    let output = Command::new("node")
        .args(["-e", &format!("console.log(JSON.stringify({javascript}));")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn interop_has_direct_host_expectations_without_the_shared_encoder() {
    assert_eq!(
        run(
            "(js* \"[~{}, ~{}, ~{}]\" (.-length #js [1 2]) (aget #js {:x 7} \"x\") (.substring \"abc\" 1))"
        ),
        "[2,7,\"bc\"]\n"
    );
    assert_eq!(
        run("(let [a (make-array nil 2 2)] (aset a 0 1 9) a)"),
        "[[null,9],[null,null]]\n"
    );
    assert_eq!(
        run(
            "(let [x 7] (js* \"(function(l0, __light0_l0, rt){return ~{}})(9, 11, null)\" (inc x)))"
        ),
        "8\n"
    );
    assert_eq!(
        run("(let [f (fn [x] (aset (js-arguments) 0 9) x)] (.call f #js {} 2))"),
        "9\n"
    );
    assert_eq!(
        run(
            "(let [v [1] o #js {}] (aset v 0 9) (set! o -a-b 2) (js* \"[~{},~{},~{}]\" (aget v 0) (nth v 0) (aget o \"a_b\")))"
        ),
        "[9,1,2]\n"
    );
}

#[test]
fn malformed_special_forms_fail_with_source_spans_before_emission() {
    for source in [
        "(new)",
        "(.)",
        "(. x)",
        "(. 1 [])",
        "(. 1 (toString) 2)",
        "(. 1 ())",
        "(. 1 (3))",
        "(.-x 1 2)",
        "(set! #js {} x 2)",
        "(set! (aget #js {} 0) 2)",
        "(js*)",
        "(js* 1)",
        "(js* \"~{}\")",
        "(js* \"~{\" 1)",
        "(js* \"~{}+~{}\" 1)",
        "(js* \"1\" 2)",
        "(aget)",
        "(aset #js {} 1)",
        "(unchecked-set #js {} 1)",
    ] {
        let error = compile(source).unwrap_err();
        let light_cljs::CompileError::Analysis(error) = error else {
            panic!("analysis expected for {source}");
        };
        assert!(
            source.get(error.span.clone()).is_some(),
            "{source}: {error}"
        );
        assert!(!error.span.is_empty(), "{source}: {error}");
    }
    let source = "(do 1\n (js* \"~{}\" missing))";
    let light_cljs::CompileError::Analysis(error) = compile(source).unwrap_err() else {
        panic!()
    };
    assert_eq!(&source[error.span], "missing");
}

#[test]
fn referred_global_names_and_qualified_methods_survive_namespace_compilation() {
    let root = std::env::temp_dir().join(format!("light-cljs-interop-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.cljs"), "(ns main (:refer-global :only [String Object] :rename {String Text})) (let [method (fn [] Text/.toUpperCase) ctor Object/new] (js/console.log ((method) \"foo\") (object? (ctor))))").unwrap();
    let javascript = compile_namespace("main", std::slice::from_ref(&root)).unwrap();
    let output = Command::new("node")
        .args(["-e", &javascript])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"FOO true\n");
    fs::remove_dir_all(root).unwrap();
}
