//! JavaScript package libspecs: registry lowering, bundler ES modules, the REPL
//! and diagnostics. Contract rows live in tests/fixtures/contracts.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::{
    analyze_namespace_entry, compile_namespace_esm, compile_namespace_with_warnings,
};
use light_cljs::{
    ApplicationOptions, EvalOptions, EvalSession, MacroHost, OptimizationLevel, PACKAGE_REGISTRY,
    compile_namespace, prepare_application,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const ROOT: &str = "tests/fixtures/compiler-namespaces/npm-packages";

fn manifest(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn node() -> Command {
    Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
}

fn run_node(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A temporary project directory, removed when the test ends.
struct Project(TempDir);
impl Project {
    fn new() -> Self {
        Self(TempDir::new("cljs-npm-packages"))
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}
/// Just enough JSON for the contract rows.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}
impl Json {
    fn parse(text: &str) -> Json {
        let mut chars = text.chars().peekable();
        let value = Self::value(&mut chars);
        assert!(chars.all(char::is_whitespace), "trailing JSON: {text}");
        value
    }
    fn value(chars: &mut std::iter::Peekable<std::str::Chars>) -> Json {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        match chars.next().expect("JSON value") {
            'n' => Self::literal(chars, "ull", Json::Null),
            't' => Self::literal(chars, "rue", Json::Bool(true)),
            'f' => Self::literal(chars, "alse", Json::Bool(false)),
            '"' => Json::String(Self::string(chars)),
            '[' => {
                let mut items = Vec::new();
                loop {
                    while chars.next_if(|c| c.is_whitespace()).is_some() {}
                    if chars.next_if_eq(&']').is_some() {
                        break Json::Array(items);
                    }
                    chars.next_if_eq(&',');
                    items.push(Self::value(chars));
                }
            }
            '{' => {
                let mut entries = Vec::new();
                loop {
                    while chars.next_if(|c| c.is_whitespace() || *c == ',').is_some() {}
                    if chars.next_if_eq(&'}').is_some() {
                        break Json::Object(entries);
                    }
                    assert_eq!(chars.next(), Some('"'));
                    let key = Self::string(chars);
                    while chars.next_if(|c| c.is_whitespace()).is_some() {}
                    assert_eq!(chars.next(), Some(':'));
                    entries.push((key, Self::value(chars)));
                }
            }
            first => {
                let mut number = String::from(first);
                while let Some(c) = chars.next_if(|c| c.is_ascii_digit() || "+-.eE".contains(*c)) {
                    number.push(c);
                }
                Json::Number(number.parse().unwrap())
            }
        }
    }
    fn literal(chars: &mut std::iter::Peekable<std::str::Chars>, rest: &str, value: Json) -> Json {
        assert!(rest.chars().all(|c| chars.next() == Some(c)));
        value
    }
    fn string(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut result = String::new();
        let mut units = Vec::new();
        loop {
            match chars.next().expect("unterminated JSON string") {
                '"' => break,
                '\\' => match chars.next().unwrap() {
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        units.push(u16::from_str_radix(&hex, 16).unwrap());
                        if !chars.clone().take(2).eq("\\u".chars()) {
                            result.push_str(&String::from_utf16(&units).unwrap());
                            units.clear();
                        }
                        continue;
                    }
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    'r' => result.push('\r'),
                    'b' => result.push('\u{8}'),
                    'f' => result.push('\u{c}'),
                    other => result.push(other),
                },
                c => result.push(c),
            }
        }
        result
    }
    fn get(&self, key: &str) -> &Json {
        let Json::Object(entries) = self else {
            panic!("expected object for {key}")
        };
        &entries
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .1
    }
    fn str(&self) -> &str {
        let Json::String(text) = self else {
            panic!("expected string: {self:?}")
        };
        text
    }
    fn items(&self) -> &[Json] {
        let Json::Array(items) = self else {
            panic!("expected array: {self:?}")
        };
        items
    }
}

fn contract_rows(text: &str, prefix: &str) -> Vec<Json> {
    text.lines()
        .filter(|line| line.contains(&format!("\"id\":\"{prefix}")))
        .map(Json::parse)
        .collect()
}

#[test]
fn package_contract_cases_execute_against_a_host_registry() {
    let mut rows = contract_rows(
        include_str!("../fixtures/contracts/namespace-cases.jsonl"),
        "light-npm-package-",
    );
    assert_eq!(
        rows.len(),
        5,
        "alias, refer/rename, default, names, missing"
    );
    // The :require-global fixture shares the same js_refers bookkeeping.
    rows.extend(contract_rows(
        include_str!("../fixtures/contracts/namespace-cases.jsonl"),
        "cljs-namespace-completion-require-global",
    ));
    for row in rows {
        let id = row.get("id").str();
        let root = manifest(row.get("source_root").str());
        let entry = row.get("entry").str();
        let file = root.join(format!(
            "{}.cljs",
            entry.replace('.', "/").replace('-', "_")
        ));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            row.get("source").str(),
            "{id}: row source must match {}",
            file.display()
        );
        let js = compile_namespace(entry, &[root]).unwrap_or_else(|e| panic!("{id}: {e}"));
        let export = row.get("export").str();
        assert_eq!(export, "run");
        let stdout = run_node(node().args([
            "-e",
            &format!(
                "const run = {js};\nconst trace = [];\nconst value = run((x) => trace.push(x));\nconsole.log(JSON.stringify({{outcome: {{status: \"returned\", value}}, trace}}));"
            ),
        ]));
        let (program_output, result) = stdout
            .trim_end()
            .rsplit_once('\n')
            .unwrap_or(("", stdout.trim_end()));
        let result = Json::parse(result);
        let expected = row.get("expected");
        assert_eq!(result.get("outcome"), expected.get("outcome"), "{id}");
        assert_eq!(result.get("trace"), expected.get("trace"), "{id}");
        assert_eq!(program_output, expected.get("stdout").str(), "{id}");
    }
}

#[test]
fn unbundled_output_reads_the_registry_at_each_use() {
    let root = manifest(ROOT);
    let js = compile_namespace("npm.alias", std::slice::from_ref(&root)).unwrap();
    assert!(
        js.starts_with("(() =>"),
        "plain output stays one expression"
    );
    assert!(!js.contains("export default"));
    assert!(js.contains(&format!("globalThis.{PACKAGE_REGISTRY};")));
    assert!(js.contains("pkg(\"react\")[\"createElement\"]"));
    assert!(js.contains("pkg(\"react\")[\"default\"][\"version\"]"));
    // The CLI writes the same bare expression without --esm.
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "npm.alias", "--source-root"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), js);
}

fn write_stub_packages(dir: &Project) {
    let package = |name: &str, extra: &str| {
        format!("{{\"name\": \"{name}\", \"type\": \"module\", \"main\": \"index.js\"{extra}}}")
    };
    dir.write("node_modules/react/package.json", &package("react", ""));
    dir.write(
        "node_modules/react/index.js",
        "export function createElement(tag) { return \"el:\" + tag; }\n\
         export function useState(value) { return \"state:\" + value; }\n\
         export function unusedExport() { return \"UNUSED_EXPORT_BODY\"; }\n",
    );
    dir.write(
        "node_modules/@scope/pkg/package.json",
        &package("@scope/pkg", ""),
    );
    dir.write(
        "node_modules/@scope/pkg/index.js",
        "export default function scoped(x) { return \"scoped:\" + x; }\n",
    );
    dir.write(
        "node_modules/lodash.debounce/package.json",
        &package("lodash.debounce", ""),
    );
    dir.write(
        "node_modules/lodash.debounce/index.js",
        "export const label = \"debounce-label\";\n",
    );
    dir.write(
        "node_modules/some-package/package.json",
        &package(
            "some-package",
            ", \"exports\": {\".\": \"./index.js\", \"./sub\": \"./sub.js\"}",
        ),
    );
    dir.write("node_modules/some-package/index.js", "export {};\n");
    dir.write(
        "node_modules/some-package/sub.js",
        "export function helper(x) { return 2 * x; }\n",
    );
}

#[test]
fn bundler_module_imports_only_surviving_package_references() {
    let root = manifest(ROOT);
    let module = compile_namespace_esm("npm.bundle", std::slice::from_ref(&root)).unwrap();
    let (imports, program) = module.split_once("export default ").unwrap();
    assert_eq!(
        imports,
        include_str!("../fixtures/compiler-namespaces/npm-packages/npm/bundle.imports.js"),
        "named imports, namespace fallback, default export and sanitized names"
    );
    assert!(program.starts_with("(() =>") && program.ends_with("})();\n"));
    for absent in [
        PACKAGE_REGISTRY,
        "pkg(",
        "host(\"react",
        "unusedExport",
        "only-in-removed-code",
        "anything",
    ] {
        assert!(
            !module.contains(absent),
            "unexpected {absent} in bundler output"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args([
            "--compile",
            "--esm",
            "--entry",
            "npm.bundle",
            "--source-root",
        ])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), module);

    // Module ordering loads every package before the program runs.
    let dir = Project::new();
    write_stub_packages(&dir);
    dir.write("bundle.mjs", &module);
    dir.write(
        "main.mjs",
        "import value from \"./bundle.mjs\";\nconsole.log(value);\n",
    );
    assert_eq!(
        run_node(node().arg(dir.0.join("main.mjs"))),
        "el:div state:1 scoped:2 debounce-label 42 false\n"
    );

    // Optional: a real bundler drops the unused export of a two-export package.
    let esbuild = std::env::var_os("ESBUILD").unwrap_or_else(|| "esbuild".into());
    let Ok(probe) = Command::new(&esbuild).arg("--version").output() else {
        eprintln!("esbuild unavailable; skipping export tree-shaking check");
        return;
    };
    if !probe.status.success() {
        eprintln!("esbuild unavailable; skipping export tree-shaking check");
        return;
    }
    let bundled = dir.0.join("out.js");
    let status = Command::new(&esbuild)
        .arg(dir.0.join("main.mjs"))
        .args(["--bundle", "--format=esm", "--log-level=warning"])
        .arg(format!("--outfile={}", bundled.display()))
        .current_dir(&dir.0)
        .status()
        .unwrap();
    assert!(status.success());
    let bundled = fs::read_to_string(bundled).unwrap();
    assert!(bundled.contains("\"el:\""));
    assert!(!bundled.contains("UNUSED_EXPORT_BODY"));
}

#[test]
fn bundler_modules_run_at_every_optimization_level() {
    let root = manifest(ROOT);
    let dir = Project::new();
    write_stub_packages(&dir);
    dir.write(
        "node_modules/only-in-removed-code/package.json",
        "{\"name\": \"only-in-removed-code\", \"type\": \"module\", \"main\": \"index.js\"}",
    );
    dir.write(
        "node_modules/only-in-removed-code/index.js",
        "export function anything() { return 0; }\n",
    );
    for level in OptimizationLevel::ALL {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--compile", "--esm", "--optimize", level.name()])
            .args(["--entry", "npm.bundle", "--source-root"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{level}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let module = String::from_utf8(output.stdout).unwrap();
        let program = analyze_namespace_entry(
            "npm.bundle",
            std::slice::from_ref(&root),
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(
            module,
            prepare_application(&program, ApplicationOptions::new(level)).emit_esm(),
            "{level}: the CLI emits the library module"
        );
        // Only `none` keeps the unused definition and so imports its packages.
        assert_eq!(
            module.contains("from \"only-in-removed-code\""),
            level == OptimizationLevel::None,
            "{level}"
        );
        let name = format!("bundle-{level}.mjs");
        dir.write(&name, &module);
        dir.write(
            "main.mjs",
            &format!("import value from \"./{name}\";\nconsole.log(value);\n"),
        );
        assert_eq!(
            run_node(node().arg(dir.0.join("main.mjs"))),
            "el:div state:1 scoped:2 debounce-label 42 false\n",
            "{level}"
        );
    }
}

#[test]
fn bundler_import_bindings_are_distinct_identifiers_for_similar_package_names() {
    let dir = Project::new();
    dir.write(
        "main.cljs",
        r#"(ns main (:require ["a-b" :refer [x] :rename {x one}] ["a_b" :refer [x] :rename {x two}]
                             ["a.b" :refer [x] :rename {x three}] ["a/b" :as four]
                             ["@a/b" :default five] ["ab$" :refer [x]]))
           [(one) (two) (three) (four/x) (five) (x)]"#,
    );
    let module = compile_namespace_esm("main", std::slice::from_ref(dir.0.path())).unwrap();
    let imports: Vec<_> = module
        .lines()
        .take_while(|l| l.starts_with("import"))
        .collect();
    assert_eq!(
        imports,
        [
            "import { x as $a_2db$x } from \"a-b\";",
            "import { x as $a_5fb$x } from \"a_b\";",
            "import { x as $a_2eb$x } from \"a.b\";",
            "import { x as $a_2fb$x } from \"a/b\";",
            "import { default as $_40a_2fb$default } from \"@a/b\";",
            "import { x as $ab_24$x } from \"ab$\";",
        ]
    );
}

#[test]
fn string_libspecs_without_bindings_warn_instead_of_failing() {
    let dir = Project::new();
    dir.write(
        "main.cljs",
        "(ns main (:require \"left-pad\" [\"@scope/side-effect\"]))\n(+ 1 2)",
    );
    let mut warnings = Vec::new();
    let js = compile_namespace_with_warnings(
        "main",
        std::slice::from_ref(dir.0.path()),
        AnalysisOptions::default(),
        &mut warnings,
    )
    .unwrap();
    assert!(!js.contains("pkg("));
    let kinds: Vec<_> = warnings.iter().map(|w| (w.kind, w.span.clone())).collect();
    assert_eq!(
        kinds,
        [
            ("package-require-no-effect", 19..29),
            ("package-require-no-effect", 31..51)
        ]
    );
    // Neither package is imported by bundler output either.
    let module = compile_namespace_esm("main", std::slice::from_ref(dir.0.path())).unwrap();
    assert!(module.starts_with("export default "));
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args([
            "--warnings",
            "--compile",
            "--entry",
            "main",
            "--source-root",
        ])
        .arg(dir.0.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("requiring JavaScript package \"left-pad\" without :as, :refer or :default has no effect; the host loads packages [package-require-no-effect]")
    );
}

#[test]
fn package_as_alias_is_reader_only() {
    let dir = Project::new();
    dir.write(
        "main.cljs",
        "(ns main (:require [\"@scope/pkg\" :as-alias p]))\n(str ::p/x)",
    );
    let mut warnings = Vec::new();
    let js = compile_namespace_with_warnings(
        "main",
        std::slice::from_ref(dir.0.path()),
        AnalysisOptions::default(),
        &mut warnings,
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!js.contains("pkg("));
    assert_eq!(run_node(node().args(["-p", &js])), ":@scope/pkg/x\n");
    let module = compile_namespace_esm("main", std::slice::from_ref(dir.0.path())).unwrap();
    assert!(module.starts_with("export default "));
}

#[test]
fn package_diagnostics_match_contract_probes() {
    let rows = contract_rows(
        include_str!("../fixtures/contracts/diagnostic-cases.jsonl"),
        "light-diagnostic-npm-package-specs",
    );
    let probes = rows[0].get("diagnostic").get("probes").items();
    assert!(probes.len() >= 10);
    for probe in probes {
        let id = probe.get("id").str();
        assert_eq!(probe.get("mode").str(), "ns");
        let entry = probe.get("entry").str();
        let dir = Project::new();
        dir.write(
            &format!("{}.cljs", entry.replace('.', "/")),
            probe.get("source").str(),
        );
        let local = probe.get("local");
        let detail = local.get("detail").str();
        let mut warnings = Vec::new();
        let result = compile_namespace_with_warnings(
            entry,
            std::slice::from_ref(dir.0.path()),
            AnalysisOptions::default(),
            &mut warnings,
        );
        match local.get("category").str() {
            "namespace-error" => {
                let error = result.expect_err(id);
                assert_eq!(error.message, detail, "{id}");
                assert!(error.path.unwrap().ends_with("bar.cljs"), "{id}");
            }
            "warning" => {
                result.unwrap_or_else(|e| panic!("{id}: {e}"));
                assert_eq!(warnings.len(), 1, "{id}: {warnings:?}");
                assert_eq!(warnings[0].kind, local.get("kind").str(), "{id}");
                assert_eq!(warnings[0].message, detail, "{id}");
            }
            other => panic!("{id}: unknown category {other}"),
        }
    }
}

fn printed(s: &mut EvalSession, source: &str) -> String {
    let value = s.eval_string(source).unwrap().value;
    let text = s.print(value).unwrap();
    s.release(value).unwrap();
    text
}

const PRELUDE: &str = r#"(set! js/lightCljsPackages
  (js-obj "react" (js-obj "createElement" (fn [tag] (str "el:" tag)) "default" :react-default)
          "@scope/pkg" (js-obj "default" (fn [x] (str "scoped:" x)))))"#;

#[test]
fn repl_requires_bind_packages_and_resolve_them_at_each_use() {
    let mut s = EvalSession::new(EvalOptions::default()).unwrap();
    printed(&mut s, PRELUDE);
    assert_eq!(
        printed(
            &mut s,
            r#"(require '["react" :as r :refer [createElement] :rename {createElement ce} :default React])"#
        ),
        "nil"
    );
    assert_eq!(printed(&mut s, "(r/createElement \"a\")"), "\"el:a\"");
    assert_eq!(printed(&mut s, "(ce \"b\")"), "\"el:b\"");
    assert_eq!(printed(&mut s, "React"), ":react-default");
    // Use sites read the registry, so a host replacement is visible to
    // functions compiled earlier.
    printed(&mut s, "(defn f [] (r/createElement \"c\"))");
    printed(
        &mut s,
        r#"(set! js/lightCljsPackages (js-obj "react" (js-obj "createElement" (fn [tag] (str "new:" tag)))))"#,
    );
    assert_eq!(printed(&mut s, "(f)"), "\"new:c\"");
    printed(&mut s, PRELUDE);
    assert_eq!(
        printed(
            &mut s,
            r#"(ns app.core (:require ["@scope/pkg" :default scoped])) (scoped 1)"#
        ),
        "\"scoped:1\""
    );
    assert_eq!(
        printed(
            &mut s,
            r#"(ns* (:require '["react" :as rx])) (rx/createElement "d")"#
        ),
        "\"el:d\""
    );
    printed(&mut s, r#"(require '["left-pad" :as left-pad])"#);
    let error = s.eval_string("(left-pad/default \"x\")").unwrap_err();
    assert!(
        error.message.contains(
            "JavaScript package \"left-pad\" is not provided by globalThis.lightCljsPackages"
        ),
        "{error}"
    );
    // A later interactive binding of the same name wins, as for namespace aliases.
    printed(&mut s, r#"(require '["@scope/pkg" :as r])"#);
    assert_eq!(printed(&mut s, "(r/default 2)"), "\"scoped:2\"");
    printed(&mut s, "(require '[clojure.string :as r])");
    assert_eq!(printed(&mut s, "(r/upper-case \"x\")"), "\"X\"");
}

#[test]
fn name_policy_checks_package_lookups_as_the_registry_global() {
    use light_cljs::compiler_data::Name;
    let mut s = EvalSession::new(EvalOptions {
        policy: light_cljs::eval_session::NamePolicy {
            deny: [Name::new(Some("js"), PACKAGE_REGISTRY)].into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    printed(
        &mut s,
        r#"(require '["react" :as r :refer [createElement]])"#,
    );
    for source in ["(r/createElement \"a\")", "(createElement \"a\")", "r"] {
        let error = s.eval_string(source).unwrap_err();
        assert!(
            error
                .message
                .contains("name policy denies js/lightCljsPackages"),
            "{source}: {error}"
        );
    }
    // Other host globals keep their full-path policy names.
    assert_eq!(printed(&mut s, "(js/Math.abs -2)"), "2");
}
