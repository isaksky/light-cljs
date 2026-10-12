//! Retained source rejection only; no original or phase pass credit.
use light_cljs::MacroHost;
use std::fs;
const ORIGINAL: &str =
    "(defprotocol P (p [x])) (defrecord R [x] P (p [_] x) (p [_] 9)) (p (->R 3))";
#[test]
#[ignore = "Finite complete original duplicate-method rejection audit"]
fn original_duplicate_method_retains_phase_rejection() {
    let root =
        std::path::PathBuf::from(std::env::var("LIGHT_SCI_RECORD_DUPLICATE_METHOD_ROOT").unwrap());
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    let mut reused = MacroHost::new();
    fs::write(
        root.join("warmup.js"),
        light_cljs::compile_with_macros("(defmacro ready [] true) (ready)", &mut reused).unwrap(),
    )
    .unwrap();
    for reuse in [false, true] {
        for whole in [false, true] {
            let name = format!(
                "{}-{}",
                if whole {
                    "complete-original"
                } else {
                    "top-level-original-setup"
                },
                if reuse { "reused" } else { "fresh" }
            );
            let source = if whole {
                format!(
                    "(ns user)\n(defmacro upstream [] (list 'quote (do {ORIGINAL})))\n(defn run [] (upstream))\n"
                )
            } else {
                format!(
                    "(ns user)\n{}(defmacro upstream [] (list 'quote (p (->R 3))))\n(defn run [] (upstream))\n",
                    ORIGINAL.strip_suffix("(p (->R 3))").unwrap()
                )
            };
            let directory = root.join(&name);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("user.cljs"), source).unwrap();
            let mut fresh = MacroHost::new();
            let error = light_cljs::compile_namespace_with_macros(
                "user",
                &[directory.clone()],
                if reuse { &mut reused } else { &mut fresh },
            )
            .unwrap_err()
            .to_string();
            fs::write(directory.join("error.txt"), &error).unwrap();
            assert!(
                error.contains("duplicate protocol method arity") && error.contains("user.cljs:"),
                "{name}: {error}"
            );
            fs::write(
                directory.join("clean-after-failure.js"),
                light_cljs::compile_with_macros(
                    "(defmacro clean [] true) (clean)",
                    if reuse { &mut reused } else { &mut fresh },
                )
                .unwrap(),
            )
            .unwrap();
            println!(
                "{name}: retained located duplicate-method rejection; subsequent clean macro compiled"
            );
        }
    }
}
