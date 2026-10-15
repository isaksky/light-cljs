//! examples/features, the application the code-splitting group measures
//! against (docs/code-splitting.md, "Phase 0"), and the Node driver that runs
//! its single-file and, later, chunked output (tests/support/chunked_execution.mjs).
use crate::chunked_execution::run_modules;
use light_cljs::OptimizationLevel;
use std::process::Command;

/// The menu line app.main logs when it loads, then one line per feature, which
/// the default export, `app.main/start`, loads and runs in turn (before the
/// switch to `light.lazy`, it called them directly and printed the same).
pub(crate) const FEATURES_RECORD: &str = concat!(
    r#"{"id":"app.main.js","outcome":{"status":"returned","value":null},"trace":[],"stdout":""#,
    r#"menu: Editor | Reports\n"#,
    r#"editor: 4 documents, first Introduction, ids #{:api :faq :intro :setup}\n"#,
    r#"reports: tags Draft, Guide, Reference; drafts FAQ, INTRO; 1050 words\n"}"#,
);

pub(crate) fn compile_features(level: OptimizationLevel, esm: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
    command
        .args(["--compile", "--optimize", level.name()])
        .args(["--entry", "app.main", "--source-root"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features"));
    if esm {
        command.arg("--esm");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{level}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn features_example_logs_its_menu_and_both_features_at_every_level() {
    for level in OptimizationLevel::ALL {
        assert!(compile_features(level, false).starts_with("(() => {"));
        let module = compile_features(level, true);
        assert!(module.starts_with("export default "), "{level}");
        assert_eq!(
            run_modules(&[("app.main.js", &module)], "app.main.js").unwrap(),
            FEATURES_RECORD,
            "{level}"
        );
    }
}

/// The driver loads `.js` files as ES modules, waits for timers and dynamic
/// imports the default export starts without returning, awaits a returned
/// promise, reports errors as outcomes, and calls only a default export that is
/// a function; a promise that never settles fails the run.
#[test]
fn chunked_driver_waits_for_dynamic_imports_and_reports_errors() {
    let files = [
        (
            "main.js",
            "console.log(\"startup\");\n\
             export default () => {\n\
               setTimeout(() => import(\"./chunks/feature.js\").then(m => m.run()), 5);\n\
               return [\"started\", {at: 0}];\n\
             };\n",
        ),
        (
            "chunks/feature.js",
            "import {shared} from \"./shared.js\";\n\
             console.log(\"feature\", shared.join());\n\
             export const run = () => Promise.resolve().then(() => console.log(\"ran\"));\n",
        ),
        ("chunks/shared.js", "export const shared = [1, 2];\n"),
        (
            "rejects.js",
            "export default async () => { await import(\"./chunks/shared.js\"); throw null; };\n",
        ),
        (
            "throws.js",
            "console.log(\"before\");\nthrow new TypeError(\"failure\");\n",
        ),
        ("never.js", "export default () => new Promise(() => {});\n"),
        ("value.js", "console.log(\"value\");\nexport default 42;\n"),
    ];
    let record = |startup: &str, outcome: &str, stdout: &str| {
        format!(r#"{{"id":"{startup}","outcome":{outcome},"trace":[],"stdout":"{stdout}"}}"#)
    };
    assert_eq!(
        run_modules(&files, "main.js").unwrap(),
        record(
            "main.js",
            r#"{"status":"returned","value":["started",{"$object":{"at":0}}]}"#,
            r"startup\nfeature 1,2\nran\n"
        )
    );
    assert_eq!(
        run_modules(&files, "rejects.js").unwrap(),
        record(
            "rejects.js",
            r#"{"status":"threw","error":{"category":"thrown-value","value":null}}"#,
            ""
        )
    );
    assert_eq!(
        run_modules(&files, "throws.js").unwrap(),
        record(
            "throws.js",
            r#"{"status":"threw","error":{"category":"type-error"}}"#,
            r"before\n"
        )
    );
    // A default export that is not a function is the value; none is undefined.
    assert_eq!(
        run_modules(&files, "value.js").unwrap(),
        record(
            "value.js",
            r#"{"status":"returned","value":42}"#,
            r"value\n"
        )
    );
    assert_eq!(
        run_modules(&files, "chunks/shared.js").unwrap(),
        record(
            "chunks/shared.js",
            r#"{"status":"returned","value":{"$value":"undefined"}}"#,
            ""
        )
    );
    let error = run_modules(&files, "never.js").unwrap_err();
    assert!(error.contains("exit status: 13"), "{error}");
}
