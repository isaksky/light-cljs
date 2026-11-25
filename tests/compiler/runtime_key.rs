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
               \"$LKEY\" \"x$LKEY\" \"$LHASH\" \"$LNAMES\" (keyword \"$LKEY\") '$LKEY]))";

const PRINTED: &str =
    "[7 7 7 3 true true \"$LKEY\" \"x$LKEY\" \"$LHASH\" \"$LNAMES\" :$LKEY $LKEY]";

/// The code of the short name of `stem` in `js`, `Advanced` output, as the
/// table the output writer wrote for the runtime's `protocolSlot` gives it
/// ("stem code stem code").
fn short_name(js: &str, stem: &str) -> Option<String> {
    js.match_indices(stem).find_map(|(at, _)| {
        let before = js[..at].chars().next_back()?;
        let rest = js[at + stem.len()..].strip_prefix(' ')?;
        let end = rest.find([' ', '"'])?;
        matches!(before, '"' | ' ').then(|| rest[..end].to_owned())
    })
}

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
            js.contains("\\u0024LKEY")
                && js.contains("\\u0024LHASH")
                && js.contains("\\u0024LNAMES"),
            "{level}"
        );
        let written = key(&js);
        let long = format!("cljs$user$P$m$arity$1${}", written.key());
        if level == OptimizationLevel::Advanced {
            // `Advanced` writes keyed names short, `$`, the key and a code
            // (light-cljs-1048), and gives `protocolSlot` the core slot's
            // code. The program protocol's slot is not in that table: the
            // text names it where the runtime needs it, its dispatcher's
            // `names` here (light-cljs-1058).
            let count =
                short_name(&js, "cljs$core$ICounted$_count$arity$1").expect("the core slot's code");
            assert!(
                js.contains(&format!(".${}{count}(", written.key())),
                "{level}"
            );
            assert!(
                short_name(&js, "cljs$user$P$m$arity$1").is_none(),
                "{level}"
            );
            let named = format!("[\"${}", written.key());
            let at = js.find(&named).expect("the dispatcher's names") + named.len();
            let code = &js[at..at + js[at..].find('"').unwrap()];
            assert!(!js.contains(&long), "{level}");
            assert!(
                js.contains(&format!(".${}{code}(", written.key())),
                "{level}"
            );
        } else {
            assert!(js.contains(&long), "{level}");
            assert!(
                unkeyed(&js).contains("cljs$user$P$m$arity$1$LKEY"),
                "{level}"
            );
        }
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
        output.status.success() && stdout.contains("11 of 11 scenarios as expected"),
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
    use light_cljs::evaluator::RUNTIME_SOURCE;
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
    #[cfg(light_v8)]
    {
        use light_cljs::evaluator::{Evaluator, EvaluatorOptions};

        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let observation = evaluator
            .evaluate_js(&format!("({probe})(rt, {RUNTIME_SOURCE})"))
            .unwrap();
        assert_eq!(
            evaluator.string(observation).unwrap(),
            expected,
            "evaluator"
        );
    }
    assert_eq!(
        execute(&format!(
            "const load = () => {RUNTIME_SOURCE}; console.log(({probe})(load(), load()));"
        )),
        format!("{expected}\n"),
        "Node"
    );
}

/// What `source` prints at each level, with each output's key written as the
/// placeholder.
fn printed_at_every_level(source: &str) -> Vec<String> {
    OptimizationLevel::ALL
        .iter()
        .map(|&level| {
            let js = release(source, level);
            key(&js).unapply(&execute(&format!("console.log({js});")))
        })
        .collect()
}

/// `Advanced`'s short names (light-cljs-1048) leave alone what raw text or a
/// string can reach: a protocol name a string must escape keeps its whole
/// name, as `protocolSlot` builds it; raw text that builds a keyed name
/// from its stem keeps every name whole; and a value's JSON leaves out only
/// its keyed names, not a property that holds the key elsewhere.
#[test]
fn short_names_leave_what_raw_text_and_strings_reach() {
    for (source, expected) in [
        (
            "(defprotocol Pπ (m [x])) (deftype T [] Pπ (m [_] 7))
             (let [t (T.)] (pr-str [(m t) (.cljs$user$Pπ$m$arity$1 t t)]))",
            "[7 7]",
        ),
        (
            "(deftype T [] ICounted (-count [_] 3))
             (let [t (T.)]
               (pr-str [(count t) (js* \"~{}['cljs$core$ICounted$_count$arity$1' + '$LKEY'](~{})\" t t)]))",
            "[3 3]",
        ),
        (
            "(let [v (vec [1 2])]
               (aset v (js* \"'user' + '$LKEY' + 'extra'\") 7)
               (.join (js/Object.keys (js/JSON.parse (js/JSON.stringify v))) \" \"))",
            "user$LKEYextra",
        ),
    ] {
        for (level, printed) in OptimizationLevel::ALL.iter().zip(printed_at_every_level(source)) {
            assert_eq!(printed, format!("{expected}\n"), "at {level}: {source}");
        }
    }
    // Raw text that spells keyed names only as members keeps short names.
    assert!(light_cljs::runtime_key::keyed_by_member(
        "~{}.cljs$user$P$m$arity$1$LKEY(~{})"
    ));
    assert!(!light_cljs::runtime_key::keyed_by_member(
        "'cljs$user$P$m$arity$1$LKEY'"
    ));
    assert!(!light_cljs::runtime_key::keyed_by_member("x + '$LKEY'"));
}

/// At `Advanced` a program protocol's slots are named where the runtime
/// needs them (light-cljs-1058): each dispatcher's `names`, and each method
/// entry that does not name its dispatcher, in place of its key or, for an
/// `extend-type` of a value that may be a host type's name, after its
/// function. The slot-code table `protocolSlot` reads holds none of them,
/// and every level answers alike: types with a dispatcher's entries and one
/// whose protocol's dispatcher only direct calls replace (`Q`), records,
/// `reify`, `specify!`, `extend-type` of a host constructor, a host type's
/// name and a runtime type, a variadic and a metadata-extended method,
/// `satisfies?`, `implements?` and a slot named by member.
#[test]
fn program_protocol_slots_are_named_by_the_text() {
    let source = "(defprotocol P (m [x] [x y]) (v [x & ys]))
        (defprotocol M :extend-via-metadata true (mm [x]))
        (defprotocol Q (q [x]))
        (deftype T [n] P (m [_] n) (m [_ y] (+ n y)) (v [_ a b] [a b]) Q (q [_] :t) ICounted (-count [_] n))
        (defrecord R [n] P (m [_] (* 2 n)) (m [_ y] (* y n)))
        (extend-type js/Date P (m [_] :date) (m [_ y] y))
        (extend-type number P (m [x y] (+ x y)))
        (extend-type PersistentVector P (m [x] (count x)) (m [x y] (conj x y)))
        (def r (reify P (m [_] :reify) (m [_ y] [:reify y]) M (mm [_] :mm)))
        (def s (specify! #js {} P (m [_] :specify) (m [_ y] y)))
        (def t (T. 3))
        (pr-str [(m t) (m t 1) (v t 1 2 3) (q t) (count t) (m (->R 2)) (m (->R 2) 5) (m (js/Date. 0))
                 (m 4 5) (m [1 2]) (m [1] 2) (m r) (m r 1) (mm r) (m s) (m s 9) (satisfies? P t)
                 (satisfies? P (js/Date. 0)) (satisfies? Q r) (implements? P t)
                 (.cljs$user$P$m$arity$2 t t 10) (mm (with-meta [] {`mm (fn [_] :meta)}))])";
    let expected = "[3 4 [nil (1 2 3)] :t 3 4 10 :date 9 2 [1 2] :reify [:reify 1] :mm :specify 9 \
                    true true false true 13 :meta]\n";
    for (level, printed) in OptimizationLevel::ALL
        .iter()
        .zip(printed_at_every_level(source))
    {
        assert_eq!(printed, expected, "at {level}");
    }
    let js = release(source, OptimizationLevel::Advanced);
    assert!(
        short_name(&js, "cljs$core$ICounted$_count$arity$1").is_some(),
        "{js}"
    );
    for method in [
        "P$m$arity$1",
        "P$m$arity$2",
        "P$v$arity$3",
        "Q$q$arity$1",
        "M$mm$arity$1",
    ] {
        assert!(
            short_name(&js, &format!("cljs$user${method}")).is_none(),
            "{method}: {js}"
        );
    }
}

/// Sol finding #3 (light-cljs-1120): two copies of the same build share keyed
/// kinds and protocol slots while keeping `hostExt` local; `seq` and
/// `LazySeq.seqSlot` must check slot identity against this runtime's built-in
/// slots so `specify!` overrides on `Cons` and `EmptyList` in copy 1 are
/// honored in copy 2.
#[test]
fn same_build_copies_honor_seq_overrides() {
    let source = "(if (exists? js/sharedCons)
                    (pr-str [(first (seq (lazy-seq js/sharedCons)))
                             (first (seq js/sharedEmpty))
                             (first (seq (lazy-seq js/sharedEmpty)))])
                    (do
                      (set! js/sharedCons (specify! (cons 1 (list 2))
                                            ISeqable (-seq [_] (array-seq #js [42]))))
                      (set! js/sharedEmpty (specify! '()
                                             ISeqable (-seq [_] (array-seq #js [99]))))
                      :first))";
    for level in OptimizationLevel::ALL {
        let js = release(source, level);
        assert_eq!(
            execute(&format!("console.log(String({js})); console.log({js});")),
            ":first\n[42 99 99]\n",
            "at {level}"
        );
    }
}

/// light-cljs-1122 (Change 1): two copies of the same build share keyed kinds
/// and protocol slots; `first` and `LazySeq`/`ChunkedCons` fast paths must check
/// slot identity against this runtime's built-in slots so `specify!` overrides
/// in copy 1 are honored when read in copy 2 (including when copy 2 constructs
/// local outer `LazySeq`/`ChunkedCons` wrappers around copy 1's operands).
#[test]
fn same_build_copies_honor_first_overrides() {
    let source = "(if (exists? js/sharedFirstVals)
                    (let [f first
                          [c ac cc-ac cc is cs ls-f ls-s ls-c] js/sharedFirstVals
                          local-cc (chunk-cons ac nil)
                          local-ls-c (lazy-seq c)
                          local-ls-is (lazy-seq is)]
                      (pr-str [(f c) (f cc-ac) (f cc) (f is) (f cs) (f ls-f) (f ls-s) (f ls-c)
                               (f local-cc) (f local-ls-c) (f local-ls-is)]))
                    (let [c (specify! (cons 1 '(2)) ISeq (-first [_] :c))
                          ac (specify! (array-chunk #js [1 2]) IIndexed (-nth [_ _] :ac))
                          cc-ac (chunk-cons ac nil)
                          cc (specify! (chunk-cons (array-chunk #js [1 2]) nil) ISeq (-first [_] :cc))
                          is (specify! (array-seq #js [1 2]) ISeq (-first [_] :is))
                          cs (specify! (seq (vec (range 70))) ISeq (-first [_] :cs))
                          ls-f (specify! (lazy-seq (cons 1 nil)) ISeq (-first [_] :ls-f))
                          ls-s (specify! (lazy-seq (cons 1 nil)) ISeqable (-seq [_] nil))
                          ls-c (lazy-seq c)]
                      (set! js/sharedFirstVals #js [c ac cc-ac cc is cs ls-f ls-s ls-c])
                      :first))";
    for level in OptimizationLevel::ALL {
        let js = release(source, level);
        assert_eq!(
            execute(&format!("console.log(String({js})); console.log({js});")),
            ":first\n[:c :ac :cc :is :cs :ls-f nil :c :ac :c :is]\n",
            "at {level}"
        );
    }
}

/// light-cljs-1122 (Change 2): two copies of the same build share keyed kinds
/// and protocol slots; `rest`, `next`, `Cons.nextSlot`, `ChunkedCons`, and
/// `LazySeq` fast paths must honor `specify!` overrides across copies,
/// including local outer `Cons`/`ChunkedCons`/`LazySeq` wrapping foreign operands.
#[test]
fn same_build_copies_honor_next_and_rest_overrides() {
    let source = "(if (exists? js/sharedNextRestVals)
                    (let [r rest
                          nx next
                          [c-r c-n ac-c cc-ac ac-d cc-ad cc-r cc-n ls-r ls-n ls-s ls-cr] js/sharedNextRestVals
                          local-cc-c (chunk-cons ac-c '(:local-more))
                          local-cc-d (chunk-cons ac-d nil)
                          local-cons (cons 0 c-r)
                          local-ls-r (lazy-seq c-r)
                          local-ls-n (lazy-seq c-n)]
                      (pr-str [(first (r c-r)) (first (nx c-r)) (first (nx c-n))
                               (first (r cc-ac)) (first (nx cc-ac))
                               (first (r cc-ad)) (first (nx cc-ad))
                               (first (r cc-r)) (first (nx cc-r)) (first (nx cc-n))
                               (first (r ls-r)) (first (nx ls-n)) (nx ls-s) (first (r ls-cr))
                               (first (r local-cc-c)) (first (nx local-cc-c))
                               (first (r local-cc-d)) (first (nx local-cc-d))
                               (first (r (nx local-cons)))
                               (first (r local-ls-r)) (first (nx local-ls-n))]))
                    (let [c-r (specify! (cons 1 '(2)) ISeq (-rest [_] '(:cr)))
                          c-n (specify! (cons 1 '(2)) INext (-next [_] '(:cn)))
                          ac-c (specify! (array-chunk #js [1 2]) ICounted (-count [_] 1))
                          cc-ac (chunk-cons ac-c '(:more))
                          ac-d (specify! (array-chunk #js [1 2]) IChunk (-drop-first [_] (array-chunk #js [:drop])))
                          cc-ad (chunk-cons ac-d nil)
                          cc-r (specify! (chunk-cons (array-chunk #js [1 2]) nil) ISeq (-rest [_] '(:ccr)))
                          cc-n (specify! (chunk-cons (array-chunk #js [1 2]) nil) INext (-next [_] '(:ccn)))
                          ls-r (specify! (lazy-seq (cons 1 '(2))) ISeq (-rest [_] '(:lsr)))
                          ls-n (specify! (lazy-seq (cons 1 '(2))) INext (-next [_] '(:lsn)))
                          ls-s (specify! (lazy-seq (cons 1 '(2))) ISeqable (-seq [_] nil))
                          ls-cr (lazy-seq c-r)]
                      (set! js/sharedNextRestVals #js [c-r c-n ac-c cc-ac ac-d cc-ad cc-r cc-n ls-r ls-n ls-s ls-cr])
                      :first))";
    for level in OptimizationLevel::ALL {
        let js = release(source, level);
        assert_eq!(
            execute(&format!("console.log(String({js})); console.log({js});")),
            ":first\n[:cr :cr :cn :more :more :drop :drop :ccr :ccr :ccn :lsr :lsn nil :cr :local-more :local-more :drop :drop :cr :cr :cn]\n",
            "at {level}"
        );
    }
}

/// Sol finding #3 (light-cljs-1122): two copies of the same build share keyed
/// protocol markers but have independent `hostExt.seq` flags. A partial `ISeq`
/// `specify!` installed in copy B on a collection created in copy A must still
/// throw for the unimplemented `ISeq` method when called from copy A.
#[test]
fn same_build_copies_preserve_partial_iseq_declarations() {
    let source = "(if (exists? js/sharedPartialISeq)
                    (let [[v-for-rest rest-fn v-for-first first-fn] js/sharedPartialISeq]
                      (specify! v-for-rest ISeq (-first [_] :b-first))
                      (specify! v-for-first ISeq (-rest [_] '(:b-rest)))
                      (pr-str [(try (rest-fn v-for-rest) (catch js/Error e (.-message e)))
                               (try (first-fn v-for-first) (catch js/Error e (.-message e)))]))
                    (let [v1 [1 2]
                          v2 [3 4]]
                      (set! js/sharedPartialISeq #js [v1 (fn [x] (rest x)) v2 (fn [x] (first x))])
                      :setup))";
    for level in OptimizationLevel::ALL {
        let js = release(source, level);
        assert_eq!(
            execute(&format!("console.log(String({js})); console.log({js});")),
            ":setup\n[\"No protocol method ISeq/-rest defined for this value\" \"No protocol method ISeq/-first defined for this value\"]\n",
            "at {level}"
        );
    }
}

/// Sol finding #9 (light-cljs-1122): values from a different build (distinct
/// runtime key) fall back through host iteration for `first`, `rest`, and `next`.
#[test]
fn different_build_seq_operations_use_host_iteration() {
    let producer = "(do (set! js/foreignSeqs #js [(cons 1 '(2 3)) (list 4 5) (chunk-cons (array-chunk #js [6 7]) nil) (lazy-seq (cons 8 '(9)))]) :produced)";
    let consumer = "(let [[c l cc ls] js/foreignSeqs]
                      (pr-str [(first c) (vec (rest c)) (vec (next c))
                               (first l) (vec (rest l)) (vec (next l))
                               (first cc) (vec (rest cc)) (vec (next cc))
                               (first ls) (vec (rest ls)) (vec (next ls))]))";
    for level in OptimizationLevel::ALL {
        let p_js = release(producer, level);
        let c_js = release(consumer, level);
        assert_ne!(key(&p_js), key(&c_js), "distinct builds at {level}");
        assert_eq!(
            execute(&format!(
                "console.log(String({p_js})); console.log({c_js});"
            )),
            ":produced\n[1 [2 3] [2 3] 4 [5] [5] 6 [7] [7] 8 [9] [9]]\n",
            "at {level}"
        );
    }
}
