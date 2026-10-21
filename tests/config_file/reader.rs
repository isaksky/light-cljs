//! Reading and validating one file: the error table of
//! docs/build-configuration.md "Errors" (the rows a file alone can produce),
//! a wrong-typed value for every key of the schema's tables, `#light-cljs/env`
//! in each of its forms, discovery, and the document's two example files.

use std::{collections::HashMap, fs, path::Path};

use light_cljs::config_file::{
    BuildId, BuildMode, ConfigFile, ConfigFileError, FILE_NAME, Setting, Value, ValueKind,
};

use crate::temp_dir::TempDir;

fn read(source: &str, environment: &[(&str, &str)]) -> Result<ConfigFile, ConfigFileError> {
    let environment: HashMap<String, String> = environment
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    ConfigFile::from_source(Path::new(FILE_NAME), source.to_owned(), &|name| {
        environment.get(name).cloned()
    })
}

fn message(source: &str, environment: &[(&str, &str)]) -> String {
    match read(source, environment) {
        Ok(_) => "no error".into(),
        Err(error) => error.to_string(),
    }
}

/// A file whose one build `:app` is valid, with `extra` at 7:4.
fn app(extra: &str) -> String {
    format!(
        "{{:source-paths [\"src\"]
 :builds
 {{:app
  {{:target :esm
   :output-dir \"out\"
   :modules {{:main {{:entries [app.main]}}}}
   {extra}}}}}}}"
    )
}

/// A file whose build `:app` has `modules` as its `:modules`, at 6:13.
fn app_modules(modules: &str) -> String {
    format!(
        "{{:source-paths [\"src\"]
 :builds
 {{:app
  {{:target :esm
   :output-dir \"out\"
   :modules {modules}}}}}}}"
    )
}

/// A file whose build `:app` is `{body}`, the body at 4:4.
fn app_body(body: &str) -> String {
    format!(
        "{{:source-paths [\"src\"]
 :builds
 {{:app
  {{{body}}}}}}}"
    )
}

/// A valid file with the top-level `extra` at 2:2.
fn top(extra: &str) -> String {
    format!(
        "{{:source-paths [\"src\"]
 {extra}
 :builds {{:app {{:target :esm :output-dir \"out\" :modules {{:main {{:entries [app.main]}}}}}}}}}}"
    )
}

/// A top-level `:http {:port FORM}`, FORM at 2:15.
fn port(form: &str) -> String {
    top(&format!(":http {{:port {form}}}"))
}

/// A `:compiler-options {OPTION}` of build `:app`, OPTION's key at 7:23.
fn option(option: &str) -> String {
    app(&format!(":compiler-options {{{option}}}"))
}

const CODE_SPLITTING: &str = "chunks are inferred from light.lazy/loadable sites \
                              (docs/code-splitting.md), and named chunks arrive with its Phase 4";
const MALFORMED: &str = "light-cljs.edn:2:15: #light-cljs/env takes \"NAME\", [\"NAME\"], \
                         [\"NAME\" DEFAULT] or [\"NAME\" :as TYPE :default DEFAULT]";

/// Every case of "Errors" that a file alone can produce, with its exact
/// message, line and column.
fn error_rows() -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut row = |source: String, expected: &str| rows.push((source, expected.to_owned()));

    // Unknown keys, with "did you mean" against the enclosing map's keys.
    row(
        app(":output-path \"x\""),
        "light-cljs.edn:7:4: unknown key :output-path in build :app; did you mean :output-dir?",
    );
    row(
        app(":output_dir \"x\""),
        "light-cljs.edn:7:4: unknown key :output_dir in build :app; did you mean :output-dir?",
    );
    row(
        app(":outputDir \"x\""),
        "light-cljs.edn:7:4: unknown key :outputDir in build :app; did you mean :output-dir?",
    );
    row(
        app(":zzz 1"),
        "light-cljs.edn:7:4: unknown key :zzz in build :app; the keys here are :build-options, \
         :closure-defines, :compiler-options, :config, :dev, :devtools, :modules, :output-dir, \
         :release, :target",
    );
    row(
        top(":source-path [\"x\"]"),
        "light-cljs.edn:2:2: unknown key :source-path at the top level; did you mean :source-paths?",
    );
    row(
        option(":optimisations :none"),
        "light-cljs.edn:7:23: unknown key :optimisations in :compiler-options of build :app; \
         did you mean :optimizations?",
    );
    row(
        app(":dev {:output-path \"x\"}"),
        "light-cljs.edn:7:10: unknown key :output-path in build :app (dev); did you mean \
         :output-dir?",
    );
    row(
        top(":http {:prot 1}"),
        "light-cljs.edn:2:9: unknown key :prot in :http; did you mean :port?",
    );
    row(
        app_modules("{:main {:entry [app.main]}}"),
        "light-cljs.edn:6:21: unknown key :entry in module :main of build :app; did you mean \
         :entries?",
    );

    // Shadow-cljs keys with no equivalent: the reason, and what to use.
    row(
        top(":dependencies [[reagent \"1.2.0\"]]"),
        "light-cljs.edn:2:2: :dependencies: light-cljs resolves no dependencies; put library \
         sources in :source-paths, and let the host or a bundler supply JavaScript packages",
    );
    row(
        top(":deps true"),
        "light-cljs.edn:2:2: :deps: light-cljs reads no deps.edn and resolves no dependencies; \
         put library sources in :source-paths, and let the host or a bundler supply JavaScript \
         packages",
    );
    row(
        top(":dev-http {8080 \"public\"}"),
        "light-cljs.edn:2:2: :dev-http: the :http dev server serves each build's :output-dir; \
         use :http {:port N}",
    );
    row(
        top(":jvm-opts [\"-Xmx2G\"]"),
        "light-cljs.edn:2:2: :jvm-opts: light-cljs runs no JVM",
    );
    row(
        app(":asset-path \"/js\""),
        "light-cljs.edn:7:4: :asset-path in build :app: chunks import one another by relative \
         URL, and the :http dev server serves each build's :output-dir; delete it",
    );
    row(
        app(":js-options {}"),
        "light-cljs.edn:7:4: :js-options in build :app: light-cljs resolves no JavaScript \
         packages; let the host or a bundler supply them",
    );
    row(
        app(":build-hooks []"),
        "light-cljs.edn:7:4: :build-hooks in build :app: no user code runs outside macros, \
         whose inputs the cache contract governs; reload hooks are ^:dev/before-load and \
         ^:dev/after-load metadata",
    );
    row(
        app(":init-fn app.main/init"),
        "light-cljs.edn:7:4: :init-fn in build :app: the entry's value is the startup chunk's \
         default export, and the entry namespace runs when it loads; call the function from \
         there",
    );
    row(
        option(":infer-externs :auto"),
        "light-cljs.edn:7:23: :infer-externs in :compiler-options of build :app: light-cljs \
         runs no Closure Compiler, so there are no externs to infer",
    );
    row(
        option(":externs [\"x.js\"]"),
        "light-cljs.edn:7:23: :externs in :compiler-options of build :app: light-cljs runs no \
         Closure Compiler, so there are no externs",
    );
    row(
        option(":output-feature-set :es2020"),
        "light-cljs.edn:7:23: :output-feature-set in :compiler-options of build :app: \
         light-cljs writes one kind of output, ES modules, so there is no feature set to choose",
    );
    row(
        option(":output-wrapper true"),
        "light-cljs.edn:7:23: :output-wrapper in :compiler-options of build :app: ES modules \
         have their own scope, so there is no wrapper",
    );
    row(
        option(":source-map true"),
        "light-cljs.edn:7:23: :source-map in :compiler-options of build :app: chunked output \
         has no source maps yet",
    );
    row(
        option(":warnings {:fn-deprecated false}"),
        "light-cljs.edn:7:23: :warnings in :compiler-options of build :app: light-cljs has no \
         per-warning switches; :cljs-warnings true makes undeclared Vars and conflicting \
         arities warnings",
    );
    row(
        option(":warnings-as-errors true"),
        "light-cljs.edn:7:23: :warnings-as-errors in :compiler-options of build :app: \
         undeclared Vars and conflicting arities are errors unless :cljs-warnings is true, and \
         there are no other warnings to promote",
    );
    row(
        option(":fn-invoke-direct true"),
        "light-cljs.edn:7:23: :fn-invoke-direct in :compiler-options of build :app: \
         light-cljs's optimizer chooses direct calls itself; use :optimizations",
    );
    row(
        app(":devtools {:after-load app.main/reload}"),
        "light-cljs.edn:7:15: :after-load in :devtools of build :app: mark the function \
         ^:dev/after-load instead, metadata shadow-cljs also reads",
    );
    row(
        app(":devtools {:before-load app.main/stop}"),
        "light-cljs.edn:7:15: :before-load in :devtools of build :app: mark the function \
         ^:dev/before-load instead, metadata shadow-cljs also reads",
    );

    // :target, :modules and its one module.
    row(
        app_body(
            ":target :browser
   :output-dir \"out\"
   :modules {:main {:entries [app.main]}}",
        ),
        "light-cljs.edn:4:12: :target of build :app must be :esm: light-cljs writes ES \
         modules, which run in browsers, Node and bundlers",
    );
    row(
        app_modules("{:main {:entries [app.main]} :extra {:entries [app.extra]}}"),
        &format!("light-cljs.edn:6:42: build :app has two modules; {CODE_SPLITTING}"),
    );
    row(
        app_modules("{:main {:entries [app.main app.extra]}}"),
        &format!(
            "light-cljs.edn:6:40: module :main of build :app has two entries; give one \
             namespace: {CODE_SPLITTING}"
        ),
    );
    row(
        app_modules("{:main {:entries [app.main] :init-fn app.main/init}}"),
        "light-cljs.edn:6:41: :init-fn in module :main of build :app: the entry's value is the \
         startup chunk's default export, and the entry namespace runs when it loads; call the \
         function from there",
    );
    row(
        app_modules("{:main {:entries [app.main] :depends-on #{:shared}}}"),
        &format!(
            "light-cljs.edn:6:41: :depends-on in module :main of build :app: a build has one \
             module; {CODE_SPLITTING}"
        ),
    );
    row(
        app_modules("{:main {:entries [app.main] :exports {default app.main/x}}}"),
        "light-cljs.edn:6:41: :exports in module :main of build :app: the startup chunk's \
         default export is the entry's value; there are no named exports",
    );
    row(
        app_modules("{:main {:entries [app.main] :prepend \"x\"}}"),
        "light-cljs.edn:6:41: :prepend in module :main of build :app: light-cljs writes the \
         startup chunk as compiled; put other scripts in the page",
    );
    row(
        app_modules("{:main {}}"),
        "light-cljs.edn:6:20: module :main of build :app has no :entries; give its namespace, \
         such as {:entries [app.main]}",
    );
    row(
        app_modules("{:main {:entries app.main}}"),
        "light-cljs.edn:6:30: :entries of module :main of build :app must be a vector of one \
         namespace symbol, such as [app.main], got app.main",
    );

    // A key in the wrong map says where it belongs.
    row(
        app(":http {:port 9000}"),
        "light-cljs.edn:7:4: :http in build :app is a top-level key, as in shadow-cljs",
    );
    row(
        app(":cache-retention 4"),
        "light-cljs.edn:7:4: :cache-retention in build :app is a top-level key, as in \
         shadow-cljs",
    );
    row(
        app(":optimizations :advanced"),
        "light-cljs.edn:7:4: :optimizations in build :app belongs under :compiler-options, as \
         in shadow-cljs",
    );
    row(
        app(":cache-level :off"),
        "light-cljs.edn:7:4: :cache-level in build :app belongs under :build-options, as in \
         shadow-cljs",
    );
    row(
        app(":entries [app.main]"),
        "light-cljs.edn:7:4: :entries in build :app belongs in a module under :modules, as in \
         shadow-cljs",
    );
    row(
        top(":output-dir \"out\""),
        "light-cljs.edn:2:2: :output-dir at the top level is a build key, as in shadow-cljs",
    );
    row(
        top(":optimizations :none"),
        "light-cljs.edn:2:2: :optimizations at the top level belongs under a build's \
         :compiler-options, as in shadow-cljs",
    );
    row(
        option(":output-dir \"x\""),
        "light-cljs.edn:7:23: :output-dir in :compiler-options of build :app is a build key, as \
         in shadow-cljs",
    );

    // :closure-defines and :checked-arrays never reach dev mode.
    row(
        app(":closure-defines {app.config/DEBUG true}"),
        "light-cljs.edn:7:4: :closure-defines of build :app applies in dev mode, whose dev \
         build takes no defines; write it under :release",
    );
    row(
        option(":closure-defines {app.config/DEBUG true}"),
        "light-cljs.edn:7:23: :closure-defines of build :app applies in dev mode, whose dev \
         build takes no defines; write it under :release",
    );
    row(
        app(":dev {:closure-defines {app.config/DEBUG true}}"),
        "light-cljs.edn:7:10: :closure-defines of build :app (dev) applies in dev mode, whose \
         dev build takes no defines; write it under :release",
    );
    // :checked-arrays is a compiler option, as in shadow-cljs; in the build's
    // own map it is misplaced before it can reach a mode.
    row(
        app(":checked-arrays :warn"),
        "light-cljs.edn:7:4: :checked-arrays in build :app belongs under :compiler-options, as \
         in shadow-cljs",
    );
    // An empty map reaches dev mode too.
    row(
        app(":closure-defines {}"),
        "light-cljs.edn:7:4: :closure-defines of build :app applies in dev mode, whose dev \
         build takes no defines; write it under :release",
    );
    row(
        option(":closure-defines {}"),
        "light-cljs.edn:7:23: :closure-defines of build :app applies in dev mode, whose dev \
         build takes no defines; write it under :release",
    );
    row(
        app(":dev {:closure-defines {}}"),
        "light-cljs.edn:7:10: :closure-defines of build :app (dev) applies in dev mode, whose \
         dev build takes no defines; write it under :release",
    );
    row(
        option(":checked-arrays :warn"),
        "light-cljs.edn:7:23: :checked-arrays of build :app applies in dev mode, whose dev \
         build checks no array accesses; write it under :release",
    );
    row(
        app(":dev {:compiler-options {:checked-arrays :warn}}"),
        "light-cljs.edn:7:29: :checked-arrays of build :app (dev) applies in dev mode, whose \
         dev build checks no array accesses; write it under :release",
    );

    // Bad values.
    row(
        option(":optimizations :simple"),
        "light-cljs.edn:7:38: :optimizations of build :app must be :none, :prepare or \
         :advanced, got :simple",
    );
    row(
        app(":config {:app/n 1N}"),
        "light-cljs.edn:7:20: :config :app/n of build :app: \"1N\" is not a configuration \
         number: write a decimal integer or floating-point number, such as 3, -2.5 or 1e3",
    );
    row(
        app(":config {:app/s foo}"),
        "light-cljs.edn:7:20: :config :app/s of build :app: \"foo\" is not a configuration \
         value: use keywords, numbers, strings, booleans, nil and lists, vectors, maps and \
         sets of them",
    );
    row(
        app(":build-options {:cache-level :jars}"),
        "light-cljs.edn:7:33: :cache-level of build :app must be :all or :off; :jars caches \
         only the namespaces of jars, and light-cljs reads no jars",
    );

    // Tags and reader conditionals.
    row(
        port("#shadow/env \"PORT\""),
        "light-cljs.edn:2:15: #shadow/env: write #light-cljs/env, which takes the same forms",
    );
    row(
        port("#env \"PORT\""),
        "light-cljs.edn:2:15: #env: write #light-cljs/env, which takes the same forms",
    );
    row(
        port("#inst \"2026-10-20\""),
        "light-cljs.edn:2:15: #inst: the configuration file reads no tagged literal but \
         #light-cljs/env",
    );
    row(
        top(":http #?(:clj {:port 1})"),
        "light-cljs.edn:2:8: #?: the configuration file is EDN, which has no reader conditionals",
    );

    // Each malformed #light-cljs/env.
    for form in [
        "PORT",
        "[]",
        "[PORT]",
        "[\"\"]",
        "{\"PORT\" 1}",
        "[\"PORT\" \"a\" \"b\"]",
        "[\"PORT\" :as :int :default]",
        "[\"PORT\" :as :int :as :int]",
        "[\"PORT\" :colour :int]",
    ] {
        row(port(&format!("#light-cljs/env {form}")), MALFORMED);
    }
    // A nil default is a nil in the file, which no port is.
    for form in ["[\"PORT\" :default nil]", "[\"PORT\" nil]"] {
        row(
            port(&format!("#light-cljs/env {form}")),
            "light-cljs.edn:2:15: :port of :http must be a port number, 0 to 65535, got nil",
        );
    }
    row(
        port("#light-cljs/env [\"PORT\" :as :float]"),
        "light-cljs.edn:2:43: #light-cljs/env :as takes :int, :bool, :symbol or :keyword, got \
         :float",
    );

    // The reader's own errors, at their spans.
    row(
        app(":output-dir \"again\""),
        "light-cljs.edn:7:4: duplicate key :output-dir, first given at 5:4",
    );
    row(
        top(":cache-blockers #{a.b a.b}"),
        "light-cljs.edn:2:24: duplicate element a.b, first given at 2:20",
    );
    row(
        app("#_#_ :output-dir \"old\""),
        "light-cljs.edn:7:6: #_ discards a second :output-dir, beside the key at 5:4; delete \
         one of them",
    );
    row(
        app(":release {:closure-defines {app.config/DEBUG true #_#_ app.config/DEBUG false}}"),
        "light-cljs.edn:7:56: #_ discards a second app.config/DEBUG, beside the key at 7:32; \
         delete one of them",
    );
    row(
        app(":config {:app/v {\"k\" 1 #_#_ \"k\" 2}}"),
        "light-cljs.edn:7:29: #_ discards a second \"k\", beside the key at 7:21; delete one of \
         them",
    );
    // Keys and elements are equal as the reader finds them, as --config does.
    row(
        app(":config {:app/v {1 :a 1.0 :b}}"),
        "light-cljs.edn:7:26: duplicate key 1.0, first given at 7:21",
    );
    row(
        app(":config {:app/v #{[1 2] (1 2)}}"),
        "light-cljs.edn:7:28: duplicate element (1 2), first given at 7:22",
    );
    row(
        top(":http {:port 1]"),
        "light-cljs.edn:2:16: expected }, found ]",
    );
    row(
        top(":http {:port}"),
        "light-cljs.edn:2:8: map requires an even number of forms",
    );

    // Mode maps hold build keys, never another mode map.
    row(
        app(":dev {:dev {}}"),
        "light-cljs.edn:7:10: :dev in build :app (dev): a mode map holds build keys; :dev and \
         :release sit in the build itself",
    );
    row(
        app(":release {:dev {}}"),
        "light-cljs.edn:7:14: :dev in build :app (release): a mode map holds build keys; :dev \
         and :release sit in the build itself",
    );

    // Build ids.
    row(
        "{:source-paths [\"src\"]
 :builds {:my_app {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}"
            .into(),
        "light-cljs.edn:2:11: build id :my_app must be a keyword of a letter followed by \
         letters, digits and hyphens, such as :app",
    );
    row(
        "{:source-paths [\"src\"]
 :builds {\"app\" {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}"
            .into(),
        "light-cljs.edn:2:11: build id \"app\" must be a keyword of a letter followed by \
         letters, digits and hyphens, such as :app",
    );
    rows
}

/// A wrong-typed value for each key of the top-level, build, compiler
/// options and nested tables.
fn type_rows() -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut row = |source: String, expected: &str| rows.push((source, expected.to_owned()));

    // Top-level keys.
    row(
        "{:source-paths \"src\"
 :builds {:app {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}"
            .into(),
        "light-cljs.edn:1:16: :source-paths must be a vector of directory strings, got \"src\"",
    );
    row(
        "{:source-paths [\"src\" 7]
 :builds {:app {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}"
            .into(),
        "light-cljs.edn:1:23: each of :source-paths must be a directory string, got 7",
    );
    row(
        "{:source-paths [\"src\"]\n :builds []}".into(),
        "light-cljs.edn:2:10: :builds must be a map of build id to build configuration, got []",
    );
    row(
        top(":cache-root 1"),
        "light-cljs.edn:2:14: :cache-root must be a directory string, got 1",
    );
    row(
        top(":cache-blockers [a.b]"),
        "light-cljs.edn:2:18: :cache-blockers must be a set of namespace symbols, got [a.b]",
    );
    row(
        top(":cache-blockers #{a/b}"),
        "light-cljs.edn:2:20: each of :cache-blockers must be a namespace symbol, got a/b",
    );
    row(
        top(":compile-threads 0"),
        "light-cljs.edn:2:19: :compile-threads must be a positive integer, got 0",
    );
    row(
        top(":cache-retention 0"),
        "light-cljs.edn:2:19: :cache-retention must be a positive integer, got 0",
    );
    row(
        top(":cache-retention \"3\""),
        "light-cljs.edn:2:19: :cache-retention must be a positive integer, got \"3\"",
    );
    row(
        top(":cache-retention -2"),
        "light-cljs.edn:2:19: :cache-retention must be a positive integer, got -2",
    );
    row(
        top(":cache-retention 2.5"),
        "light-cljs.edn:2:19: :cache-retention must be a positive integer, got 2.5",
    );
    row(
        top(":http 9630"),
        "light-cljs.edn:2:8: :http must be a map, got 9630",
    );
    row(
        port("\"x\""),
        "light-cljs.edn:2:15: :port of :http must be a port number, 0 to 65535, got \"x\"",
    );
    row(
        port("65536"),
        "light-cljs.edn:2:15: :port of :http must be a port number, 0 to 65535, got 65536",
    );
    row(
        top(":http {:host 1}"),
        "light-cljs.edn:2:15: :host of :http must be a host name string, got 1",
    );
    row(
        top(":http {:strict \"yes\"}"),
        "light-cljs.edn:2:17: :strict of :http must be true or false, got \"yes\"",
    );
    row(
        top(":nrepl true"),
        "light-cljs.edn:2:9: :nrepl must be false or a map such as {:port 0}, got true",
    );
    row(
        top(":nrepl {:port -1}"),
        "light-cljs.edn:2:16: :port of :nrepl must be a port number, 0 to 65535, got -1",
    );
    row(
        top(":fs-watch {:loop-wait 0}"),
        "light-cljs.edn:2:24: :loop-wait of :fs-watch must be a positive integer, got 0",
    );
    row(
        top(":fs-watch {:impl :hawk}"),
        "light-cljs.edn:2:19: :impl of :fs-watch must be :polling, the one watcher light-cljs \
         has, got :hawk",
    );

    // Build keys.
    row(
        app_body(
            ":target \"esm\"
   :output-dir \"out\"
   :modules {:main {:entries [app.main]}}",
        ),
        "light-cljs.edn:4:12: :target of build :app must be :esm: light-cljs writes ES \
         modules, which run in browsers, Node and bundlers",
    );
    row(
        app_body(
            ":target :esm
   :output-dir out
   :modules {:main {:entries [app.main]}}",
        ),
        "light-cljs.edn:5:16: :output-dir of build :app must be a directory string, got out",
    );
    row(
        app_modules("[app.main]"),
        "light-cljs.edn:6:13: :modules of build :app must be a map of one module, such as \
         {:main {:entries [app.main]}}, got [app.main]",
    );
    row(
        app_modules("{}"),
        "light-cljs.edn:6:13: :modules of build :app must be a map of one module, such as \
         {:main {:entries [app.main]}}, got {}",
    );
    row(
        app(":config [1]"),
        "light-cljs.edn:7:12: :config of build :app must be a map of keyword to EDN value, got [1]",
    );
    row(
        app(":compiler-options []"),
        "light-cljs.edn:7:22: :compiler-options of build :app must be a map, got []",
    );
    row(
        app(":closure-defines {DEBUG true}"),
        "light-cljs.edn:7:22: each key of :closure-defines of build :app must be a qualified \
         symbol such as app.config/DEBUG, got DEBUG",
    );
    row(
        app(":release {:closure-defines []}"),
        "light-cljs.edn:7:31: :closure-defines of build :app (release) must be a map of \
         qualified symbol to boolean, number or string, got []",
    );
    row(
        app(":release {:closure-defines {app.config/DEBUG :yes}}"),
        "light-cljs.edn:7:49: :closure-defines app.config/DEBUG of build :app (release) must be \
         a boolean, number or string, got :yes",
    );
    row(
        app(":build-options :off"),
        "light-cljs.edn:7:19: :build-options of build :app must be a map, got :off",
    );
    row(
        app(":build-options {:cache-level :none}"),
        "light-cljs.edn:7:33: :cache-level of build :app must be :all or :off, got :none",
    );
    row(
        app(":build-options {:cache-blockers #{\"a.b\"}}"),
        "light-cljs.edn:7:38: each of :cache-blockers of build :app must be a namespace symbol, \
         got \"a.b\"",
    );
    row(
        app(":devtools true"),
        "light-cljs.edn:7:14: :devtools of build :app must be a map, got true",
    );
    row(
        app(":devtools {:reload-strategy :partial}"),
        "light-cljs.edn:7:32: :reload-strategy of build :app must be :full, got :partial",
    );
    row(
        app(":dev []"),
        "light-cljs.edn:7:9: :dev of build :app must be a map of build keys, got []",
    );
    row(
        app(":release nil"),
        "light-cljs.edn:7:13: :release of build :app must be a map of build keys, got nil",
    );

    // Compiler options.
    for (key, value, column, expected) in [
        (
            "optimizations",
            "\"advanced\"",
            38,
            ":none, :prepare or :advanced",
        ),
        ("pseudo-names", "1", 37, "true or false"),
        ("elide-asserts", "\"true\"", 38, "true or false"),
        ("checked-arrays", "true", 39, ":warn or :error"),
        ("cljs-warnings", ":yes", 38, "true or false"),
        (
            "closure-defines",
            "true",
            40,
            "a map of qualified symbol to boolean, number or string",
        ),
        ("macro-expansions", "0", 41, "a positive integer"),
        ("macro-nodes", "-5", 36, "a positive integer"),
        ("macro-timeout-ms", "1.5", 41, "a positive integer"),
        ("macro-memory-limit", "\"64M\"", 43, "a positive integer"),
        ("macro-poll-budget", "1N", 42, "a positive integer"),
    ] {
        row(
            option(&format!(":{key} {value}")),
            &format!(
                "light-cljs.edn:7:{column}: :{key} of build :app must be {expected}, got {value}"
            ),
        );
    }
    rows
}

fn check_rows(rows: Vec<(String, String)>) {
    let mut failures = Vec::new();
    for (source, expected) in &rows {
        let actual = message(source, &[("HTTP_PORT", "web")]);
        if &actual != expected {
            failures.push(format!(
                "source:\n{source}\nexpected: {expected}\nactual:   {actual}\n"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} rows differ:\n\n{}",
        failures.len(),
        rows.len(),
        failures.join("\n")
    );
}

#[test]
fn each_error_a_file_alone_can_produce_is_located_exactly() {
    check_rows(error_rows());
}

#[test]
fn a_wrong_typed_value_for_every_key_is_located_exactly() {
    check_rows(type_rows());
}

#[test]
fn as_int_on_a_value_that_is_no_integer_is_an_error_at_the_tag() {
    let source = port("#light-cljs/env [\"HTTP_PORT\" :as :int :default 9630]");
    assert_eq!(
        message(&source, &[("HTTP_PORT", "web")]),
        "light-cljs.edn:2:15: HTTP_PORT is \"web\", not a decimal integer (:as :int)"
    );
    assert_eq!(
        message(&source, &[("HTTP_PORT", "80.5")]),
        "light-cljs.edn:2:15: HTTP_PORT is \"80.5\", not a decimal integer (:as :int)"
    );
    assert_eq!(message(&source, &[("HTTP_PORT", "8000")]), "no error");
}

#[test]
fn closure_defines_and_checked_arrays_are_accepted_under_release() {
    let file = read(
        &app(
            ":release {:closure-defines {app.config/DEBUG false app.config/N 3}
             :compiler-options {:checked-arrays :error
                                :closure-defines {app.config/VERSION \"1\"}}}",
        ),
        &[],
    )
    .unwrap();
    let build = file.build(BuildId(0));
    let release = Some(BuildMode::Release);
    assert_eq!(
        build.setting(release, Setting::CheckedArrays).unwrap().kind,
        ValueKind::Keyword("error".into())
    );
    assert!(build.setting(release, Setting::ClosureDefines).is_some());
    assert!(
        build
            .setting(release, Setting::CompilerClosureDefines)
            .is_some()
    );
    assert!(build.setting(None, Setting::ClosureDefines).is_none());
}

#[test]
fn unset_variables_inside_collections_are_kept_for_the_mode_that_runs() {
    let file = read(
        &app(
            ":release {:modules {:main {:entries [#light-cljs/env [\"RELEASE_ENTRY\" :as :symbol]]}}}",
        ),
        &[],
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let modules = file
        .build(BuildId(0))
        .setting(Some(BuildMode::Release), Setting::Modules)
        .unwrap();
    let ValueKind::Vector(entries) = &modules.get("main").unwrap().get("entries").unwrap().kind
    else {
        panic!("{modules:?}")
    };
    assert!(entries[0].is_unset_environment());

    let source = "{:source-paths [\"src\" #light-cljs/env \"EXTRA_SOURCES\"]
 :cache-blockers #{#light-cljs/env [\"BLOCKER\" :as :symbol]}
 :builds {:app {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}";
    let file = read(source, &[]).unwrap_or_else(|error| panic!("{error}"));
    let ValueKind::Vector(paths) = &file.setting(Setting::SourcePaths).unwrap().kind else {
        panic!()
    };
    assert!(paths[1].is_unset_environment());
    let ValueKind::Set(blockers) = &file.setting(Setting::CacheBlockers).unwrap().kind else {
        panic!()
    };
    assert!(blockers[0].is_unset_environment());
    // Set, the same values are checked as literals are.
    assert_eq!(
        message(source, &[("EXTRA_SOURCES", ""), ("BLOCKER", "app.x")]),
        "light-cljs.edn:1:23: each of :source-paths must be a directory string, got \"\""
    );
}

#[test]
fn a_map_given_by_an_unset_variable_keeps_its_origin_in_the_document() {
    let source = app(":release {:compiler-options #light-cljs/env \"RELEASE_OPTIONS\"}");
    let file = read(&source, &[]).unwrap_or_else(|error| panic!("{error}"));
    let options = file
        .document()
        .get("builds")
        .and_then(|builds| builds.get("app"))
        .and_then(|build| build.get("release"))
        .and_then(|release| release.get("compiler-options"))
        .unwrap();
    assert!(options.is_unset_environment());
    assert_eq!(
        options.environment.as_ref().unwrap().variable,
        "RELEASE_OPTIONS"
    );
    assert_eq!(
        &source[options.span.clone()],
        "#light-cljs/env \"RELEASE_OPTIONS\""
    );
}

#[test]
fn config_strings_keep_their_utf16_code_units_as_config_does() {
    let value = config_value("\"a\\uD800b\"", &[]);
    assert_eq!(value.to_edn(), "\"a\\ud800b\"");
    let mut from_file = light_cljs::BuildConfiguration::new();
    from_file.insert("app/v", &value.to_edn()).unwrap();
    let mut from_flag = light_cljs::BuildConfiguration::new();
    from_flag.insert_text("app/v=\"a\\uD800b\"").unwrap();
    assert_eq!(from_file.text(), from_flag.text());
    // No path takes one.
    assert_eq!(
        message(
            &app_body(
                ":target :esm
   :output-dir \"out\\uD800\"
   :modules {:main {:entries [app.main]}}"
            ),
            &[]
        ),
        "light-cljs.edn:5:16: :output-dir of build :app must be a directory string, got \
         \"out\\ud800\""
    );
}

#[test]
fn environment_conversions_refuse_what_is_no_symbol_or_keyword() {
    let symbol = app_modules("{:main {:entries [#light-cljs/env [\"ENTRY\" :as :symbol]]}}");
    for (text, quoted) in [
        ("app.main other", "\"app.main other\""),
        ("", "\"\""),
        (":app.main", "\":app.main\""),
    ] {
        assert_eq!(
            message(&symbol, &[("ENTRY", text)]),
            format!("light-cljs.edn:6:31: ENTRY is {quoted}, not a symbol (:as :symbol)")
        );
    }
    let keyword = app(":config {:app/v #light-cljs/env [\"KIND\" :as :keyword]}");
    for (text, quoted) in [(":dev", "\":dev\""), ("", "\"\""), ("a b", "\"a b\"")] {
        assert_eq!(
            message(&keyword, &[("KIND", text)]),
            format!("light-cljs.edn:7:20: KIND is {quoted}, not a keyword name (:as :keyword)")
        );
    }
}

#[test]
fn a_file_that_is_not_one_map_is_an_error() {
    assert_eq!(
        message("", &[]),
        "light-cljs.edn:1:1: the configuration file is empty; it holds one map, such as \
         {:source-paths [\"src\"] :builds {...}}"
    );
    assert_eq!(
        message("[:source-paths]", &[]),
        "light-cljs.edn:1:1: the configuration file holds one map of :source-paths and \
         :builds, got [:source-paths]"
    );
    assert_eq!(
        message("{:source-paths [\"src\"] :builds {}}\n{}", &[]),
        "light-cljs.edn:2:1: the configuration file holds one map; this is a second form"
    );
    assert_eq!(
        message("{:source-paths [\"src\"] :builds {}}", &[]),
        "light-cljs.edn:1:32: :builds must hold at least one build, such as {:app {:target \
         :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}"
    );
    assert_eq!(
        message(
            "{:builds {:app {:target :esm :output-dir \"out\" :modules {:main {:entries [a]}}}}}",
            &[]
        ),
        "light-cljs.edn:1:1: the configuration file has no :source-paths, the directories its \
         builds read namespaces from; add one, such as :source-paths [\"src\"]"
    );
}

#[test]
fn a_required_build_key_may_come_from_both_mode_maps() {
    let both = app_body(
        ":target :esm
   :modules {:main {:entries [app.main]}}
   :dev {:output-dir \"out/dev\"}
   :release {:output-dir \"out/release\"}",
    );
    assert_eq!(message(&both, &[]), "no error");
    let one = app_body(
        ":target :esm
   :modules {:main {:entries [app.main]}}
   :dev {:output-dir \"out/dev\"}",
    );
    assert_eq!(
        message(&one, &[]),
        "light-cljs.edn:3:3: build :app has no :output-dir in release mode, which every build \
         needs; give it in the build or under :release"
    );
    let none = app_body(":target :esm :modules {:main {:entries [app.main]}}");
    assert_eq!(
        message(&none, &[]),
        "light-cljs.edn:3:3: build :app has no :output-dir, which every build needs"
    );
}

// ---------------------------------------------------------------------------
// #light-cljs/env

/// The value of `:config {:app/v FORM}` in build `:app`.
fn config_value(form: &str, environment: &[(&str, &str)]) -> Value {
    let file = read(&app(&format!(":config {{:app/v {form}}}")), environment)
        .unwrap_or_else(|error| panic!("{form}: {error}"));
    file.build(BuildId(0))
        .setting(None, Setting::Config)
        .and_then(|config| config.get("app/v"))
        .cloned()
        .unwrap()
}

/// The kind a literal has in the same place.
fn literal(text: &str) -> ValueKind {
    let value = config_value(text, &[]);
    assert!(value.environment.is_none());
    value.kind
}

#[test]
fn each_environment_form_reads_its_variable_or_its_default() {
    let set = [("APP_V", "x")];
    for form in ["\"APP_V\"", "[\"APP_V\"]"] {
        let form = format!("#light-cljs/env {form}");
        assert_eq!(config_value(&form, &set).kind, literal("\"x\""), "{form}");
        let unset = config_value(&form, &[]);
        assert_eq!(unset.kind, ValueKind::Nil, "{form}");
        let origin = unset.environment.unwrap();
        assert_eq!((origin.variable.as_str(), origin.set), ("APP_V", false));
    }
    let form = "#light-cljs/env [\"APP_V\" :fallback]";
    assert_eq!(config_value(form, &set).kind, literal("\"x\""));
    assert_eq!(config_value(form, &[]).kind, literal(":fallback"));
    let form = "#light-cljs/env [\"APP_V\" :default {:a [1 2]}]";
    assert_eq!(config_value(form, &set).kind, literal("\"x\""));
    // A collection default's elements keep the default's own spans.
    assert_eq!(config_value(form, &[]).to_edn(), "{:a [1 2]}");

    // The value keeps the tag's span and its variable.
    let source = app(":config {:app/v #light-cljs/env \"APP_V\"}");
    let file = read(&source, &set).unwrap();
    let value = file
        .build(BuildId(0))
        .setting(None, Setting::Config)
        .unwrap()
        .get("app/v")
        .unwrap();
    assert_eq!(
        &source[value.span.clone()],
        "#light-cljs/env \"APP_V\"",
        "the span of the whole tag"
    );
    let origin = value.environment.as_ref().unwrap();
    assert_eq!((origin.variable.as_str(), origin.set), ("APP_V", true));
}

#[test]
fn each_conversion_gives_what_a_literal_would() {
    let int = "#light-cljs/env [\"APP_N\" :as :int :default 3]";
    assert_eq!(config_value(int, &[("APP_N", "42")]).kind, literal("42"));
    assert_eq!(config_value(int, &[("APP_N", "-7")]).kind, literal("-7"));
    assert_eq!(config_value(int, &[("APP_N", "+5")]).kind, literal("5"));
    assert_eq!(config_value(int, &[]).kind, literal("3"));

    let bool = "#light-cljs/env [\"APP_B\" :as :bool]";
    assert_eq!(
        config_value(bool, &[("APP_B", "true")]).kind,
        literal("true")
    );
    for other in ["TRUE", "1", "yes", "false", ""] {
        assert_eq!(
            config_value(bool, &[("APP_B", other)]).kind,
            literal("false"),
            "{other:?}"
        );
    }
    assert_eq!(config_value(bool, &[]).kind, ValueKind::Nil);
    let bool_default = "#light-cljs/env [\"APP_B\" :as :bool :default true]";
    assert_eq!(config_value(bool_default, &[]).kind, literal("true"));

    let keyword = "#light-cljs/env [\"APP_K\" :as :keyword :default :prod]";
    assert_eq!(
        config_value(keyword, &[("APP_K", "dev")]).kind,
        literal(":dev")
    );
    assert_eq!(
        config_value(keyword, &[("APP_K", "app/dev")]).kind,
        literal(":app/dev")
    );
    assert_eq!(config_value(keyword, &[]).kind, literal(":prod"));
    let default_only = "#light-cljs/env [\"APP_K\" :default \"d\"]";
    assert_eq!(
        config_value(default_only, &[("APP_K", "v")]).kind,
        literal("\"v\"")
    );
    assert_eq!(config_value(default_only, &[]).kind, literal("\"d\""));

    // A symbol lands where a literal symbol would: a module's entry.
    let entry = |environment: &[(&str, &str)]| {
        let file = read(
            &app_modules(
                "{:main {:entries [#light-cljs/env [\"ENTRY\" :as :symbol :default app.main]]}}",
            ),
            environment,
        )
        .unwrap();
        let (module, namespace) = file.build(BuildId(0)).entry(None).unwrap();
        (module.to_owned(), namespace.to_owned())
    };
    assert_eq!(
        entry(&[("ENTRY", "app.other")]),
        ("main".into(), "app.other".into())
    );
    assert_eq!(entry(&[]), ("main".into(), "app.main".into()));

    // :config refuses what --config refuses, at the tag.
    assert_eq!(
        message(
            &app(":config {:app/v #light-cljs/env [\"APP_S\" :as :symbol]}"),
            &[("APP_S", "app.x")]
        ),
        "light-cljs.edn:7:20: :config :app/v of build :app: \"app.x\" is not a configuration \
         value: use keywords, numbers, strings, booleans, nil and lists, vectors, maps and \
         sets of them"
    );
}

#[test]
fn an_unset_variable_without_default_is_kept_for_the_mode_that_runs() {
    // Required where it lands, but checked only for the build and mode
    // that run, which reading does not know.
    let file = read(
        &app(":release {:closure-defines {app.config/VERSION #light-cljs/env \"APP_VERSION\"}}"),
        &[],
    )
    .unwrap();
    let defines = file
        .build(BuildId(0))
        .setting(Some(BuildMode::Release), Setting::ClosureDefines)
        .unwrap();
    let ValueKind::Map(entries) = &defines.kind else {
        panic!("{defines:?}")
    };
    assert!(entries[0].1.is_unset_environment());
}

// ---------------------------------------------------------------------------
// Discovery

#[test]
fn discovery_walks_up_to_the_first_file() {
    let root = TempDir::new("config-discovery");
    let deep = root.join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(ConfigFile::discover(&deep), None, "no file anywhere above");

    fs::write(root.join("a").join(FILE_NAME), "{}").unwrap();
    assert_eq!(
        ConfigFile::discover(&deep),
        Some(root.join("a").join(FILE_NAME)),
        "two levels up"
    );

    fs::write(deep.join(FILE_NAME), "{}").unwrap();
    assert_eq!(
        ConfigFile::discover(&deep),
        Some(deep.join(FILE_NAME)),
        "the start directory itself"
    );

    // A directory of that name is not the file.
    let other = root.join("d");
    fs::create_dir_all(other.join(FILE_NAME)).unwrap();
    assert_eq!(ConfigFile::discover(&other), None);
}

// ---------------------------------------------------------------------------
// The document's examples

#[test]
fn the_features_example_reads_with_no_error() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("features")
        .join(FILE_NAME);
    let file = ConfigFile::read(&path, &|_| None).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(file.builds().len(), 1);
    assert_eq!(file.find_build("features"), Some(BuildId(0)));
    let build = file.build(BuildId(0));
    assert_eq!(build.name(), "features");
    assert_eq!(build.entry(None), Some(("main", "app.main")));
    assert_eq!(
        build
            .setting(None, Setting::OutputDir)
            .and_then(Value::as_str),
        Some("out/release")
    );
    assert_eq!(
        build
            .setting(Some(BuildMode::Dev), Setting::OutputDir)
            .and_then(Value::as_str),
        Some("out/dev")
    );
    assert_eq!(
        build.setting(Some(BuildMode::Release), Setting::OutputDir),
        None
    );
    let paths = file.setting(Setting::SourcePaths).unwrap();
    assert_eq!(paths.to_edn(), "[\".\"]");
}

/// The first `clojure` block under `heading` in docs/build-configuration.md.
fn document_example(heading: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("build-configuration.md");
    let document = fs::read_to_string(path).unwrap();
    let section = &document[document.find(heading).expect(heading)..];
    let start = section.find("```clojure\n").unwrap() + "```clojure\n".len();
    section[start..start + section[start..].find("```").unwrap()].to_owned()
}

#[test]
fn the_documents_full_illustration_reads_with_and_without_its_variables() {
    let source = document_example("### A full illustration");
    for environment in [
        &[][..],
        &[("HTTP_PORT", "8000"), ("APP_VERSION", "2.4.0")][..],
    ] {
        let file = read(&source, environment).unwrap_or_else(|error| panic!("{error}"));
        let names: Vec<&str> = file.builds().iter().map(|build| build.name()).collect();
        assert_eq!(names, ["app", "test", "worker"]);
        for (index, name) in names.iter().enumerate() {
            assert_eq!(file.find_build(name), Some(BuildId(index as u32)));
        }
        assert_eq!(file.find_build("ap"), None);
        let set = !environment.is_empty();
        let port = file.setting(Setting::HttpPort).unwrap();
        assert_eq!(port.to_edn(), if set { "8000" } else { "9630" });
        assert_eq!(port.environment.as_ref().unwrap().set, set);
        let app = file.build(file.find_build("app").unwrap());
        let defines = app
            .setting(Some(BuildMode::Release), Setting::ClosureDefines)
            .unwrap();
        assert_eq!(
            defines.to_edn(),
            if set {
                "{app.config/DEBUG false app.config/VERSION \"2.4.0\"}"
            } else {
                "{app.config/DEBUG false app.config/VERSION \"dev\"}"
            }
        );
        assert_eq!(
            app.setting(Some(BuildMode::Dev), Setting::Config)
                .unwrap()
                .to_edn(),
            "{:app/api-origin \"http://127.0.0.1:8080\"}"
        );
        let test = file.build(file.find_build("test").unwrap());
        assert_eq!(test.entry(None), Some(("main", "app.test-runner")));
        assert_eq!(
            test.setting(None, Setting::Optimizations).unwrap().to_edn(),
            ":none"
        );
    }
}
