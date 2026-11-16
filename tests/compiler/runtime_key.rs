//! The per-build runtime key (docs/value-layout.md, section 1, light-cljs-1025):
//! the output writer writes each build's key for the placeholder of every
//! keyed name, derived from the output's text and level, and each runtime
//! registers it page-wide as it loads.
use super::optimization::execute;
use light_cljs::dev_build::emit_dev_build;
use light_cljs::runtime_key::{KEY_LENGTH, RuntimeKey, unkeyed};
use light_cljs::source_map::SourceFile;
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze, parse, prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn release(source: &str, level: OptimizationLevel) -> String {
    prepare_application(&program(source), ApplicationOptions::new(level)).emit_js()
}

/// A program type implementing a program and a core protocol, read through
/// keyed members, a slot in raw JavaScript, a string that names no keyed slot,
/// and strings that spell the placeholders, which no output changes.
const PROGRAM: &str = "(defprotocol P (m [x])) (deftype T [v] P (m [_] v) ICounted (-count [_] 3))
    (let [t (T. 7)]
      (pr-str [(m t) (.cljs$user$P$m$arity$1 t t) (js* \"~{}.cljs$user$P$m$arity$1$LKEY(~{})\" t t)
               (count t) (undefined? (aget t \"cljs$user$P$m$arity$1\"))
               (undefined? (aget t \"cljs$user$P$m$arity$1$LKEY\"))
               \"$LKEY\" \"x$LKEY\" \"$LHASH\" (keyword \"$LKEY\") '$LKEY]))";

const PRINTED: &str = "[7 7 7 3 true true \"$LKEY\" \"x$LKEY\" \"$LHASH\" :$LKEY $LKEY]";

/// The key the output writer wrote into `js`: eight base-36 characters.
fn key(js: &str) -> RuntimeKey {
    let key = RuntimeKey::written(js).expect("the output registers a key");
    assert_eq!(key.key().len(), KEY_LENGTH);
    assert!(
        key.key()
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()),
        "{}",
        key.key()
    );
    key
}

#[test]
fn release_output_writes_its_own_key_for_the_placeholder() {
    let mut keys = Vec::new();
    for level in OptimizationLevel::ALL {
        let js = release(PROGRAM, level);
        // Only strings of the program spell a placeholder, escaped.
        assert!(!js.contains("$LKEY") && !js.contains("$LHASH"), "{level}");
        assert!(
            js.contains("\\u0024LKEY") && js.contains("\\u0024LHASH"),
            "{level}"
        );
        let written = key(&js);
        assert!(
            js.contains(&format!("cljs$user$P$m$arity$1${}", written.key())),
            "{level}"
        );
        assert!(
            unkeyed(&js).contains("cljs$user$P$m$arity$1$LKEY"),
            "{level}"
        );
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{PRINTED}\n"),
            "{level}"
        );
        // The same program at the same level gets the same key, the text
        // written as source maps see it and as an ES module its own.
        let prepared = prepare_application(&program(PROGRAM), ApplicationOptions::new(level));
        assert_eq!(prepared.emit_js(), js, "{level}");
        assert_eq!(prepared.emit_js_mapped().0, js, "{level}");
        let module = prepared.emit_esm();
        assert!(!module.contains("$LKEY"), "{level}");
        assert_ne!(key(&module), written, "{level}");
        keys.push(written);
    }
    // Each level writes another output, and so another key.
    assert_ne!(keys[0], keys[1]);
    assert_ne!(keys[1], keys[2]);
    assert_ne!(keys[0], keys[2]);
    // So does another program, even one differing in a constant.
    for level in OptimizationLevel::ALL {
        let other = release(&PROGRAM.replace("(T. 7)", "(T. 8)"), level);
        assert_ne!(
            key(&other).key(),
            key(&release(PROGRAM, level)).key(),
            "{level}"
        );
    }
}

/// `scripts` loaded in order into one fresh realm, each printed as `ok` or
/// as the message it threw.
fn load_in_one_realm(scripts: &[&str]) -> String {
    let dir = crate::temp_dir::TempDir::new("runtime-key-realm");
    let files = scripts
        .iter()
        .enumerate()
        .map(|(index, script)| {
            let path = dir.join(format!("{index}.js"));
            std::fs::write(&path, script).unwrap();
            format!("{:?}", path.to_string_lossy())
        })
        .collect::<Vec<_>>()
        .join(",");
    execute(&format!(
        "const vm = require('node:vm'), fs = require('node:fs');
         const realm = vm.createContext({{console: {{log() {{}}}}}});
         for (const file of [{files}]) {{
           try {{ vm.runInContext(fs.readFileSync(file, 'utf8'), realm); console.log('ok'); }}
           catch (error) {{ console.log(error.message); }}
         }}"
    ))
}

#[test]
fn outputs_register_their_keys_page_wide() {
    let a = release(PROGRAM, OptimizationLevel::Advanced);
    let b = release("(pr-str (conj [1 2] 3))", OptimizationLevel::Advanced);
    let (key_a, key_b) = (key(&a), key(&b));
    assert_ne!(key_a.key(), key_b.key());
    // `b` written with `a`'s key: two different outputs whose keys collide.
    let colliding = b.replace(&format!("${}", key_b.key()), &format!("${}", key_a.key()));
    assert_eq!(key(&colliding).key(), key_a.key());
    assert_ne!(key(&colliding).hash(), key_a.hash());
    let collision = format!(
        "Two light-cljs builds share the runtime key {}: give one a different :runtime-key",
        key_a.key()
    );
    assert_eq!(
        load_in_one_realm(&[&a, &a, &b]),
        "ok\nok\nok\n",
        "the same output twice and another build load"
    );
    assert_eq!(
        load_in_one_realm(&[&a, &colliding, &a]),
        format!("ok\n{collision}\nok\n")
    );
    // The registry is a `Map` of each key to the hash of its output.
    assert_eq!(
        execute(&format!(
            "{a}; {b}; const keys = globalThis[Symbol.for('light-cljs.runtime-keys')];
             console.log(keys instanceof Map, keys.size, keys.get('${}'), keys.get('${}'));",
            key_a.key(),
            key_b.key()
        )),
        format!(
            "true 2 {} {}\n",
            key_a.hash().unwrap(),
            key_b.hash().unwrap()
        )
    );
    // Two copies of one build share its key and recognise each other's
    // values; another build does not ("Decisions for the user" 1).
    let shared = "(if (exists? js/shared) (pr-str [(count js/shared) (vector? js/shared)])
                    (do (set! js/shared [1 2 3]) :first))";
    let (copy, other) = (
        release(shared, OptimizationLevel::Advanced),
        release(
            &shared.replace(":first", ":other"),
            OptimizationLevel::Advanced,
        ),
    );
    assert_ne!(key(&copy).key(), key(&other).key());
    assert_eq!(
        execute(&format!(
            "console.log(String({copy})); console.log({copy});
             try {{ console.log({other}); }} catch (error) {{ console.log(error.message); }}"
        )),
        ":first\n[3 true]\ncount not supported on this type\n"
    );
}

#[test]
fn dev_builds_share_the_runtimes_key_and_register_their_identity() {
    let dev = |entry: &str| {
        let root = crate::temp_dir::TempDir::new("runtime-key-dev");
        let path = root.join("src").join(format!("{entry}.cljs"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = format!(
            "(ns {entry}) (deftype T [] ICounted (-count [_] 2))
             (js/console.log (count (T.)) (.cljs$core$ICounted$_count$arity$1 (T.) nil))"
        );
        std::fs::write(&path, &text).unwrap();
        let program = light_cljs::compiler::analyze_namespace_entry(
            entry,
            &[root.join("src")],
            &mut light_cljs::MacroHost::new(),
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let url = path.to_string_lossy().into_owned();
        emit_dev_build(
            &program,
            &[SourceFile {
                url: &url,
                text: &text,
            }],
        )
    };
    let (one, two) = (dev("one"), dev("two"));
    let runtime = |files: &[light_cljs::dev_build::DevFile]| {
        files
            .iter()
            .find(|file| file.name == "runtime.js")
            .unwrap()
            .text
            .clone()
    };
    let unit = |files: &[light_cljs::dev_build::DevFile], name: &str| {
        files
            .iter()
            .find(|file| file.name == name)
            .unwrap()
            .text
            .clone()
    };
    let key = RuntimeKey::dev_units();
    for (files, name) in [(&one, "one.js"), (&two, "two.js")] {
        let (runtime, unit) = (runtime(files), unit(files, name));
        assert!(!runtime.contains("$LKEY") && !runtime.contains("$LHASH"));
        assert!(!unit.contains("$LKEY"), "{unit}");
        assert!(runtime.contains(&format!("${}", key.key())));
        assert!(unit.contains(&format!("${}", key.key())), "{unit}");
        assert_eq!(RuntimeKey::written(&runtime).unwrap().key(), key.key());
    }
    // Two different dev builds have the same key and their own identities.
    let (runtime_one, runtime_two) = (runtime(&one), runtime(&two));
    assert_ne!(
        RuntimeKey::written(&runtime_one).unwrap().hash(),
        RuntimeKey::written(&runtime_two).unwrap().hash()
    );
    let collision = format!(
        "Two light-cljs builds share the runtime key {}: give one a different :runtime-key",
        key.key()
    );
    assert_eq!(
        load_in_one_realm(&[&runtime_one, &runtime_one, &runtime_two]),
        format!("ok\nok\n{collision}\n")
    );
}
