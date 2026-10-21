//! Merging a build's layers (docs/build-configuration.md, "Modes",
//! "LIGHT_CLJS and --config-merge", "Precedence" layers 1-5, and the rows of
//! "Errors" that need merging): shadow-cljs's `deep-merge`, each layer over
//! the one below with the winning value's origin, `LIGHT_CLJS`,
//! `--config-merge`, the checks after merging, and the features example.

use std::{collections::HashMap, fs, io, path::Path};

use light_cljs::config_file::{
    BuildId, BuildMode, BuildSettings, ConfigFile, ConfigFileError, ConfigMerge, ConfigSource,
    FILE_NAME, Setting, Value, ValueKind, deep_merge,
};

use crate::temp_dir::TempDir;

const DEV: BuildMode = BuildMode::Dev;
const RELEASE: BuildMode = BuildMode::Release;
const CODE_SPLITTING: &str = "chunks are inferred from light.lazy/loadable sites \
                              (docs/code-splitting.md), and named chunks arrive with its Phase 4";

fn lookup(environment: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let environment: HashMap<String, String> = environment
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    move |name| environment.get(name).cloned()
}

fn read(source: &str, environment: &[(&str, &str)]) -> Result<ConfigFile, ConfigFileError> {
    ConfigFile::from_source(
        Path::new(FILE_NAME),
        source.to_owned(),
        &lookup(environment),
    )
}

fn file(source: &str, environment: &[(&str, &str)]) -> ConfigFile {
    read(source, environment).unwrap_or_else(|error| panic!("{error}"))
}

/// The `index`th `--config-merge DATA`, relative paths in the current
/// directory.
fn merge(index: usize, data: &str, environment: &[(&str, &str)]) -> ConfigMerge {
    ConfigMerge::parse(index, data, Path::new(""), &lookup(environment))
        .unwrap_or_else(|error| panic!("{error}"))
}

fn settings(
    file: &ConfigFile,
    build: &str,
    mode: BuildMode,
    merges: &[ConfigMerge],
) -> Result<BuildSettings, String> {
    let id = file.find_build(build).expect(build);
    file.settings(id, mode, merges)
        .map_err(|error| error.to_string())
}

fn ok(file: &ConfigFile, build: &str, mode: BuildMode, merges: &[ConfigMerge]) -> BuildSettings {
    settings(file, build, mode, merges).unwrap_or_else(|error| panic!("{error}"))
}

/// A setting's value as EDN and its origin, or `None`.
fn shown(settings: &BuildSettings, setting: Setting) -> Option<(String, String)> {
    let slot = settings.slot(setting)?;
    Some((slot.value.to_edn(), slot.origin.to_string()))
}

fn pair(value: &str, origin: &str) -> Option<(String, String)> {
    Some((value.to_owned(), origin.to_owned()))
}

/// A value of EDN text, as a `--config-merge` reads it.
fn value(edn: &str) -> Value {
    let merge = merge(1, &format!("{{:config {{:v {edn}}}}}"), &[]);
    let config = &merge.layer().get(Setting::Config).unwrap().value;
    config.get("v").unwrap().clone()
}

// ---------------------------------------------------------------------------
// deep-merge

#[test]
fn deep_merge_follows_shadow_cljs() {
    // shadow/build/api.clj deep-merge, case by case: (lower upper merged).
    let cases = [
        // nil on either side.
        ("nil", "1", "1"),
        ("1", "nil", "1"),
        ("{:a 1}", "nil", "{:a 1}"),
        // Maps by key, nested two deep.
        (
            "{:a {:b {:c 1 :d 1} :e 1} :f 1}",
            "{:a {:b {:d 2 :g 2}} :h 2}",
            "{:a {:b {:c 1 :d 2 :g 2} :e 1} :f 1 :h 2}",
        ),
        ("{:a {:b 1}}", "{:a {:b nil}}", "{:a {:b 1}}"),
        // Vectors concatenated, each distinct element once, in order.
        ("[1 2 2 3]", "[3 4 1 5]", "[1 2 3 4 5]"),
        ("[[1] {:a 1}]", "[[1] {:a 2}]", "[[1] {:a 1} {:a 2}]"),
        ("[]", "[1]", "[1]"),
        // Sets unioned.
        ("#{1 2}", "#{2 3}", "#{1 2 3}"),
        ("#{:a}", "#{}", "#{:a}"),
        // A scalar over a map, and every scalar kind over another.
        ("{:a 1}", "\"text\"", "\"text\""),
        ("{:a 1}", "2", "2"),
        ("{:a 1}", "false", "false"),
        ("{:a 1}", ":k", ":k"),
        ("[1]", "1.5", "1.5"),
        ("#{1}", "true", "true"),
        ("\"a\"", "\"b\"", "\"b\""),
        // Anything else is replaced: a list, and a collection over another kind.
        ("(1 2)", "(3)", "(3)"),
        ("[1 2]", "#{3}", "#{3}"),
        ("1", "{:a 1}", "{:a 1}"),
    ];
    for (lower, upper, expected) in cases {
        assert_eq!(
            deep_merge(value(lower), value(upper)).to_edn(),
            expected,
            "{lower} under {upper}"
        );
    }
}

#[test]
fn deep_merge_keeps_each_elements_origin() {
    let lower = merge(1, "{:config {:v {:a 1 :b [1]}}}", &[]);
    let upper = merge(2, "{:config {:v {:b [2] :c 3}}}", &[]);
    let get = |merge: &ConfigMerge| {
        merge
            .layer()
            .get(Setting::Config)
            .unwrap()
            .value
            .get("v")
            .unwrap()
            .clone()
    };
    let merged = deep_merge(get(&lower), get(&upper));
    assert_eq!(merged.to_edn(), "{:a 1 :b [1 2] :c 3}");
    assert_eq!(merged.source, ConfigSource::ConfigMerge(2));
    assert_eq!(
        merged.get("a").unwrap().source,
        ConfigSource::ConfigMerge(1)
    );
    assert_eq!(
        merged.get("b").unwrap().source,
        ConfigSource::ConfigMerge(2)
    );
    let ValueKind::Vector(items) = &merged.get("b").unwrap().kind else {
        panic!("a vector")
    };
    let sources: Vec<ConfigSource> = items.iter().map(|item| item.source).collect();
    assert_eq!(
        sources,
        [ConfigSource::ConfigMerge(1), ConfigSource::ConfigMerge(2)]
    );
}

// ---------------------------------------------------------------------------
// Layers

/// A file of one build `:app`, with `build` in the build at 7:4 and
/// `release` in its `:release` map at 8:14.
fn app(build: &str, release: &str) -> String {
    format!(
        "{{:source-paths [\"src\"]
 :builds
 {{:app
  {{:target :esm
   :output-dir \"out\"
   :modules {{:main {{:entries [app.main]}}}}
   {build}
   :release {{{release}}}}}}}}}"
    )
}

/// The release settings of `app` with the first `count` of `layers`: the
/// build's own map, `LIGHT_CLJS`, the `:release` map and two
/// `--config-merge`s, lowest first.
fn layered(layers: [&str; 5], count: usize) -> (ConfigFile, Vec<ConfigMerge>, BuildSettings) {
    let used = |index: usize| if index < count { layers[index] } else { "" };
    let file = file(&app(used(0), used(2)), &[("LIGHT_CLJS", used(1))]);
    let merges: Vec<ConfigMerge> = (3..count)
        .map(|index| merge(index - 2, layers[index], &[]))
        .collect();
    let settings = ok(&file, "app", RELEASE, &merges);
    (file, merges, settings)
}

#[test]
fn each_layer_overrides_the_one_below_for_a_scalar() {
    let layers = [
        ":compiler-options {:macro-timeout-ms 20000}",
        "{:builds {:app {:compiler-options {:macro-timeout-ms 30000}}}}",
        ":compiler-options {:macro-timeout-ms 40000}",
        "{:compiler-options {:macro-timeout-ms 50000}}",
        "{:compiler-options {:macro-timeout-ms 60000}}",
    ];
    let expected = [
        ("10000", "default"),
        ("20000", "light-cljs.edn:7:41"),
        ("30000", "LIGHT_CLJS:1:54"),
        ("40000", "light-cljs.edn:8:51 (:release)"),
        ("50000", "--config-merge 1:1:39"),
        ("60000", "--config-merge 2:1:39"),
    ];
    for (count, (value, origin)) in expected.into_iter().enumerate() {
        let (_, _, settings) = layered(layers, count);
        assert_eq!(
            shown(&settings, Setting::MacroTimeoutMs),
            pair(value, origin),
            "{count} layers"
        );
    }
}

#[test]
fn each_layer_overrides_the_one_below_for_a_map() {
    let layers = [
        ":config {:app/a 1 :app/b 1}",
        "{:builds {:app {:config {:app/b 2 :app/c 2}}}}",
        ":config {:app/c 3 :app/d 3}",
        "{:config {:app/d 4}}",
        "{:config {:app/e 5}}",
    ];
    let expected = [
        None,
        pair("{:app/a 1 :app/b 1}", "light-cljs.edn:7:12"),
        pair("{:app/a 1 :app/b 2 :app/c 2}", "LIGHT_CLJS:1:25"),
        pair(
            "{:app/a 1 :app/b 2 :app/c 3 :app/d 3}",
            "light-cljs.edn:8:22 (:release)",
        ),
        pair(
            "{:app/a 1 :app/b 2 :app/c 3 :app/d 4}",
            "--config-merge 1:1:10",
        ),
        pair(
            "{:app/a 1 :app/b 2 :app/c 3 :app/d 4 :app/e 5}",
            "--config-merge 2:1:10",
        ),
    ];
    for (count, expected) in expected.into_iter().enumerate() {
        let (_, _, settings) = layered(layers, count);
        assert_eq!(
            shown(&settings, Setting::Config),
            expected,
            "{count} layers"
        );
    }
    // Each entry keeps the origin of the layer that gave it.
    let (file, merges, settings) = layered(layers, 5);
    let config = settings.get(Setting::Config).unwrap();
    let origins: Vec<String> = ["app/a", "app/b", "app/c", "app/d", "app/e"]
        .iter()
        .map(|key| file.origin(config.get(key).unwrap(), &merges).to_string())
        .collect();
    assert_eq!(
        origins,
        [
            "light-cljs.edn:7:20",
            "LIGHT_CLJS:1:33",
            "light-cljs.edn:8:30",
            "--config-merge 1:1:18",
            "--config-merge 2:1:18",
        ]
    );
}

#[test]
fn each_layer_overrides_the_one_below_for_a_vector() {
    let layers = [
        ":config {:app/list [1 2]}",
        "{:builds {:app {:config {:app/list [2 3]}}}}",
        ":config {:app/list [3 4]}",
        "{:config {:app/list [4 5]}}",
        "{:config {:app/list [5 1 6]}}",
    ];
    let expected = [
        ("[1 2]", "light-cljs.edn:7:23"),
        ("[1 2 3]", "LIGHT_CLJS:1:36"),
        ("[1 2 3 4]", "light-cljs.edn:8:33"),
        ("[1 2 3 4 5]", "--config-merge 1:1:21"),
        ("[1 2 3 4 5 6]", "--config-merge 2:1:21"),
    ];
    for (index, (value, origin)) in expected.into_iter().enumerate() {
        let (file, merges, settings) = layered(layers, index + 1);
        let list = settings
            .get(Setting::Config)
            .unwrap()
            .get("app/list")
            .unwrap();
        assert_eq!(
            (list.to_edn(), file.origin(list, &merges).to_string()),
            (value.to_owned(), origin.to_owned()),
            "{} layers",
            index + 1
        );
    }

    // The top-level vector :source-paths, under LIGHT_CLJS.
    let source = app("", "");
    let without = ok(&file(&source, &[]), "app", DEV, &[]);
    assert_eq!(
        shown(&without, Setting::SourcePaths),
        pair("[\"src\"]", "light-cljs.edn:1:16")
    );
    let with = ok(
        &file(
            &source,
            &[("LIGHT_CLJS", "{:source-paths [\"test\" \"src\"]}")],
        ),
        "app",
        DEV,
        &[],
    );
    assert_eq!(
        shown(&with, Setting::SourcePaths),
        pair("[\"src\" \"test\"]", "LIGHT_CLJS:1:16")
    );
}

#[test]
fn each_layer_overrides_the_one_below_for_a_set() {
    let layers = [
        ":build-options {:cache-blockers #{a.one}}",
        "{:builds {:app {:build-options {:cache-blockers #{a.two}}}}}",
        ":build-options {:cache-blockers #{a.three}}",
        "{:build-options {:cache-blockers #{a.four}}}",
        "{:build-options {:cache-blockers #{a.five a.one}}}",
    ];
    let expected = [
        ("#{}", "default"),
        ("#{a.one}", "light-cljs.edn:7:36"),
        ("#{a.one a.two}", "LIGHT_CLJS:1:49"),
        ("#{a.one a.two a.three}", "light-cljs.edn:8:46 (:release)"),
        ("#{a.one a.two a.three a.four}", "--config-merge 1:1:34"),
        (
            "#{a.one a.two a.three a.four a.five}",
            "--config-merge 2:1:34",
        ),
    ];
    for (count, (value, origin)) in expected.into_iter().enumerate() {
        let (_, _, settings) = layered(layers, count);
        assert_eq!(
            shown(&settings, Setting::BuildCacheBlockers),
            pair(value, origin),
            "{count} layers"
        );
    }
}

#[test]
fn defaults_are_todays_flag_defaults_with_releases_advanced() {
    let file = file(&app("", ""), &[]);
    let release = ok(&file, "app", RELEASE, &[]);
    let dev = ok(&file, "app", DEV, &[]);
    let defaults = [
        (Setting::CacheRoot, "\".light-cljs\"", "\".light-cljs\""),
        (Setting::CacheBlockers, "#{}", "#{}"),
        (Setting::CompileThreads, "1", "1"),
        (Setting::HttpPort, "9630", "9630"),
        (Setting::HttpHost, "\"127.0.0.1\"", "\"127.0.0.1\""),
        (Setting::FsWatchLoopWait, "100", "100"),
        (Setting::Optimizations, ":none", ":advanced"),
        (Setting::PseudoNames, "false", "false"),
        (Setting::ElideAsserts, "false", "false"),
        (Setting::CljsWarnings, "true", "false"),
        (Setting::MacroExpansions, "1024", "1024"),
        (Setting::MacroNodes, "100000", "100000"),
        (Setting::MacroTimeoutMs, "10000", "10000"),
        (Setting::MacroMemoryLimit, "67108864", "67108864"),
        (Setting::MacroPollBudget, "1000000", "1000000"),
        (Setting::CacheLevel, ":all", ":all"),
        (Setting::BuildCacheBlockers, "#{}", "#{}"),
    ];
    for (setting, in_dev, in_release) in defaults {
        assert_eq!(shown(&dev, setting), pair(in_dev, "default"), "{setting:?}");
        assert_eq!(
            shown(&release, setting),
            pair(in_release, "default"),
            "{setting:?}"
        );
    }
    let unset = [
        Setting::HttpStrict,
        Setting::Nrepl,
        Setting::FsWatchImpl,
        Setting::Config,
        Setting::ClosureDefines,
        Setting::CheckedArrays,
        Setting::CompilerClosureDefines,
        Setting::ReloadStrategy,
    ];
    for setting in unset {
        assert_eq!(shown(&release, setting), None, "{setting:?}");
    }
    assert_eq!(
        shown(&release, Setting::OutputDir),
        pair("\"out\"", "light-cljs.edn:5:16")
    );
    assert_eq!(release.build(), BuildId(0));
    assert_eq!(release.name(), "app");
    assert_eq!(release.mode(), RELEASE);
}

#[test]
fn build_defines_merge_over_the_compiler_options_and_top_blockers_into_the_builds() {
    let source = "{:source-paths [\"src\"]
 :cache-blockers #{a.top}
 :builds
 {:app
  {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}
   :build-options {:cache-blockers #{a.build}}
   :release {:compiler-options {:closure-defines {app.config/A 1 app.config/B 1}}
             :closure-defines {app.config/B 2}}}
  :other
  {:target :esm :output-dir \"other\" :modules {:main {:entries [app.other]}}}}}";
    let file = file(source, &[]);
    let merges = [merge(
        1,
        "{:compiler-options {:closure-defines {app.config/B 9 app.config/C 3}}}",
        &[],
    )];
    let release = ok(&file, "app", RELEASE, &merges);
    assert_eq!(
        shown(&release, Setting::ClosureDefines),
        pair(
            "{app.config/A 1 app.config/B 2 app.config/C 3}",
            "light-cljs.edn:8:31 (:release)"
        )
    );
    assert_eq!(shown(&release, Setting::CompilerClosureDefines), None);
    assert_eq!(
        shown(&release, Setting::CacheBlockers),
        pair("#{a.top}", "light-cljs.edn:2:18")
    );
    assert_eq!(
        shown(&release, Setting::BuildCacheBlockers),
        pair("#{a.top a.build}", "light-cljs.edn:6:36")
    );
    let dev = ok(&file, "app", DEV, &[]);
    assert_eq!(shown(&dev, Setting::ClosureDefines), None);
    // A build without its own blockers has the top level's, from there.
    let other = ok(&file, "other", DEV, &[]);
    assert_eq!(
        shown(&other, Setting::BuildCacheBlockers),
        pair("#{a.top}", "light-cljs.edn:2:18")
    );
}

// ---------------------------------------------------------------------------
// LIGHT_CLJS

#[test]
fn light_cljs_is_merged_over_the_whole_file() {
    let source = "{:source-paths [\"src\"]
 :compile-threads 2
 :builds {:app {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}";
    let light = "{:compile-threads 3
 :http {:port 8000}
 :builds {:app {:dev {:output-dir \"out/dev\"}}
          :worker {:target :esm :output-dir \"out/worker\"
                   :modules {:main {:entries [app.worker]}}}}}";
    let file = file(source, &[("LIGHT_CLJS", light)]);
    assert_eq!(file.light_cljs(), Some(light));
    let names: Vec<&str> = file.builds().iter().map(|build| build.name()).collect();
    assert_eq!(names, ["app", "worker"]);
    assert_eq!(file.build(BuildId(0)).source(), ConfigSource::File);
    assert_eq!(file.build(BuildId(1)).source(), ConfigSource::LightCljs);

    let dev = ok(&file, "app", DEV, &[]);
    assert_eq!(
        shown(&dev, Setting::CompileThreads),
        pair("3", "LIGHT_CLJS:1:19")
    );
    assert_eq!(
        shown(&dev, Setting::HttpPort),
        pair("8000", "LIGHT_CLJS:2:15")
    );
    assert_eq!(
        shown(&dev, Setting::OutputDir),
        pair("\"out/dev\"", "LIGHT_CLJS:3:35 (:dev)")
    );
    let release = ok(&file, "app", RELEASE, &[]);
    assert_eq!(
        shown(&release, Setting::OutputDir),
        pair("\"out\"", "light-cljs.edn:3:42")
    );
    let worker = ok(&file, "worker", RELEASE, &[]);
    assert_eq!(
        shown(&worker, Setting::OutputDir),
        pair("\"out/worker\"", "LIGHT_CLJS:4:45")
    );

    // LIGHT_CLJS may give what the file leaves out; empty or holding no
    // form, it is not there.
    let partial = "{:builds {:app {:output-dir \"out\" :modules {:main {:entries [app.main]}}}}}";
    let light = "{:source-paths [\"src\"] :builds {:app {:target :esm}}}";
    let merged = file_with(partial, light);
    assert_eq!(
        shown(&ok(&merged, "app", DEV, &[]), Setting::Target),
        pair(":esm", "LIGHT_CLJS:1:47")
    );
    for none in ["", ";; nothing"] {
        assert_eq!(
            read(partial, &[("LIGHT_CLJS", none)])
                .map(|_| ())
                .unwrap_err()
                .to_string(),
            "light-cljs.edn:1:11: build :app has no :target, which every build needs",
            "{none:?}"
        );
    }
    let light = "{:builds {:app {:target :esm}}}";
    assert_eq!(
        read(partial, &[("LIGHT_CLJS", light)])
            .map(|_| ())
            .unwrap_err()
            .to_string(),
        "light-cljs.edn:1:1: the configuration file has no :source-paths, the directories \
         its builds read namespaces from; add one, such as :source-paths [\"src\"]"
    );
}

fn file_with(source: &str, light: &str) -> ConfigFile {
    file(source, &[("LIGHT_CLJS", light)])
}

#[test]
fn light_cljs_errors_are_located_in_it() {
    let source = app("", "");
    let rows = [
        (
            "{:builds {:app",
            "LIGHT_CLJS:1:15: unexpected end of input; expected a closing delimiter",
        ),
        (
            "{:cache-rot \".x\"}",
            "LIGHT_CLJS:1:2: unknown key :cache-rot at the top level; did you mean :cache-root?",
        ),
        (
            "{:builds {:app {:output-path \"x\"}}}",
            "LIGHT_CLJS:1:17: unknown key :output-path in build :app; did you mean :output-dir?",
        ),
        (
            "{:builds\n {:app {:compiler-options {:optimizations :simple}}}}",
            "LIGHT_CLJS:2:43: :optimizations of build :app must be :none, :prepare or \
             :advanced, got :simple",
        ),
        (
            "[1]",
            "LIGHT_CLJS:1:1: LIGHT_CLJS holds one map of the configuration file's shape, such \
             as {:builds {:app {:output-dir \"out\"}}}, got [1]",
        ),
        (
            "{} {}",
            "LIGHT_CLJS:1:4: LIGHT_CLJS holds one map; this is a second form",
        ),
        (
            "{:cache-root #shadow/env \"X\"}",
            "LIGHT_CLJS:1:14: #shadow/env: write #light-cljs/env, which takes the same forms",
        ),
        (
            "{:builds {:app {:closure-defines {app.config/DEBUG true}}}}",
            "LIGHT_CLJS:1:17: :closure-defines of build :app applies in dev mode, whose dev \
             build takes no defines; write it under :release",
        ),
        (
            "{:builds {:worker {:output-dir \"w\"}}}",
            "LIGHT_CLJS:1:11: build :worker has no :target, which every build needs",
        ),
        (
            "{:http {:port #light-cljs/env [\"PORT\" :as :int]}}",
            "LIGHT_CLJS:1:15: PORT is \"web\", not a decimal integer (:as :int)",
        ),
    ];
    for (light, expected) in rows {
        let message = match read(&source, &[("LIGHT_CLJS", light), ("PORT", "web")]) {
            Ok(_) => "no error".into(),
            Err(error) => error.to_string(),
        };
        assert_eq!(message, expected, "{light}");
    }
}

// ---------------------------------------------------------------------------
// --config-merge

#[test]
fn config_merges_are_edn_text_or_a_file_and_apply_in_order() {
    let directory = TempDir::new("config-merge");
    fs::write(
        directory.join("release.edn"),
        ";; a merge file\n{:output-dir \"out/file\"\n :compiler-options {:elide-asserts true}}",
    )
    .unwrap();
    let file = file(&app("", ""), &[]);
    let text =
        ConfigMerge::parse(1, "{:output-dir \"out/text\"}", directory.path(), &|_| None).unwrap();
    let from_file = ConfigMerge::parse(2, "release.edn", directory.path(), &|_| None).unwrap();
    assert_eq!(text.index(), 1);
    assert_eq!(text.source(), "{:output-dir \"out/text\"}");
    assert_eq!(from_file.index(), 2);
    assert!(from_file.source().starts_with(";; a merge file"));

    let one = ok(&file, "app", RELEASE, std::slice::from_ref(&text));
    assert_eq!(
        shown(&one, Setting::OutputDir),
        pair("\"out/text\"", "--config-merge 1:1:14")
    );
    let both = ok(&file, "app", RELEASE, &[text.clone(), from_file.clone()]);
    assert_eq!(
        shown(&both, Setting::OutputDir),
        pair("\"out/file\"", "--config-merge 2:2:14")
    );
    assert_eq!(
        shown(&both, Setting::ElideAsserts),
        pair("true", "--config-merge 2:3:36")
    );
    // The same two in the other order: the last one wins.
    let text_last = ConfigMerge::parse(2, text.source(), directory.path(), &|_| None).unwrap();
    let file_first = ConfigMerge::parse(1, "release.edn", directory.path(), &|_| None).unwrap();
    let reversed = ok(&file, "app", RELEASE, &[file_first, text_last]);
    assert_eq!(
        shown(&reversed, Setting::OutputDir),
        pair("\"out/text\"", "--config-merge 2:1:14")
    );
    // A merge turns off a boolean no flag negates.
    let off = merge(3, "{:compiler-options {:elide-asserts false}}", &[]);
    let merged = ok(&file, "app", RELEASE, &[from_file, off]);
    assert_eq!(
        shown(&merged, Setting::ElideAsserts),
        pair("false", "--config-merge 3:1:36")
    );
}

#[test]
fn config_merges_read_environment_values() {
    let file = file(&app("", ""), &[]);
    let data = "{:output-dir #light-cljs/env \"OUT\"}";
    let set = merge(1, data, &[("OUT", "out/env")]);
    assert_eq!(
        shown(&ok(&file, "app", DEV, &[set]), Setting::OutputDir),
        pair("\"out/env\"", "--config-merge 1:1:14, from OUT")
    );
    // Unset and without a default, it is nil, which keeps what is below it.
    let unset = merge(1, data, &[]);
    assert_eq!(
        shown(&ok(&file, "app", DEV, &[unset]), Setting::OutputDir),
        pair("\"out\"", "light-cljs.edn:5:16")
    );
    let defaulted = merge(1, "{:output-dir #light-cljs/env [\"OUT\" \"out/d\"]}", &[]);
    assert_eq!(
        shown(&ok(&file, "app", DEV, &[defaulted]), Setting::OutputDir),
        pair("\"out/d\"", "--config-merge 1:1:14, OUT unset")
    );
}

#[test]
fn config_merge_errors_have_exact_messages() {
    let directory = TempDir::new("config-merge-errors");
    fs::write(directory.join("vector.edn"), "[:output-dir \"x\"]").unwrap();
    fs::write(directory.join("empty.edn"), "  ;; nothing\n").unwrap();
    let missing = directory.join("missing.edn");
    let not_found = io::Error::from_raw_os_error(2);
    let rows = [
        (
            "classpath:demo.edn".to_owned(),
            "--config-merge 1: classpath:demo.edn: light-cljs has no classpath; give EDN text \
             starting with { or a file path relative to the project directory"
                .to_owned(),
        ),
        (
            "vector.edn".into(),
            "--config-merge 1:1:1: --config-merge must be a map of build keys, such as \
             {:output-dir \"out/x\"}, got [:output-dir \"x\"]"
                .into(),
        ),
        (
            "empty.edn".into(),
            "--config-merge 1:1:1: --config-merge must be a map of build keys, such as \
             {:output-dir \"out/x\"}, got nothing"
                .into(),
        ),
        (
            "missing.edn".into(),
            format!(
                "--config-merge 1: cannot read {}: {not_found}",
                missing.display()
            ),
        ),
        (
            "{:dev {:output-dir \"x\"}}".into(),
            "--config-merge 1:1:2: :dev in --config-merge: the command chooses the mode \
             (compile and watch run :dev, release runs :release), so --config-merge holds the \
             build keys themselves"
                .into(),
        ),
        (
            "{:output-dir \"x\" :release {}}".into(),
            "--config-merge 1:1:18: :release in --config-merge: the command chooses the mode \
             (compile and watch run :dev, release runs :release), so --config-merge holds the \
             build keys themselves"
                .into(),
        ),
        (
            "{:output-path \"x\"}".into(),
            "--config-merge 1:1:2: unknown key :output-path in the merged build; did you mean \
             :output-dir?"
                .into(),
        ),
        (
            "{:source-paths [\"x\"]}".into(),
            "--config-merge 1:1:2: :source-paths in the merged build is a top-level key, as in \
             shadow-cljs"
                .into(),
        ),
        (
            "{:compiler-options {:optimisations :none}}".into(),
            "--config-merge 1:1:21: unknown key :optimisations in :compiler-options of the \
             merged build; did you mean :optimizations?"
                .into(),
        ),
        (
            "{:output-dir 1}".into(),
            "--config-merge 1:1:14: :output-dir of the merged build must be a directory string, \
             got 1"
                .into(),
        ),
        (
            "{:target :browser}".into(),
            "--config-merge 1:1:10: :target of the merged build must be :esm: light-cljs writes \
             ES modules, which run in browsers, Node and bundlers"
                .into(),
        ),
        (
            "{:modules {:main {:entries [a b]}}}".into(),
            format!(
                "--config-merge 1:1:31: module :main of the merged build has two entries; give \
                 one namespace: {CODE_SPLITTING}"
            ),
        ),
        (
            "{:output-dir \"a\"} {}".into(),
            "--config-merge 1:1:19: --config-merge holds one map; this is a second form".into(),
        ),
        (
            "{:output-dir #light-cljs/env 1}".into(),
            "--config-merge 1:1:14: #light-cljs/env takes \"NAME\", [\"NAME\"], [\"NAME\" \
             DEFAULT] or [\"NAME\" :as TYPE :default DEFAULT]"
                .into(),
        ),
    ];
    for (data, expected) in rows {
        let message = match ConfigMerge::parse(1, &data, directory.path(), &|_| None) {
            Ok(_) => "no error".into(),
            Err(error) => error.to_string(),
        };
        assert_eq!(message, expected, "{data}");
    }
}

#[test]
fn merged_settings_are_validated_again() {
    let file = file(&app("", ""), &[]);
    let rows = [
        (
            DEV,
            "{:closure-defines {app.config/DEBUG true}}",
            "--config-merge 2:1:2: :closure-defines of build :app applies in dev mode, whose \
             dev build takes no defines; merge it into release builds only"
                .to_owned(),
        ),
        (
            DEV,
            "{:compiler-options {:checked-arrays :warn}}",
            "--config-merge 2:1:21: :checked-arrays of build :app applies in dev mode, whose \
             dev build checks no array accesses; merge it into release builds only"
                .into(),
        ),
        (
            DEV,
            "{:compiler-options {:closure-defines {app.config/DEBUG true}}}",
            "--config-merge 2:1:21: :closure-defines of build :app applies in dev mode, whose \
             dev build takes no defines; merge it into release builds only"
                .into(),
        ),
        (
            RELEASE,
            "{:closure-defines {app.config/DEBUG true}}",
            "no error".into(),
        ),
        (
            RELEASE,
            "{:modules {:main {:entries [app.other]}}}",
            format!(
                "--config-merge 2:1:29: module :main of build :app (release) has two entries; \
                 give one namespace: {CODE_SPLITTING}"
            ),
        ),
        (
            DEV,
            "{:modules {:other {:entries [app.other]}}}",
            format!("--config-merge 2:1:12: build :app (dev) has two modules; {CODE_SPLITTING}"),
        ),
        // The same entry again is one entry.
        (
            DEV,
            "{:modules {:main {:entries [app.main]}}}",
            "no error".into(),
        ),
    ];
    for (mode, data, expected) in rows {
        let merges = [merge(2, data, &[])];
        let message = match settings(&file, "app", mode, &merges) {
            Ok(_) => "no error".into(),
            Err(error) => error,
        };
        assert_eq!(message, expected, "{data} in {mode:?}");
    }
}

// ---------------------------------------------------------------------------
// Unset variables, for the build and mode that run

/// The document's full illustration, its `APP_VERSION` without a default.
fn illustration_without_version_default() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("build-configuration.md");
    let document = fs::read_to_string(path).unwrap();
    let heading = "### A full illustration";
    let section = &document[document.find(heading).expect(heading)..];
    let start = section.find("```clojure\n").unwrap() + "```clojure\n".len();
    let example = &section[start..start + section[start..].find("```").unwrap()];
    let defaulted = "#light-cljs/env [\"APP_VERSION\" \"dev\"]";
    assert!(example.contains(defaulted));
    example.replace(defaulted, "#light-cljs/env \"APP_VERSION\"")
}

#[test]
fn a_release_only_unset_variable_fails_release_alone() {
    let source = illustration_without_version_default();
    let file = file(&source, &[]);
    let dev = ok(&file, "app", DEV, &[]);
    assert_eq!(shown(&dev, Setting::ClosureDefines), None);
    assert_eq!(
        settings(&file, "app", RELEASE, &[]).map(|_| ()),
        Err(
            "light-cljs.edn:15:51: :closure-defines app.config/VERSION of build :app (release) \
             is nil: APP_VERSION is unset and the form gives no default"
                .into()
        )
    );
    // The other builds do not read it.
    ok(&file, "worker", RELEASE, &[]);
    ok(&file, "test", RELEASE, &[]);
    let set = self::file(&source, &[("APP_VERSION", "2.4.0")]);
    let release = ok(&set, "app", RELEASE, &[]);
    assert_eq!(
        shown(&release, Setting::ClosureDefines),
        pair(
            "{app.config/DEBUG false app.config/VERSION \"2.4.0\"}",
            "light-cljs.edn:14:31 (:release)"
        )
    );
    let ValueKind::Map(defines) = &release.get(Setting::ClosureDefines).unwrap().kind else {
        panic!("a map")
    };
    let (_, version) = defines
        .iter()
        .find(|(key, _)| key.to_edn() == "app.config/VERSION")
        .unwrap();
    assert_eq!(
        set.origin(version, &[]).to_string(),
        "light-cljs.edn:15:51, from APP_VERSION"
    );
}

/// A one-line file whose build `:app` has `:target` and `:modules`, and
/// `build` at 1:91.
fn line(top: &str, build: &str) -> String {
    format!(
        "{{:source-paths [\"src\"] :builds {{:app {{:target :esm :modules {{:main {{:entries \
         [app.main]}}}} {build}}}}}{top}}}"
    )
}

#[test]
fn required_values_from_unset_variables_are_errors_in_the_modes_that_read_them() {
    let rows: [(&str, String, &str, &str); 13] = [
        (
            "an unset :output-dir with nothing under it",
            line("", ":output-dir #light-cljs/env \"OUT\""),
            "light-cljs.edn:1:103: :output-dir of build :app (dev) is nil: OUT is unset and \
             the form gives no default",
            "light-cljs.edn:1:103: :output-dir of build :app (release) is nil: OUT is unset and \
             the form gives no default",
        ),
        (
            "an unset :output-dir over the build's",
            line(
                "",
                ":output-dir \"out\" :release {:output-dir #light-cljs/env \"OUT\"}",
            ),
            "no error",
            "no error",
        ),
        (
            "a source path",
            "{:source-paths [\"src\" #light-cljs/env \"EXTRA\"] :builds {:app {:target :esm \
             :output-dir \"out\" :modules {:main {:entries [app.main]}}}}}"
                .into(),
            "light-cljs.edn:1:23: a source path of build :app (dev) is nil: EXTRA is unset and \
             the form gives no default",
            "light-cljs.edn:1:23: a source path of build :app (release) is nil: EXTRA is unset \
             and the form gives no default",
        ),
        (
            "a module's entry",
            "{:source-paths [\"src\"] :builds {:app {:target :esm :output-dir \"out\" :modules \
             {:main {:entries [#light-cljs/env \"ENTRY\"]}}}}}"
                .into(),
            "light-cljs.edn:1:97: the entry of module :main of build :app (dev) is nil: ENTRY \
             is unset and the form gives no default",
            "light-cljs.edn:1:97: the entry of module :main of build :app (release) is nil: \
             ENTRY is unset and the form gives no default",
        ),
        (
            "a module",
            "{:source-paths [\"src\"] :builds {:app {:target :esm :output-dir \"out\" :modules \
             {:main #light-cljs/env \"MODULE\"}}}}"
                .into(),
            "light-cljs.edn:1:86: module :main of build :app (dev) is nil: MODULE is unset and \
             the form gives no default",
            "light-cljs.edn:1:86: module :main of build :app (release) is nil: MODULE is unset \
             and the form gives no default",
        ),
        (
            "a module's :entries",
            "{:source-paths [\"src\"] :builds {:app {:target :esm :output-dir \"out\" :modules \
             {:main {:entries #light-cljs/env \"ENTRIES\"}}}}}"
                .into(),
            "light-cljs.edn:1:96: :entries of module :main of build :app (dev) is nil: ENTRIES \
             is unset and the form gives no default",
            "light-cljs.edn:1:96: :entries of module :main of build :app (release) is nil: \
             ENTRIES is unset and the form gives no default",
        ),
        (
            "an unset entry in :release beside the build's, no second namespace",
            line(
                "",
                ":output-dir \"out\" :release {:modules {:main {:entries [#light-cljs/env \
                 \"ENTRY\"]}}}",
            ),
            "no error",
            "light-cljs.edn:1:146: the entry of module :main of build :app (release) is nil: \
             ENTRY is unset and the form gives no default",
        ),
        (
            "an unset module and :entries over the build's",
            line(
                "",
                ":output-dir \"out\" :dev {:modules {:main {:entries #light-cljs/env \
                 \"ENTRIES\"}}} :release {:modules {:main #light-cljs/env \"MODULE\"}}",
            ),
            "no error",
            "no error",
        ),
        (
            "a top-level cache blocker",
            line(
                " :cache-blockers #{#light-cljs/env \"BLOCKER\"}",
                ":output-dir \"out\"",
            ),
            "light-cljs.edn:1:129: a cache blocker of build :app (dev) is nil: BLOCKER is unset \
             and the form gives no default",
            "light-cljs.edn:1:129: a cache blocker of build :app (release) is nil: BLOCKER is \
             unset and the form gives no default",
        ),
        (
            "the nREPL port, which only the watch reads",
            line(
                " :nrepl {:port #light-cljs/env \"NREPL_PORT\"}",
                ":output-dir \"out\"",
            ),
            "light-cljs.edn:1:125: :nrepl :port of build :app (dev) is nil: NREPL_PORT is unset \
             and the form gives no default",
            "no error",
        ),
        (
            "a release define",
            line(
                "",
                ":output-dir \"out\" :release {:compiler-options {:closure-defines {app.config/V \
                 #light-cljs/env \"V\"}}}",
            ),
            "no error",
            "light-cljs.edn:1:169: :closure-defines app.config/V of build :app (release) is nil: \
             V is unset and the form gives no default",
        ),
        (
            "a :config value, which takes nil",
            line(
                "",
                ":output-dir \"out\" :config {:app/x #light-cljs/env \"X\"}",
            ),
            "no error",
            "no error",
        ),
        (
            "an optional setting, then not given",
            line(
                " :http {:port #light-cljs/env \"PORT\"}",
                ":output-dir \"out\" :release {:compiler-options {:checked-arrays \
                 #light-cljs/env \"CHECKED\"}}",
            ),
            "no error",
            "no error",
        ),
    ];
    for (case, source, in_dev, in_release) in rows {
        let file = file(&source, &[]);
        for (mode, expected) in [(DEV, in_dev), (RELEASE, in_release)] {
            let message = match settings(&file, "app", mode, &[]) {
                Ok(_) => "no error".into(),
                Err(error) => error,
            };
            assert_eq!(message, expected, "{case} in {mode:?}");
        }
    }
    let optional = file(
        &line(
            " :http {:port #light-cljs/env \"PORT\"}",
            ":output-dir \"out\" :config {:app/x #light-cljs/env \"X\"} :release \
             {:compiler-options {:checked-arrays #light-cljs/env \"CHECKED\"}}",
        ),
        &[],
    );
    let dev = ok(&optional, "app", DEV, &[]);
    assert_eq!(shown(&dev, Setting::HttpPort), pair("9630", "default"));
    assert_eq!(
        shown(&dev, Setting::Config),
        pair("{:app/x nil}", "light-cljs.edn:1:117")
    );
    let release = ok(&optional, "app", RELEASE, &[]);
    assert_eq!(shown(&release, Setting::CheckedArrays), None);
}

// ---------------------------------------------------------------------------
// Output directories

#[test]
fn two_builds_may_not_write_one_directory_in_a_mode() {
    let source = "{:source-paths [\"src\"]
 :builds
 {:app {:target :esm :output-dir \"out/app\" :modules {:main {:entries [app.main]}}
        :dev {:output-dir \"out/dev\"}}
  :worker {:target :esm :output-dir \"out/worker\" :modules {:main {:entries [app.worker]}}
           :dev {:output-dir \"./out/dev/\"}}}}";
    let file = file(source, &[]);
    assert_eq!(
        settings(&file, "app", DEV, &[]).map(|_| ()),
        Err("light-cljs.edn:4:27: builds :app and :worker both write out/dev in dev mode".into())
    );
    assert_eq!(
        settings(&file, "worker", DEV, &[]).map(|_| ()),
        Err(
            "light-cljs.edn:6:30: builds :app and :worker both write ./out/dev/ in dev mode".into()
        )
    );
    // Only dev collides.
    ok(&file, "app", RELEASE, &[]);
    ok(&file, "worker", RELEASE, &[]);

    // A merge that sends one build into another's directory collides too.
    let into_worker = [merge(1, "{:output-dir \"out/worker\"}", &[])];
    assert_eq!(
        settings(&file, "app", RELEASE, &into_worker).map(|_| ()),
        Err(
            "--config-merge 1:1:14: builds :app and :worker both write out/worker in release \
             mode"
                .into()
        )
    );
    // A merge given to two builds of one command: each is checked against the
    // file's other builds by `settings`, and against each other here.
    let shared = [merge(1, "{:output-dir \"out/x\"}", &[])];
    let both = [
        ok(&file, "app", RELEASE, &shared),
        ok(&file, "worker", RELEASE, &shared),
    ];
    assert_eq!(
        file.check_outputs(&both, &shared)
            .map_err(|error| error.to_string()),
        Err(
            "--config-merge 1:1:14: builds :app and :worker both write out/x in release mode"
                .into()
        )
    );
    let apart = [
        ok(&file, "app", RELEASE, &[]),
        ok(&file, "worker", RELEASE, &[]),
    ];
    assert_eq!(file.check_outputs(&apart, &[]), Ok(()));
}

#[test]
fn modules_are_checked_in_every_build_and_mode_as_the_file_is_read() {
    let worker = "{:source-paths [\"src\"]
 :builds
 {:app {:target :esm :output-dir \"out/app\" :modules {:main {:entries [app.main]}}}
  :worker {:target :esm :output-dir \"out/worker\" :modules {:main {:entries [worker.main]}}}}}";
    let rows = [
        (
            worker.to_owned(),
            "{:builds {:worker {:modules {:main {:entries [worker.other]}}}}}",
            format!(
                "LIGHT_CLJS:1:47: module :main of build :worker (dev) has two entries; give one \
                 namespace: {CODE_SPLITTING}"
            ),
        ),
        (
            line(
                "",
                ":output-dir \"out\" :release {:modules {:main {:entries [app.other]}}}",
            ),
            "",
            format!(
                "light-cljs.edn:1:146: module :main of build :app (release) has two entries; \
                 give one namespace: {CODE_SPLITTING}"
            ),
        ),
        (
            line(
                "",
                ":output-dir \"out\" :dev {:modules {:other {:entries [app.other]}}}",
            ),
            "",
            format!("light-cljs.edn:1:125: build :app (dev) has two modules; {CODE_SPLITTING}"),
        ),
    ];
    for (source, light, expected) in rows {
        let message = match read(&source, &[("LIGHT_CLJS", light)]) {
            Ok(_) => "no error".into(),
            Err(error) => error.to_string(),
        };
        assert_eq!(message, expected, "{source} {light}");
    }
}

#[test]
fn directories_compare_alike_however_they_are_spelled() {
    // The file is named relatively, so relative paths are the current
    // directory's, and some paths are written absolute.
    let cwd = std::env::current_dir().unwrap();
    let cwd = cwd.to_str().unwrap();
    let source = format!(
        "{{:source-paths [\"{cwd}/src\"]
 :cache-root \"{cwd}/state\"
 :builds
 {{:app {{:target :esm :modules {{:main {{:entries [app.main]}}}}
        :output-dir \"src\"
        :release {{:output-dir \"state/cache\"}}}}
  :worker {{:target :esm :modules {{:main {{:entries [app.worker]}}}}
           :output-dir \"{cwd}/out/worker\"}}}}}}"
    );
    let file = file(&source, &[]);
    assert_eq!(
        settings(&file, "app", DEV, &[]).map(|_| ()),
        Err(format!(
            "light-cljs.edn:5:21: build :app writes src in dev mode, the source path \
             \"{cwd}/src\"; give it a directory of its own"
        ))
    );
    assert_eq!(
        settings(&file, "app", RELEASE, &[]).map(|_| ()),
        Err(format!(
            "light-cljs.edn:6:31: build :app writes state/cache in release mode, the cache \
             store of :cache-root \"{cwd}/state\"; give it a directory of its own"
        ))
    );
    let merges = [merge(1, "{:output-dir \"out/worker\"}", &[])];
    assert_eq!(
        settings(&file, "app", RELEASE, &merges).map(|_| ()),
        Err(
            "--config-merge 1:1:14: builds :app and :worker both write out/worker in release \
             mode"
                .into()
        )
    );
    let spelled = [
        merge(1, &format!("{{:output-dir \"{cwd}/x\"}}"), &[]),
        merge(2, "{:output-dir \"./out/../x/\"}", &[]),
    ];
    let both = [
        ok(&file, "app", RELEASE, &spelled[..1]),
        ok(&file, "worker", RELEASE, &spelled[1..]),
    ];
    assert_eq!(
        file.check_outputs(&both, &spelled)
            .map_err(|error| error.to_string()),
        Err(
            "--config-merge 2:1:14: builds :app and :worker both write ./out/../x/ in release \
             mode"
                .into()
        )
    );
}

#[test]
fn an_output_directory_may_not_be_the_cache_store_or_a_source_path() {
    let rows = [
        (
            line(" :cache-root \"out\"", ":output-dir \"out/cache\""),
            "light-cljs.edn:1:103: build :app writes out/cache in dev mode, the cache store of \
             :cache-root \"out\"; give it a directory of its own",
        ),
        (
            line("", ":output-dir \".light-cljs/./cache\""),
            "light-cljs.edn:1:103: build :app writes .light-cljs/./cache in dev mode, the cache \
             store of :cache-root \".light-cljs\"; give it a directory of its own",
        ),
        (
            line("", ":output-dir \"lib/../src/\""),
            "light-cljs.edn:1:103: build :app writes lib/../src/ in dev mode, the source path \
             \"src\"; give it a directory of its own",
        ),
        (line("", ":output-dir \"src/out\""), "no error"),
        (
            line(" :cache-root \"out\"", ":output-dir \"out\""),
            "no error",
        ),
    ];
    for (source, expected) in rows {
        let file = file(&source, &[]);
        let message = match settings(&file, "app", DEV, &[]) {
            Ok(_) => "no error".into(),
            Err(error) => error,
        };
        assert_eq!(message, expected, "{source}");
    }
}

// ---------------------------------------------------------------------------
// The features example

#[test]
fn the_features_example_merges_its_modes_and_a_config_merge() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("features")
        .join(FILE_NAME);
    let file = ConfigFile::read(&path, &|_| None).unwrap_or_else(|error| panic!("{error}"));
    let dev = ok(&file, "features", DEV, &[]);
    assert_eq!(
        shown(&dev, Setting::OutputDir),
        pair("\"out/dev\"", "light-cljs.edn:13:22 (:dev)")
    );
    let release = ok(&file, "features", RELEASE, &[]);
    assert_eq!(
        shown(&release, Setting::OutputDir),
        pair("\"out/release\"", "light-cljs.edn:11:16")
    );
    assert_eq!(
        shown(&release, Setting::Optimizations),
        pair(":advanced", "default")
    );
    assert_eq!(
        shown(&release, Setting::SourcePaths),
        pair("[\".\"]", "light-cljs.edn:7:16")
    );
    let merges = [ConfigMerge::parse(
        1,
        "{:output-dir \"out/x\"}",
        path.parent().unwrap(),
        &|_| None,
    )
    .unwrap()];
    for mode in [DEV, RELEASE] {
        assert_eq!(
            shown(&ok(&file, "features", mode, &merges), Setting::OutputDir),
            pair("\"out/x\"", "--config-merge 1:1:14"),
            "{mode:?}"
        );
    }
}

/// `BuildSettings::changed`, which a watch's reread compares with
/// (docs/build-configuration.md, "Watching the configuration file"): slot by
/// slot, by value alone. Comments, layout, where a value was written and
/// whether a variable gave it change nothing, and neither does another
/// build's section; what a build can observe does, the number as written, a
/// list for a vector and the order of a map's entries among it. Each changed
/// setting is listed once, in slot order, either way round.
#[test]
fn changed_compares_the_build_settings_slot_by_slot_by_value() {
    let base = "{:source-paths [\"src\"] :http {:port 9630}\n\
                :builds {:app {:target :esm :output-dir \"out/app\"\n\
                :modules {:main {:entries [app.main]}}\n\
                :config {:app/a 1 :app/b \"x\" :app/c [1 2]}}\n\
                :other {:target :esm :output-dir \"out/other\" \
                :modules {:main {:entries [app.other]}}}}}";
    let running = ok(&file(base, &[]), "app", DEV, &[]);
    /// A case's name, the edited file, its environment and what changed.
    type Case<'a> = (&'a str, String, &'a [(&'a str, &'a str)], &'a [Setting]);
    let cases: Vec<Case> = vec![
        ("a comment", format!(";; a comment\n{base}"), &[], &[]),
        (
            "layout",
            base.replace(":app/a 1 :app/b", ":app/a 1\n   :app/b")
                .replace("{:port 9630}", "{ :port   9630 }"),
            &[],
            &[],
        ),
        (
            "the order of a map",
            base.replace(":app/a 1 :app/b \"x\"", ":app/b \"x\" :app/a 1"),
            &[],
            &[Setting::Config],
        ),
        (
            "a float for an integer",
            base.replace(":app/a 1", ":app/a 1.0"),
            &[],
            &[Setting::Config],
        ),
        (
            "a list for a vector",
            base.replace("[1 2]", "(1 2)"),
            &[],
            &[Setting::Config],
        ),
        (
            "a variable",
            base.replace("\"x\" :app/c", "#light-cljs/env \"B\" :app/c"),
            &[("B", "x")],
            &[],
        ),
        (
            "another build",
            base.replace("\"out/other\"", "\"out/other2\""),
            &[],
            &[],
        ),
        (
            "the config",
            base.replace(":app/a 1", ":app/a 2"),
            &[],
            &[Setting::Config],
        ),
        (
            "the port",
            base.replace("9630", "9631"),
            &[],
            &[Setting::HttpPort],
        ),
        (
            "the source paths and the output",
            base.replace("[\"src\"]", "[\"src\" \"lib\"]")
                .replace("\"out/app\"", "\"out/app2\""),
            &[],
            &[Setting::SourcePaths, Setting::OutputDir],
        ),
        (
            "the dev mode",
            base.replace(
                ":config {",
                ":dev {:compiler-options {:elide-asserts true}} :config {",
            ),
            &[],
            &[Setting::ElideAsserts],
        ),
    ];
    for (name, edited, environment, expected) in cases {
        let next = ok(&file(&edited, environment), "app", DEV, &[]);
        assert_eq!(running.changed(&next), expected, "{name}");
        assert_eq!(next.changed(&running), expected, "{name}, the other way");
    }
}
