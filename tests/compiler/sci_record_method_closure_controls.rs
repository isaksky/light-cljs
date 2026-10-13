//! Finite diagnostic controls only. The original SCI closure outcome stays unresolved.
use light_cljs::MacroHost;
use std::fs;
const ORIGINAL: &str = "(defprotocol P (p [x])) (let [kept [40]] (defrecord R [x] P (p [this] (fn [] [x (first kept) (= this this)])))) ((p (->R 2)))";
const WARMUP: &str = "(defmacro phase-ready [] true) (phase-ready)";

#[test]
#[ignore = "Explicit finite original source and phase-boundary diagnostic control"]
fn original_closure_and_ownership_controls() {
    let root = std::path::PathBuf::from(std::env::var("LIGHT_SCI_CLOSURE_CONTROL_ROOT").unwrap());
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    let mut reused = MacroHost::new();
    fs::write(
        root.join("warmup.js"),
        light_cljs::compile_with_macros(WARMUP, &mut reused).unwrap(),
    )
    .unwrap();
    for reuse in [false, true] {
        for (name, source, rejection) in [
            ("original-complete", format!("(ns user)\n(defmacro upstream [] (list 'quote (do {ORIGINAL})))\n(defn run [] (upstream))\n"), Some("unavailable definition: user/kept")),
            ("original-split", format!("(ns user)\n{}(defmacro upstream [] (list 'quote ((p (->R 2)))))\n(defn run [] (upstream))\n", ORIGINAL.strip_suffix("((p (->R 2)))").unwrap()), Some("unavailable definition: user/->R")),
            ("ordinary-declared-missing", "(ns user)\n(declare absent) (defmacro upstream [] absent) (defn run [] (upstream))\n".to_string(), Some("unavailable definition: user/absent")),
            // This is a distinct source/setup control, never a substitute original adapter.
            ("explicit-global-control", format!("(ns user)\n(def kept [99])\n(defmacro upstream [] (list 'quote (do {ORIGINAL})))\n(defn run [] (upstream))\n"), None),
            ("lexical-owner-control", "(ns user)\n(let [kept [40]] (defn saved [] (first kept)))\n(defmacro upstream [] (assert (= 40 (saved))) (list 'quote (saved)))\n(defn run [] (upstream))\n".to_string(), None),
        ] {
            let variant = format!("{name}-{}", if reuse { "reused" } else { "fresh" });
            let directory = root.join(&variant);
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("user.cljs"), &source).unwrap();
            let mut fresh = MacroHost::new();
            let result = light_cljs::compile_namespace_with_macros("user", std::slice::from_ref(&directory), if reuse { &mut reused } else { &mut fresh });
            match (result, rejection) {
                (Err(error), Some(expected)) => {
                    let error = error.to_string();
                    fs::write(directory.join("error.txt"), &error).unwrap();
                    assert!(error.contains(expected), "{variant}: {error}");
                    assert!(error.contains("user.cljs:"), "{variant}: {error}");
                    println!("{variant}: retained expected located rejection {expected}");
                }
                (Ok(javascript), None) => {
                    fs::write(directory.join("compiled.js"), &javascript).unwrap();
                    let expected = if name == "explicit-global-control" { r#"{"$vector":[2,99,true]}"# } else { "40" };
                    let captured = javascript.replacen(" = // Original bounded runtime", " = globalThis.lightRuntime = // Original bounded runtime", 1);
                    let script = format!("(() => {{ const run = {captured}; return ({}) (globalThis.lightRuntime)('control',observe=>run(observe)); }})()", include_str!("../support/sci_execution_observation.js"));
                    let runtime = rquickjs::Runtime::new().unwrap();
                    let context = rquickjs::Context::full(&runtime).unwrap();
                    let observation = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
                    fs::write(directory.join("quickjs.json"), &observation).unwrap();
                    assert!(observation.contains(&format!("\"value\":{expected}")), "{variant}: {observation}");
                    use std::{io::Write, process::{Command, Stdio}};
                    let mut node = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into())).arg("-").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
                    node.stdin.take().unwrap().write_all(format!("console.log({script});").as_bytes()).unwrap();
                    let result = node.wait_with_output().unwrap();
                    fs::write(directory.join("node.stdout"), &result.stdout).unwrap();
                    fs::write(directory.join("node.stderr"), &result.stderr).unwrap();
                    assert!(result.status.success() && result.stderr.is_empty());
                    assert_eq!(String::from_utf8(result.stdout).unwrap().trim(), observation);
                    println!("{variant}: actual MacroHost then Node/QuickJS observed {expected}");
                }
                (Err(error), None) => panic!("{variant}: unexpected {error}"),
                (Ok(_), Some(expected)) => panic!("{variant}: lost rejection {expected}"),
            }
        }
    }
}
