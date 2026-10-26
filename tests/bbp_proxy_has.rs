#![cfg(feature = "bbp-macros")]

use light_cljs::{MacroHost, compile_namespace_with_macros};

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[test]
fn source_macro_demands_foreign_printer_extension_through_package_has() {
    let root = temp_dir::TempDir::new("native-foreign-printer-extension");
    std::fs::write(
        root.join("fixture.cljs"),
        r#"
(ns fixture (:require ["transit-js" :as transit]))
(def TransitUUID (.-UUID transit))
(extend-type TransitUUID
  IUUID
  IPrintWithWriter
  (-pr-writer [uuid writer opts] (-write writer "foreign")))
(defmacro demand-printer [] (pr-str [nil]))
(demand-printer)
"#,
    )
    .unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.opaque_packages = true;
    let output =
        compile_namespace_with_macros("fixture", &[root.path().to_path_buf()], &mut host).unwrap();
    assert!(output.contains("\"[nil]\""), "printer macro result missing");

    #[cfg(feature = "quickjs")]
    {
        let mut legacy = MacroHost::new();
        legacy.opaque_packages = true;
        let expected =
            compile_namespace_with_macros("fixture", &[root.path().to_path_buf()], &mut legacy)
                .unwrap();
        assert_eq!(output, expected);
    }

    let mut unavailable = MacroHost::new();
    unavailable.use_native_macros();
    let error =
        compile_namespace_with_macros("fixture", &[root.path().to_path_buf()], &mut unavailable)
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("JavaScript package \"transit-js\" is not provided"),
        "{error}"
    );
    assert!(error.contains("extension:"), "{error}");
}

#[test]
fn unsupported_native_foreign_printer_extension_remains_explicit() {
    let root = temp_dir::TempDir::new("native-unsupported-foreign-printer-extension");
    std::fs::write(
        root.join("fixture.cljs"),
        r#"
(ns fixture)
(extend-type js/Date
  IPrintWithWriter
  (-pr-writer [date writer opts] (-write writer "date")))
(defmacro demand-printer [] (pr-str [nil]))
(demand-printer)
"#,
    )
    .unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let error = compile_namespace_with_macros("fixture", &[root.path().to_path_buf()], &mut host)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "extension: unsupported operation: native protocol extension of [object Function]"
        ),
        "{error}"
    );
    assert!(error.contains("macro fixture/demand-printer"), "{error}");
}
