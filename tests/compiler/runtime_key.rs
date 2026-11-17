//! The per-build runtime key (docs/value-layout.md, section 1, light-cljs-1025):
//! the output writer writes each build's key for the placeholder of every
//! keyed name, derived from the output's text and level, and each runtime
//! registers it page-wide as it loads.
use super::optimization::execute;
use light_cljs::dev_build::{DevFile, emit_dev_build_of};
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

/// The program of entry `app.main` whose sources are `files`, by path, with
/// them as a dev build's sources.
fn namespace_program(files: &[(&str, &str)]) -> (Program, Vec<(std::path::PathBuf, String)>) {
    let root = crate::temp_dir::TempDir::new("runtime-key-sources");
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }
    light_cljs::compiler::analyze_namespace_entry_with_sources(
        "app.main",
        &[root.to_path_buf()],
        &mut light_cljs::MacroHost::new(),
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

fn dev_build(
    program: &Program,
    sources: &[(std::path::PathBuf, String)],
    build: Option<&str>,
) -> Vec<DevFile> {
    let urls = sources
        .iter()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let files = urls
        .iter()
        .zip(sources)
        .map(|(url, (_, text))| SourceFile { url, text })
        .collect::<Vec<_>>();
    emit_dev_build_of(program, &files, build)
}

fn file(files: &[DevFile], name: &str) -> String {
    files
        .iter()
        .find(|file| file.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .text
        .clone()
}

/// A dev build's key is one per build identity, its build of light-cljs.edn
/// and its entry (light-cljs-1045): an edit changes neither, another build
/// of light-cljs.edn with the same entry gets another key, and every global
/// of the build ends in it, so different dev builds load side by side into
/// one realm. Two copies of one build share the key; two different builds
/// whose keys match (one written with the other's) stop as the second
/// loads.
#[test]
fn dev_builds_have_a_key_per_build_identity() {
    let main = |n: u32| {
        format!(
            "(ns app.main) (deftype T [] ICounted (-count [_] {n}))
             (js/console.log (count (T.)) (.cljs$core$ICounted$_count$arity$1 (T.) nil))"
        )
    };
    let (one, one_sources) = namespace_program(&[("app/main.cljs", &main(1))]);
    let (two, two_sources) = namespace_program(&[("app/main.cljs", &main(2))]);
    let builds = [
        (None, dev_build(&one, &one_sources, None)),
        (None, dev_build(&two, &two_sources, None)),
        (Some("app"), dev_build(&one, &one_sources, Some("app"))),
        (
            Some("worker"),
            dev_build(&one, &one_sources, Some("worker")),
        ),
    ];
    for (build, files) in &builds {
        let key = light_cljs::dev_build::dev_key(*build, "app.main");
        let key = key.key();
        for name in ["runtime.js", "client.js", "app.main.js", "index.js"] {
            let text = file(files, name);
            assert!(
                !text.contains("$LKEY") && !text.contains("$LHASH"),
                "{name}"
            );
        }
        let (runtime, unit) = (file(files, "runtime.js"), file(files, "app.main.js"));
        assert!(runtime.starts_with(&format!("globalThis.__lightRuntime${key} = ")));
        assert_eq!(RuntimeKey::written(&runtime).unwrap().key(), key);
        assert!(
            unit.starts_with(&format!("globalThis.__lightDev${key}.define(")),
            "{unit}"
        );
        for keyed in [
            format!("cljs$core$ICounted$_count$arity$1${key}"),
            format!("globalThis.__lightApplicationRoots${key}["),
        ] {
            assert!(unit.contains(&keyed), "{keyed}: {unit}");
        }
        assert!(file(files, "client.js").contains(&format!(
            "globalThis.__lightDev = globalThis.__lightDev${key} = "
        )));
        assert!(file(files, "index.js").contains(&format!(
            "globalThis.__lightDevLoaded = globalThis.__lightDevLoaded${key} = "
        )));
    }
    let runtimes = builds
        .each_ref()
        .map(|(_, files)| file(files, "runtime.js"));
    let key = |runtime: &str| RuntimeKey::written(runtime).unwrap().key().to_owned();
    assert_eq!(runtimes[0], runtimes[1]);
    assert_ne!(key(&runtimes[0]), key(&runtimes[2]));
    assert_ne!(key(&runtimes[2]), key(&runtimes[3]));
    // worker's runtime, written with app's key.
    let forged = runtimes[3].replace(
        &format!("${}", key(&runtimes[3])),
        &format!("${}", key(&runtimes[2])),
    );
    let collision = format!(
        "Two light-cljs builds share the runtime key {}: give one a different :runtime-key",
        key(&runtimes[2])
    );
    assert_eq!(
        load_in_one_realm(&[
            &runtimes[2],
            &runtimes[2],
            &runtimes[3],
            &runtimes[0],
            &forged
        ]),
        format!("ok\nok\nok\nok\n{collision}\n")
    );
}

/// A host property whose name ends in a placeholder's spelling but is no
/// keyed name keeps that name at every level and in a dev build: read as a
/// member, written as a member or a string, and as a type's or a record's
/// field (the Sol review of light-cljs-1025).
#[test]
fn ordinary_property_names_keep_their_spelling() {
    let body = "(defrecord R [a$LKEY]) (deftype T [b$LKEY])
        (let [o (js-obj \"foo$LKEY\" 7 \"bar$LHASH\" 8) r (->R 1) t (T. 2)]
          (set! (.-baz$LKEY o) 9)
          (pr-str [(.-foo$LKEY o) (.-bar$LHASH o) (aget o \"baz$LKEY\") (vec (sort (js-keys o)))
                   (:a$LKEY r) (.-a$LKEY r) (.-b$LKEY t) (get (assoc r :a$LKEY 3) :a$LKEY)]))";
    let expected = "[7 8 9 [\"bar$LHASH\" \"baz$LKEY\" \"foo$LKEY\"] 1 1 2 3]";
    for level in OptimizationLevel::ALL {
        assert_eq!(
            execute(&format!("console.log({});", release(body, level))),
            format!("{expected}\n"),
            "{level}"
        );
    }
    let (program, sources) = namespace_program(&[(
        "app/main.cljs",
        &format!("(ns app.main) (js/console.log {body})"),
    )]);
    let dir = crate::temp_dir::TempDir::new("runtime-key-names");
    let [none, dev] = crate::dev_execution::none_and_dev(&program, &sources, &dir, false);
    // The record types print with the field keywords they cache, whose own
    // keyed fields (light-cljs-1011) end in each build's key: `$`, eight
    // characters of [0-9a-z] and the closing quote of the property name.
    let unkeyed = |output: &String| {
        let (mut text, mut rest) = (String::new(), output.as_str());
        while let Some(at) = rest.find('$') {
            text.push_str(&rest[..=at]);
            rest = &rest[at + 1..];
            let keyed = rest.as_bytes().get(..9).is_some_and(|key| {
                key[8] == b'\''
                    && key[..8]
                        .iter()
                        .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
            });
            if keyed {
                text.push_str("<key>'");
                rest = &rest[9..];
            }
        }
        text + rest
    };
    assert_eq!(dev.as_ref().map(unkeyed), none.as_ref().map(unkeyed));
    assert!(none.as_ref().unwrap().contains("bar$LHASH"), "{none:?}");
}

/// A chunked build's chunks share one key, derived from all of them: a
/// constant only a feature's chunk holds changes the key and the hash
/// startup registers (the Sol review of light-cljs-1025).
#[test]
fn chunked_builds_derive_one_key_from_every_chunk() {
    let files = |n: u32| {
        vec![
            (
                "app/main.cljs",
                "(ns app.main (:require [light.lazy :as lazy]))
                 (deftype T [] ICounted (-count [_] 1))
                 (def editor (lazy/loadable app.editor/start))
                 (fn [] [(count (T.)) editor])"
                    .to_owned(),
            ),
            (
                "app/editor.cljs",
                format!(
                    "(ns app.editor) (deftype E [] ICounted (-count [_] {n}))
                     (defn start [] [(count (E.)) (.-cljs$core$ICounted$ (E.))])"
                ),
            ),
        ]
    };
    let build = |n: u32| {
        let files = files(n);
        let borrowed = files
            .iter()
            .map(|(path, text)| (*path, text.as_str()))
            .collect::<Vec<_>>();
        let (program, _) = namespace_program(&borrowed);
        prepare_application(
            &program,
            ApplicationOptions::new(OptimizationLevel::Advanced),
        )
        .emit_chunks()
    };
    let (first, again, other) = (build(2), build(2), build(3));
    assert_eq!(first, again);
    assert!(
        first.files.len() >= 2,
        "{:?}",
        first
            .files
            .iter()
            .map(|file| &file.name)
            .collect::<Vec<_>>()
    );
    let key = RuntimeKey::written(&first.startup().source).expect("startup registers the key");
    for chunk in &first.files {
        assert!(
            !chunk.source.contains("$LKEY") && !chunk.source.contains("$LHASH"),
            "{}",
            chunk.name
        );
    }
    let editor = first
        .files
        .iter()
        .find(|chunk| chunk.name == "app.editor.js")
        .expect("the feature's chunk");
    assert!(
        editor.source.contains(&format!("${}", key.key())),
        "the feature's keyed names have startup's key:\n{}",
        editor.source
    );
    let changed = RuntimeKey::written(&other.startup().source).unwrap();
    assert_eq!(first.startup().source.len(), other.startup().source.len());
    assert_ne!(changed.key(), key.key());
    assert_ne!(changed.hash(), key.hash());
}

/// Light-cljs runtimes on one page (docs/value-layout.md, section 1, "Two
/// runtimes on one page"; light-cljs-1018): tests/support/two_runtimes.mjs
/// builds tests/fixtures/two-runtimes with this compiler (two apps, one of
/// them also with a `:runtime-key`, with another constant and as a dev
/// build) and loads them in pairs, and with a forced key collision, into
/// one Node process per page. With `--chromium NPM_TOOLS` it runs the same
/// pages in headless Chromium.
#[test]
fn runtimes_on_one_page_keep_apart_unless_they_share_a_key() {
    let node = std::env::var_os("NODE").unwrap_or_else(|| "node".into());
    let output = std::process::Command::new(node)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/two_runtimes.mjs"
        ))
        .args(["--light", env!("CARGO_BIN_EXE_light-cljs")])
        .output()
        .expect("the two-runtime pages require Node");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("9 of 9 scenarios as expected"),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Two loads of the compile-time runtime in one engine (docs/value-layout.md,
/// section 4): they share its fixed key, so each reads the other's values
/// through their keyed slots, but each stamps its values with its own
/// brand, so neither's form bridge nor `inspect` takes the other's vector
/// for a language value. In the evaluator's engine (V8, or QuickJS in the
/// QuickJS reference) and in Node.
#[test]
fn compile_time_runtime_loads_keep_their_own_brands() {
    use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
    let probe = "(rt, other) => {
        const mine = rt.vectorFromArray([1, 2]), theirs = other.vectorFromArray([1, 2]);
        const encodes = (bridge, value) => {
          try { return 'encoded ' + bridge.encode(value, 100, 10)[0]; } catch (error) { return error.message; }
        };
        return JSON.stringify([rt.inspect(mine), rt.inspect(theirs), other.inspect(theirs), other.inspect(mine),
          encodes(rt.compilerForms, mine), encodes(rt.compilerForms, theirs),
          encodes(other.compilerForms, theirs), encodes(other.compilerForms, mine),
          rt.core.count(theirs), other.core.count(mine), rt.core.prStr(theirs)]);
      }";
    let rejected = "Unsupported macro result arbitrary object (including promise) at $";
    let expected = format!(
        r#"[{{"kind":"vector","items":[1,2]}},null,{{"kind":"vector","items":[1,2]}},null,"encoded vector","{rejected}","encoded vector","{rejected}",2,2,"[1 2]"]"#
    );
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let observation = evaluator
        .evaluate_js(&format!("({probe})(rt, {RUNTIME_SOURCE})"))
        .unwrap();
    assert_eq!(
        evaluator.string(observation).unwrap(),
        expected,
        "evaluator"
    );
    assert_eq!(
        execute(&format!(
            "const load = () => {RUNTIME_SOURCE}; console.log(({probe})(load(), load()));"
        )),
        format!("{expected}\n"),
        "Node"
    );
}
