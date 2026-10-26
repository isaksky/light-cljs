#![cfg(feature = "bbp-macros")]

use light_cljs::{MacroHost, compile_namespace_with_macros};

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[test]
fn source_macro_demands_foreign_printer_extension_through_package_has() {
    let root = temp_dir::TempDir::new("native-foreign-printer-extension");
    // A source-defined printer protocol lets this exercise extension demand
    // independently of the native core printer services added by light-cljs-513.
    std::fs::write(
        root.join("fixture.cljs"),
        r#"
(ns fixture (:require ["transit-js" :as transit]))
(defprotocol Printer (print-value [value writer opts]))
(def TransitUUID (.-UUID transit))
(extend-type TransitUUID
  Printer
  (print-value [uuid writer opts] :foreign))
(defmacro demand-printer [] (print-value nil nil nil))
(demand-printer)
"#,
    )
    .unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.opaque_packages = true;
    let error = compile_namespace_with_macros("fixture", &[root.path().to_path_buf()], &mut host)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("extension: unsupported Proxy trap: get"),
        "{error}"
    );
    assert!(error.contains("macro fixture/demand-printer"), "{error}");
    assert!(!error.contains("unsupported Proxy trap: has"), "{error}");
}
