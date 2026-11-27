//! Compiler-owned bundled libraries are exempt from `--macro-expansions` and
//! `--macro-nodes` while staying subject to nesting depth, deadline, memory,
//! and poll limits; `--stats` still counts every executed expansion, and user
//! namespaces (including explicit source-root overrides of bundled library
//! names) remain charged.
use crate::temp_dir::TempDir;
use light_cljs::{
    CompileStats, MacroHost, MacroLimits,
    analyzer::AnalysisOptions,
    compiler::{EntryRecorders, analyze_namespace_entry_recorded},
    dev_session::{Action, DevSession},
    eval_session::{EvalOptions, EvalSession},
    source_provider::{FileSourceProvider, SourceInput},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    rc::Rc,
};

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn light_cljs(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn parse_stat(stderr: &str, key: &str) -> usize {
    stderr
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(' ')?;
            (k == key).then(|| v.parse::<usize>().ok()).flatten()
        })
        .unwrap_or_else(|| panic!("missing stat {key} in:\n{stderr}"))
}

fn parse_ns_macro_calls(stderr: &str, ns: &str) -> Option<usize> {
    let (_, table) = stderr.split_once("[macro-calls-by-namespace]\n")?;
    table.lines().find_map(|line| {
        let (name, count) = line.split_once(' ')?;
        (name == ns).then(|| count.parse::<usize>().ok()).flatten()
    })
}

const DEFTEST_MAIN: &str = r#"(ns app.main
  (:require [cljs.test :as t :refer [deftest is]]))

(enable-console-print!)

(deftest arithmetic
  (is (= 2 (+ 1 1))))

(t/test-vars [#'arithmetic])
"#;

#[test]
fn cljs_test_with_deftest_compiles_under_tight_limits_uncached_and_cached_at_one_and_four_jobs() {
    let dir = TempDir::new("bundled-limits-deftest");
    let root = dir.join("src");
    write(&root, "app/main.cljs", DEFTEST_MAIN);
    let root_str = root.to_str().unwrap();

    // Uncached builds at --jobs 1 and --jobs 4 with --macro-expansions 20 and
    // tight --macro-nodes (far below cljs.test's own ~273 expansions / tens of
    // thousands of nodes).
    let mut uncached_outputs = Vec::new();
    let mut uncached_stats = Vec::new();
    for jobs in ["1", "4"] {
        let output = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_str,
                "--no-cache",
                "--jobs",
                jobs,
                "--macro-expansions",
                "20",
                "--macro-nodes",
                "5000",
                "--stats",
            ],
        );
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            output.status.success(),
            "uncached --jobs {jobs} failed:\n{stderr}"
        );
        let js = String::from_utf8(output.stdout).unwrap();
        let printed = crate::canonical_json::node_stdout(&js);
        assert!(
            printed.is_empty(),
            "expected passing test output, got: {printed:?}"
        );

        // --stats stays truthful: bundled expansions are counted in both
        // macro-calls and [macro-calls-by-namespace].
        let total = parse_stat(&stderr, "macro-calls");
        let peak_nodes = parse_stat(&stderr, "macro-nodes.peak");
        let cljs_test_calls = parse_ns_macro_calls(&stderr, "cljs.test")
            .unwrap_or_else(|| panic!("cljs.test missing from stats table:\n{stderr}"));
        let app_calls = parse_ns_macro_calls(&stderr, "app.main")
            .unwrap_or_else(|| panic!("app.main missing from stats table:\n{stderr}"));
        assert!(
            cljs_test_calls > 100,
            "expected cljs.test to report > 100 macro calls, got {cljs_test_calls}"
        );
        assert!(
            app_calls > 0 && app_calls <= 20,
            "expected app.main to report 1..=20 macro calls, got {app_calls}"
        );
        assert!(total >= cljs_test_calls + app_calls);
        assert!(peak_nodes > 5000);
        uncached_outputs.push(js);
        uncached_stats.push((total, peak_nodes, cljs_test_calls, app_calls));
    }
    assert_eq!(uncached_outputs[0], uncached_outputs[1]);
    assert_eq!(uncached_stats[0], uncached_stats[1]);

    // Populate persistent cache with --macro-expansions 20 --macro-nodes 5000,
    // then build from cache at --jobs 1 and --jobs 4 (both partial hit when
    // app.main changes, and full warm hit).
    let cache = dir.join("cache");
    let cache_str = cache.to_str().unwrap();
    let cold = light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root_str,
            "--cache-dir",
            cache_str,
            "--macro-expansions",
            "20",
            "--macro-nodes",
            "5000",
        ],
    );
    assert!(
        cold.status.success(),
        "cold cache build failed:\n{}",
        String::from_utf8_lossy(&cold.stderr)
    );

    // Edit app.main per job count so cljs.test (and its bundled dependencies)
    // are installed from the cache while app.main itself compiles fresh under
    // the tight limit, proving installed bundled artifacts charge nothing.
    for jobs in ["1", "4"] {
        let edited = format!("{DEFTEST_MAIN}; edited-jobs-{jobs}\n");
        write(&root, "app/main.cljs", &edited);
        let warm = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_str,
                "--cache-dir",
                cache_str,
                "--jobs",
                jobs,
                "--macro-expansions",
                "20",
                "--macro-nodes",
                "5000",
                "--stats",
            ],
        );
        let stderr = String::from_utf8_lossy(&warm.stderr).into_owned();
        assert!(
            warm.status.success(),
            "warm cached --jobs {jobs} failed:\n{stderr}"
        );
        assert!(
            parse_stat(&stderr, "cache.hits") >= 4,
            "expected bundled namespaces to hit cache:\n{stderr}"
        );
        // cljs.test was installed from cache without running its macros.
        assert_eq!(
            parse_ns_macro_calls(&stderr, "cljs.test"),
            Some(0),
            "expected cljs.test to be a cache hit:\n{stderr}"
        );
        // Full warm hit on second pass also succeeds under the tight limit.
        let warm_all = light_cljs(
            dir.path(),
            &[
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_str,
                "--cache-dir",
                cache_str,
                "--jobs",
                jobs,
                "--macro-expansions",
                "20",
                "--macro-nodes",
                "5000",
                "--stats",
            ],
        );
        let stderr_all = String::from_utf8_lossy(&warm_all.stderr).into_owned();
        assert!(
            warm_all.status.success(),
            "full warm cached --jobs {jobs} failed:\n{stderr_all}"
        );
        assert_eq!(parse_stat(&stderr_all, "macro-calls"), 0);
    }
}

#[test]
fn user_namespace_exceeding_macro_limit_fails_at_users_call() {
    let dir = TempDir::new("bundled-limits-user-fail");
    let root = dir.join("src");
    let mut source = String::from(
        "(ns app.main (:require [cljs.test :refer [deftest is]]))\n\
         (deftest ok (is (= 1 1)))\n",
    );
    for i in 0..25 {
        source.push_str(&format!("(def v{i} (when true {i}))\n"));
    }
    source.push_str("v24\n");
    write(&root, "app/main.cljs", &source);
    let root_str = root.to_str().unwrap();

    // Warm the cache with cljs.test at --macro-expansions 20 using a small
    // app.main first, then overwrite app.main with the over-budget source so
    // cljs.test is a real cache hit when app.main fails.
    let cache = dir.join("cache");
    let cache_str = cache.to_str().unwrap();
    write(&root, "app/main.cljs", DEFTEST_MAIN);
    let warm = light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root_str,
            "--cache-dir",
            cache_str,
            "--macro-expansions",
            "20",
        ],
    );
    assert!(
        warm.status.success(),
        "{}",
        String::from_utf8_lossy(&warm.stderr)
    );

    // Overwrite app.main with the 25-expansion source so cljs.test hits cache
    // while app.main compiles fresh and crosses the 20-expansion budget.
    write(&root, "app/main.cljs", &source);

    for (mode_name, cache_args) in [
        ("uncached", vec!["--no-cache"]),
        ("cached", vec!["--cache-dir", cache_str]),
    ] {
        for jobs in ["1", "4"] {
            let mut args = vec![
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                root_str,
                "--jobs",
                jobs,
                "--macro-expansions",
                "20",
            ];
            args.extend_from_slice(&cache_args);
            let output = light_cljs(dir.path(), &args);
            assert!(
                !output.status.success(),
                "{mode_name} --jobs {jobs} unexpectedly succeeded"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("compiler macro expansion limit exceeded"),
                "{mode_name} --jobs {jobs} wrong message:\n{stderr}"
            );
            assert!(
                stderr.contains("app/main.cljs"),
                "{mode_name} --jobs {jobs} should report failure at user's call:\n{stderr}"
            );
            assert!(
                !stderr.contains("<first-party>"),
                "{mode_name} --jobs {jobs} should not blame bundled library:\n{stderr}"
            );
        }
    }

    // Node budget is also still enforced on the user namespace.
    let node_fail = light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            root_str,
            "--no-cache",
            "--macro-nodes",
            "50",
        ],
    );
    assert!(!node_fail.status.success());
    let stderr = String::from_utf8_lossy(&node_fail.stderr);
    assert!(
        stderr.contains("compiler macro generated-node limit exceeded")
            || stderr.contains("compiler input conversion limit")
            || stderr.contains("compiler output conversion limit"),
        "unexpected node-limit error:\n{stderr}"
    );
    assert!(stderr.contains("app/main.cljs"), "{stderr}");
}

#[test]
fn user_source_root_overriding_clojure_template_is_charged() {
    let dir = TempDir::new("bundled-limits-override");
    let bundled_root = dir.join("bundled-only");
    write(
        &bundled_root,
        "app/main.cljs",
        "(ns app.main (:require [clojure.template :as tmpl]))\n\
         (tmpl/do-template [x] (def a x) 1)\n\
         a\n",
    );

    // With the compiler-owned bundled clojure.template, 5 expansions is plenty
    // for app.main's single do-template call.
    let ok = light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            bundled_root.to_str().unwrap(),
            "--no-cache",
            "--macro-expansions",
            "5",
        ],
    );
    assert!(
        ok.status.success(),
        "bundled clojure.template should be exempt:\n{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    // Now supply clojure.template from an explicit source root with 10 macro
    // expansions inside it: it is user source and must be charged.
    let override_root = dir.join("override");
    write(
        &override_root,
        "app/main.cljs",
        "(ns app.main (:require [clojure.template :as tmpl]))\n\
         (tmpl/value)\n",
    );
    let mut override_template = String::from("(ns clojure.template)\n");
    for i in 0..10 {
        override_template.push_str(&format!("(def t{i} (when true {i}))\n"));
    }
    override_template.push_str("(defn value [] t9)\n");
    write(&override_root, "clojure/template.cljs", &override_template);

    let cache = dir.join("override-cache");
    // Warm the cache at default limits first.
    let warm = light_cljs(
        dir.path(),
        &[
            "--compile",
            "--entry",
            "app.main",
            "--source-root",
            override_root.to_str().unwrap(),
            "--cache-dir",
            cache.to_str().unwrap(),
        ],
    );
    assert!(
        warm.status.success(),
        "{}",
        String::from_utf8_lossy(&warm.stderr)
    );

    for (mode, extra) in [
        ("uncached", vec!["--no-cache"]),
        ("cached", vec!["--cache-dir", cache.to_str().unwrap()]),
    ] {
        for jobs in ["1", "4"] {
            let mut args = vec![
                "--compile",
                "--entry",
                "app.main",
                "--source-root",
                override_root.to_str().unwrap(),
                "--jobs",
                jobs,
                "--macro-expansions",
                "5",
            ];
            args.extend_from_slice(&extra);
            let fail = light_cljs(dir.path(), &args);
            assert!(
                !fail.status.success(),
                "overridden clojure.template ({mode}, --jobs {jobs}) should be charged and fail"
            );
            let stderr = String::from_utf8_lossy(&fail.stderr);
            assert!(
                stderr.contains("compiler macro expansion limit exceeded"),
                "{mode} --jobs {jobs}:\n{stderr}"
            );
            assert!(
                stderr.contains("clojure/template.cljs"),
                "{mode} --jobs {jobs} should report failure in user override:\n{stderr}"
            );
        }
    }
}

#[test]
fn bundled_libraries_stay_subject_to_depth_limit() {
    let dir = TempDir::new("bundled-limits-depth");
    let root = dir.join("src");
    write(&root, "app/main.cljs", DEFTEST_MAIN);
    let roots = vec![PathBuf::from(&root)];
    let mut host = MacroHost::new();
    host.limits.expansion_depth = 1;
    let err = analyze_namespace_entry_recorded(
        "app.main",
        &roots,
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders::default(),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("depth"),
        "expected depth limit error, got: {err}"
    );
}

#[test]
fn captured_bundled_artifact_has_zero_expansion_and_node_charges_and_keeps_depth() {
    let dir = TempDir::new("bundled-limits-artifact");
    let root = dir.join("src");
    write(&root, "app/main.cljs", DEFTEST_MAIN);
    let roots = vec![PathBuf::from(&root)];

    let mut captured = std::collections::HashMap::new();
    let mut capture_sink = |cap: light_cljs::namespace_artifact::NamespaceCapture| {
        if let Ok(artifact) = cap.artifact {
            captured.insert(cap.namespace, artifact);
        }
    };
    let mut default_host = MacroHost::new();
    analyze_namespace_entry_recorded(
        "app.main",
        &roots,
        &mut default_host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            capture: Some(&mut capture_sink),
            ..EntryRecorders::default()
        },
    )
    .unwrap();

    let cljs_test = captured
        .get("cljs.test")
        .expect("cljs.test artifact captured");
    let accounting = cljs_test.accounting.expect("cljs.test accounting");
    assert_eq!(accounting.expansions, 0);
    assert_eq!(accounting.generated_nodes, 0);
    assert_eq!(accounting.node_peak, 0);
    assert_eq!(accounting.conversion_peak, 0);
    assert!(
        accounting.macro_depth > 1,
        "expected bundled library macro_depth to remain recorded: {accounting:?}"
    );
    if let Some(bodies) = &cljs_test.bodies {
        assert!(
            bodies.charges.is_empty(),
            "expected no charged body expansions for bundled library, got {}",
            bodies.charges.len()
        );
    }

    // Installing the default-captured bundled artifacts into a tight-budget
    // compilation succeeds and charges nothing to the user namespace's budget.
    let mut tight_host = MacroHost::new();
    tight_host.limits = MacroLimits {
        expansions: 20,
        generated_nodes: 5_000,
        ..MacroLimits::default()
    };
    let mut stats = CompileStats::new();
    {
        let lookup = |ns: &str| (ns != "app.main").then(|| captured.get(ns)).flatten();
        analyze_namespace_entry_recorded(
            "app.main",
            &roots,
            &mut tight_host,
            AnalysisOptions::default(),
            &mut Vec::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                install: Some(&lookup),
                ..EntryRecorders::default()
            },
        )
        .unwrap();
    }
    let install = stats.install.as_ref().expect("install stats");
    assert!(
        install.installed.iter().any(|ns| ns == "cljs.test"),
        "expected cljs.test to install from artifact: {install:?}"
    );
    assert_eq!(stats.macro_calls_in("cljs.test"), Some(0));

    // The same artifact is refused when the build's depth limit is too low.
    let mut shallow_host = MacroHost::new();
    shallow_host.limits.expansion_depth = 1;
    let lookup = |ns: &str| (ns != "app.main").then(|| captured.get(ns)).flatten();
    let err = analyze_namespace_entry_recorded(
        "app.main",
        &roots,
        &mut shallow_host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            install: Some(&lookup),
            ..EntryRecorders::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("depth"),
        "expected depth limit error after refusing bundled artifact, got: {err}"
    );
}

#[test]
fn eval_session_exempts_bundled_libraries_and_charges_user_and_overrides() {
    let options = EvalOptions {
        compiler: MacroLimits {
            expansions: 20,
            generated_nodes: 5_000,
            ..MacroLimits::default()
        },
        ..EvalOptions::default()
    };
    let mut session = EvalSession::new(options).unwrap();
    let result = session
        .eval_string(
            "(ns app.eval (:require [cljs.test :as t :refer [deftest is]]))\n\
             (deftest ok (is (= 2 (+ 1 1))))\n\
             :ok",
        )
        .unwrap();
    assert_eq!(session.print(result.value).unwrap(), ":ok");

    // User code exceeding the expansion limit in the same session still fails.
    let mut too_many = String::new();
    for i in 0..25 {
        too_many.push_str(&format!("(when true {i})\n"));
    }
    let err = session.eval_string(&too_many).unwrap_err();
    assert!(
        err.message
            .contains("compiler macro expansion limit exceeded"),
        "unexpected error: {err}"
    );

    // An explicit source root overriding clojure.template is charged in EvalSession.
    let dir = TempDir::new("bundled-limits-eval-override");
    let root = dir.join("src");
    let mut override_template = String::from("(ns clojure.template)\n");
    for i in 0..10 {
        override_template.push_str(&format!("(def t{i} (when true {i}))\n"));
    }
    write(&root, "clojure/template.cljs", &override_template);
    let mut override_session = EvalSession::new(EvalOptions {
        compiler: MacroLimits {
            expansions: 5,
            generated_nodes: 5_000,
            ..MacroLimits::default()
        },
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![root]))),
        ..EvalOptions::default()
    })
    .unwrap();
    let err = override_session
        .eval_string("(require 'clojure.template)")
        .unwrap_err();
    assert!(
        err.message
            .contains("compiler macro expansion limit exceeded"),
        "overridden clojure.template should be charged in EvalSession: {err}"
    );
}

#[test]
fn dev_session_watch_step_exempts_newly_required_bundled_library() {
    let dir = TempDir::new("bundled-limits-watch");
    let root = dir.join("src");
    write(&root, "app/main.cljs", "(ns app.main)\n(def x 1)\n");

    let mut host = MacroHost::new();
    host.limits = MacroLimits {
        expansions: 20,
        generated_nodes: 5_000,
        ..MacroLimits::default()
    };
    let mut stats = CompileStats::new();
    let _ = analyze_namespace_entry_recorded(
        "app.main",
        std::slice::from_ref(&root),
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
        EntryRecorders {
            stats: Some(&mut stats),
            ..EntryRecorders::default()
        },
    )
    .unwrap();

    // Simulate a watch step reloading app.main after the user adds a require of
    // bundled cljs.test and a deftest, under a 20-expansion budget.
    let session = DevSession::new(&EvalOptions {
        compiler: MacroLimits {
            expansions: 20,
            generated_nodes: 5_000,
            ..MacroLimits::default()
        },
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![root.clone()]))),
        ..EvalOptions::default()
    })
    .unwrap();
    session.reset_expansion_budget();
    let actions = session.load_namespace_file(
        "app.main",
        &SourceInput::new(root.join("app/main.cljs").to_string_lossy(), DEFTEST_MAIN),
    );
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Error(_))),
        "watch step loading bundled cljs.test should succeed: {actions:?}"
    );

    // Reloading app.main with > 20 user macro calls fails the watch step.
    let mut too_many = String::from("(ns app.main (:require [cljs.test :refer [deftest is]]))\n");
    for i in 0..25 {
        too_many.push_str(&format!("(def v{i} (when true {i}))\n"));
    }
    session.reset_expansion_budget();
    let actions = session.load_namespace_file(
        "app.main",
        &SourceInput::new(root.join("app/main.cljs").to_string_lossy(), too_many),
    );
    let error = actions.iter().find_map(|action| match action {
        Action::Error(err) => Some(err.to_string()),
        _ => None,
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|m| m.contains("compiler macro expansion limit exceeded")),
        "expected user limit error in watch step, got: {error:?}"
    );
}
