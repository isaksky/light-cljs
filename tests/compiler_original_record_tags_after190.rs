//! Diagnostic only: preserve the original full record-tag source and failures.
//! No semantic or macro execution credit and no replacement tag reader.
use light_cljs::compiler_data::Source;
use light_cljs::source_provider::SourceInput;
use light_cljs::{EvalOptions, EvalSession, MacroHost, compile, compile_namespace_with_macros};
use std::io::Write;

#[test]
#[ignore = "explicit unchanged record-tag diagnostic; no completion credit"]
fn unchanged_record_tag_namespace_records_current_read_failures() {
    let source =
        include_str!("../research/clojurescript/src/test/cljs/data_readers_test/records.cljc");
    let root =
        std::path::PathBuf::from(std::env::var("LIGHT_RECORD_TAGS_AFTER190_ARTIFACTS").unwrap());
    let fixture = root.join("source/data_readers_test/records.cljc");
    std::fs::create_dir_all(fixture.parent().unwrap()).unwrap();
    std::fs::write(&fixture, source).unwrap();
    let mut output = std::fs::File::create(root.join("observations.tsv")).unwrap();
    let reader_error = compile(source).expect_err("Original record reader diagnostic changed");
    writeln!(output, "direct-compiler\tfresh\t{reader_error}").unwrap();
    let first_assertion = source
        .lines()
        .find(|line| line.starts_with("(assert (= (Foo. 1 2) #"))
        .unwrap();
    let form_error = MacroHost::new()
        .read_compiler_form(&Source::new("records.cljs", first_assertion))
        .unwrap_err();
    assert!(form_error.message.contains("UnknownTag"), "{form_error}");
    std::fs::write(root.join("original-first-assertion.cljs"), first_assertion).unwrap();
    writeln!(
        output,
        "original-first-assertion-reader\tfresh\t{form_error}"
    )
    .unwrap();
    for reused in [false, true] {
        let lifecycle = if reused { "reused" } else { "fresh" };
        let mut session = EvalSession::new(EvalOptions {
            namespace: "data-readers-test.records".into(),
            ..Default::default()
        })
        .unwrap();
        if reused {
            let warmup = session
                .eval_string("(defmacro initialized [] :warm) (initialized)")
                .unwrap();
            session.release(warmup.value).unwrap();
        }
        let error = match session.eval_source(&SourceInput::new("records.cljc", source)) {
            Err(error) => error.to_string(),
            Ok(result) => {
                session.release(result.value).unwrap();
                panic!(
                    "Diagnostic expectation changed; independently re-review original execution"
                );
            }
        };
        assert!(
            error.contains("macros use ordinary :require"),
            "unexpected original namespace boundary: {error}"
        );
        writeln!(output, "application\t{lifecycle}\t{error}").unwrap();
        let mut host = MacroHost::new();
        if reused {
            let warm = root.join("warm");
            std::fs::create_dir_all(&warm).unwrap();
            std::fs::write(
                warm.join("warm.cljs"),
                "(ns warm) (defmacro initialized [] :warm) (initialized)",
            )
            .unwrap();
            let js = compile_namespace_with_macros("warm", &[warm], &mut host).unwrap();
            std::fs::write(root.join("warmup.js"), js).unwrap();
        }
        let error = compile_namespace_with_macros(
            "data-readers-test.records",
            &[root.join("source")],
            &mut host,
        )
        .expect_err("Diagnostic expectation changed; re-review exact original phase proof");
        writeln!(output, "macro-host-compiler\t{lifecycle}\t{error}").unwrap();
    }
    // The sole documented adapter removes the retired self :require-macros
    // namespace declaration. Every original record declaration/assertion and
    // quoted field stays byte-identical. This diagnostic grants no original
    // source pass; it isolates the reader failure behind the namespace boundary.
    let old_namespace =
        "(ns data-readers-test.records\n  #?(:cljs (:require-macros [data-readers-test.records])))";
    assert_eq!(source.matches(old_namespace).count(), 1);
    let adapted = source.replacen(old_namespace, "(ns data-readers-test.records)", 1);
    std::fs::write(root.join("namespace-adapted-records.cljs"), &adapted).unwrap();
    for reused in [false, true] {
        let lifecycle = if reused { "reused" } else { "fresh" };
        let mut session = EvalSession::new(EvalOptions {
            namespace: "data-readers-test.records".into(),
            ..Default::default()
        })
        .unwrap();
        if reused {
            let warm = session
                .eval_string("(defmacro initialized [] :warm) (initialized)")
                .unwrap();
            session.release(warm.value).unwrap();
        }
        let error = match session.eval_source(&SourceInput::new(
            "namespace-adapted-records.cljs",
            &adapted,
        )) {
            Err(error) => error.to_string(),
            Ok(result) => {
                session.release(result.value).unwrap();
                panic!("Namespace-adapted diagnostic changed");
            }
        };
        assert!(error.contains("UnknownTag"), "{error}");
        writeln!(
            output,
            "namespace-adapted-application\t{lifecycle}\t{error}"
        )
        .unwrap();
    }
    output.flush().unwrap();
}
