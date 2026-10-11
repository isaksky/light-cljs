//! Public metadata observations are independent of the generated table.
use light_cljs::{
    analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE},
    parse, MacroHost,
};
use std::process::Command;

#[test]
fn recovered_core_var_metadata_executes_in_applications_and_macros() {
    // Literal names, source locations, docs and arities come from the pinned
    // public declarations, never from the compiler metadata table at test time.
    let contract = include_str!("fixtures/compiler-core-var-metadata.cljs");
    let source = format!(
        "{contract}\n(defmacro inspect-metadata [] (list 'quote (metadata-contract)))\n[(metadata-contract) (inspect-metadata)]"
    );
    let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|e| panic!("{e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .print(evaluator.evaluate(&program).unwrap())
            .unwrap(),
        "[true true]"
    );
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&program)
    );
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "[true true]");
}
