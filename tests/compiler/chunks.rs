//! The namespace-level chunk planner, `light_cljs::chunks::plan`
//! (docs/code-splitting.md, "The model" and "Phase 2"): analysis-only cases
//! at every level, since nothing emits chunks yet. Each plan is also checked
//! against the invariants the chunked emitter relies on (`check`).
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::chunks::{ChunkId, ChunkPlan, STARTUP, plan};
use light_cljs::compiler::{analyze_namespace_entry, analyze_namespace_entry_with_provider};
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
use light_cljs::{
    ApplicationOptions, ExprId, ExprKind, MacroHost, OptimizationLevel, PreparedApplication,
    Program, prepare_application,
};
use std::path::PathBuf;

/// The program of entry `app.main` whose sources are `files`, by path.
fn analyze(files: &[(&str, &str)]) -> Program {
    analyze_entry("app.main", files)
}

fn analyze_entry(entry: &str, files: &[(&str, &str)]) -> Program {
    let provider = |request: &LoadRequest| -> Result<Option<SourceInput>, LoadError> {
        let namespace = String::from_utf16_lossy(request.namespace.name());
        let path = format!("{}.cljs", namespace.replace('.', "/").replace('-', "_"));
        Ok(files
            .iter()
            .find(|(file, _)| *file == path)
            .map(|(file, source)| SourceInput::new(*file, *source)))
    };
    analyze_namespace_entry_with_provider(
        entry,
        &provider,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{error}"))
}

/// The plan of `program` at each level, checked.
fn plans(program: &Program) -> Vec<(OptimizationLevel, ChunkPlan)> {
    OptimizationLevel::ALL
        .into_iter()
        .map(|level| {
            let prepared = prepare_application(program, ApplicationOptions::new(level));
            let plan = plan(program, &prepared);
            check(program, &prepared, &plan, level);
            (level, plan)
        })
        .collect()
}

/// The entries of the flattened root, as the emitter writes them.
fn flatten(program: &Program, id: ExprId, out: &mut Vec<ExprId>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for &item in items {
            flatten(program, item, out);
        }
    } else {
        out.push(id);
    }
}

/// What the chunked emitter relies on: the chunks partition the items of the
/// prepared program in program order, each item is in its namespace's chunk,
/// startup imports nothing, and every import precedes its importer and has a
/// signature strictly containing the importer's.
fn check(
    program: &Program,
    prepared: &PreparedApplication,
    plan: &ChunkPlan,
    level: OptimizationLevel,
) {
    let emitted = prepared.program();
    let mut top = Vec::new();
    flatten(emitted, emitted.root, &mut top);
    assert_eq!(
        plan.items.iter().map(|item| item.expr).collect::<Vec<_>>(),
        top,
        "{level}"
    );
    assert_eq!(plan.namespace_chunks.len(), program.namespaces.len());
    assert_eq!(plan.roots.len(), 1 + program.lazy_namespaces.len());
    assert_eq!(plan.global_chunks.len(), emitted.globals.len());
    assert!(plan.chunks[STARTUP.0].imports.is_empty(), "{level}");
    let mut seen = vec![false; plan.items.len()];
    for (id, chunk) in plan.chunks.iter().enumerate() {
        assert!(chunk.items.is_sorted(), "{level}: {}", chunk.name);
        for &item in &chunk.items {
            assert!(!std::mem::replace(&mut seen[item], true));
            assert_eq!(plan.items[item].chunk, ChunkId(id));
        }
        for &namespace in &chunk.namespaces {
            assert_eq!(plan.namespace_chunks[namespace], ChunkId(id));
        }
        for &root in &chunk.roots {
            assert_eq!(plan.roots[root].chunk, ChunkId(id));
        }
        for &import in &chunk.imports {
            let target = &plan.chunks[import.0].signature;
            assert!(import.0 < id, "{level}: {}", chunk.name);
            assert!(
                target.is_superset(&chunk.signature) && *target != chunk.signature,
                "{level}: {} imports {}",
                chunk.name,
                plan.chunks[import.0].name
            );
        }
        if id != STARTUP.0 {
            assert!(!chunk.signature.contains(0), "{level}: {}", chunk.name);
        }
    }
    assert!(seen.into_iter().all(|seen| seen), "{level}");
    for item in &plan.items {
        assert_eq!(item.chunk, plan.namespace_chunks[item.namespace], "{level}");
    }
    let mut names = plan.chunks.iter().map(|c| &c.name).collect::<Vec<_>>();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), plan.chunks.len(), "{level}");
}

/// Each chunk's name with its namespaces, roots (by namespace) and imports
/// (by name).
fn membership(program: &Program, plan: &ChunkPlan) -> Vec<String> {
    let namespace = |source: usize| program.namespaces[source].as_str();
    plan.chunks
        .iter()
        .map(|chunk| {
            let namespaces = chunk.namespaces.iter().map(|&s| namespace(s));
            let roots = chunk
                .roots
                .iter()
                .map(|&root| namespace(plan.roots[root].namespace));
            let imports = chunk
                .imports
                .iter()
                .map(|import| plan.chunks[import.0].name.as_str());
            format!(
                "{}: {} | roots {} | imports {}",
                chunk.name,
                namespaces.collect::<Vec<_>>().join(" "),
                roots.collect::<Vec<_>>().join(" "),
                imports.collect::<Vec<_>>().join(" ")
            )
        })
        .collect()
}

/// The namespaces of the roots in the signature of chunk `chunk`.
fn signature_roots(program: &Program, plan: &ChunkPlan, chunk: usize) -> Vec<String> {
    plan.chunks[chunk]
        .signature
        .roots()
        .map(|root| program.namespaces[plan.roots[root].namespace].clone())
        .collect()
}

/// The membership of `program`'s plans, the same at every level, and its
/// `Advanced` plan.
fn same_membership(program: &Program) -> (Vec<String>, ChunkPlan) {
    let plans = plans(program);
    let expected = membership(program, &plans[0].1);
    for (level, plan) in &plans {
        assert_eq!(membership(program, plan), expected, "{level}");
    }
    let (_, advanced) = plans
        .into_iter()
        .find(|(level, _)| level.advanced())
        .unwrap();
    (expected, advanced)
}

#[test]
fn the_features_example_plans_startup_and_one_chunk_per_feature() {
    let program = analyze_namespace_entry(
        "app.main",
        &[PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/features"
        ))],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    // Both features read app.shared, which startup requires for its menu,
    // so at namespace granularity they share nothing startup lacks: the
    // `app.editor+app.reports` chunk of the Goal comes with definition-level
    // placement (Phase 3).
    let (membership, plan) = same_membership(&program);
    assert_eq!(
        membership,
        [
            "app.main.js: app.shared light.lazy app.main | roots app.main | imports ",
            "app.editor.js: app.editor | roots app.editor | imports app.main.js",
            "app.reports.js: clojure.string app.reports | roots app.reports | imports app.main.js",
        ]
    );
    for (chunk, roots) in [
        (0, &["app.main", "app.editor", "app.reports"][..]),
        (1, &["app.editor"]),
        (2, &["app.reports"]),
    ] {
        assert_eq!(signature_roots(&program, &plan, chunk), roots);
    }
    // Each loadable site of app.main loads its feature's chunk.
    let targets = (0..program.lazy_targets.len())
        .map(|target| plan.target_chunk(&program, light_cljs::analyzer::LazyTargetId(target)))
        .collect::<Vec<_>>();
    assert_eq!(targets, [ChunkId(1), ChunkId(2)]);
    // No feature assigns a global of another chunk.
    assert!(plan.chunks.iter().all(|chunk| chunk.setters.is_empty()));
}

#[test]
fn a_helper_two_features_require_is_one_shared_chunk() {
    static DIAMOND: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (def reports (lazy/loadable app.reports/start))\n\
             (fn [] [editor reports])\n",
        ),
        (
            "app/helper.cljs",
            "(ns app.helper)\n(defn shout [s] (str s \"!\"))\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.helper :as helper]))\n\
             (defn start [] (helper/shout \"editor\"))\n",
        ),
        (
            "app/reports.cljs",
            "(ns app.reports (:require [app.helper :as helper]))\n\
             (defn start [] (helper/shout \"reports\"))\n",
        ),
    ];
    let program = analyze(DIAMOND);
    let (membership, plan) = same_membership(&program);
    assert_eq!(
        membership,
        [
            "app.main.js: light.lazy app.main | roots app.main | imports ",
            "app.editor+app.reports.js: app.helper | roots  | imports ",
            "app.editor.js: app.editor | roots app.editor | imports app.editor+app.reports.js",
            "app.reports.js: app.reports | roots app.reports | imports app.editor+app.reports.js",
        ]
    );
    assert_eq!(
        signature_roots(&program, &plan, 1),
        ["app.editor", "app.reports"]
    );
}

#[test]
fn a_feature_that_loads_a_sub_feature_is_a_chain_whose_edges_point_up() {
    static CHAIN: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (fn [] editor)\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.text :as text]))\n\
             (def spelling (light.lazy/loadable app.spell/check))\n\
             (defn start [] (text/greet))\n",
        ),
        ("app/text.cljs", "(ns app.text)\n(defn greet [] \"hi\")\n"),
        (
            "app/spell.cljs",
            "(ns app.spell (:require [app.editor :as editor] [app.text :as text]))\n\
             (defn check [] (str (text/greet) (editor/start)))\n",
        ),
    ];
    let program = analyze(CHAIN);
    let (membership, plan) = same_membership(&program);
    // The sub-feature requires the feature, so the feature's chunk is one
    // both roots reach: the sub-feature imports it, and it imports startup
    // for light.lazy.
    assert_eq!(
        membership,
        [
            "app.main.js: light.lazy app.main | roots app.main | imports ",
            "app.editor+app.spell.js: app.text app.editor | roots app.editor | imports app.main.js",
            "app.spell.js: app.spell | roots app.spell | imports app.editor+app.spell.js",
        ]
    );
    assert_eq!(
        signature_roots(&program, &plan, 1),
        ["app.editor", "app.spell"]
    );
    assert_eq!(signature_roots(&program, &plan, 2), ["app.spell"]);
}

#[test]
fn a_lazy_target_that_requires_the_loading_namespace_leaves_it_in_startup() {
    static BACK: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main\n  (:require [app.shared :as shared] [app.util :as util]))\n\
             (def editor (light.lazy/loadable app.editor/start))\n\
             (def twice (light.lazy/loadable app.util/twice))\n\
             (defn ready [] (shared/label :main))\n\
             (fn [] [editor twice])\n",
        ),
        (
            "app/shared.cljs",
            "(ns app.shared)\n(defn label [k] (str \"label \" (name k)))\n",
        ),
        (
            "app/util.cljs",
            "(ns app.util)\n(defn twice [s] (str s s))\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor\n  (:require [app.main :as main] [app.text :as text]))\n\
             (defn start [] (str (main/ready) text/greeting))\n",
        ),
        ("app/text.cljs", "(ns app.text)\n(def greeting \"hi\")\n"),
    ];
    let program = analyze(BACK);
    let (membership, plan) = same_membership(&program);
    // app.util, which the entry requires, is a resident target: its root's
    // chunk is startup.
    assert_eq!(
        membership,
        [
            "app.main.js: app.shared app.util light.lazy app.main | roots app.main app.util | imports ",
            "app.editor.js: app.text app.editor | roots app.editor | imports app.main.js",
        ]
    );
    let main = program
        .namespaces
        .iter()
        .position(|n| n == "app.main")
        .unwrap();
    assert_eq!(plan.namespace_chunks[main], STARTUP);
    assert_eq!(
        plan.namespace_signatures[main].roots().collect::<Vec<_>>(),
        [0, 1]
    );
}

#[test]
fn a_feature_assigning_a_startup_var_goes_through_the_owner_setter() {
    static SETTERS: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.state :as state]))\n\
             (def editor (light.lazy/loadable app.editor/start))\n\
             (fn [] [editor state/mode state/*depth* state/fixed])\n",
        ),
        (
            "app/state.cljs",
            "(ns app.state)\n\
             (def mode :plain)\n\
             (def ^:dynamic *depth* 0)\n\
             (def fixed 1)\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.state :as state]))\n\
             (def edits 0)\n\
             (defn start []\n\
               (set! state/mode :edit)\n\
               (set! edits (inc edits))\n\
               (binding [state/*depth* 1] [state/mode state/*depth* state/fixed edits]))\n",
        ),
    ];
    let program = analyze(SETTERS);
    for (level, plan) in plans(&program) {
        let setters = plan
            .chunks
            .iter()
            .map(|chunk| {
                chunk
                    .setters
                    .iter()
                    .map(|global| {
                        let binding = &program.globals[global.0];
                        format!("{}/{}", binding.namespace, binding.name)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        // The editor's assignment of its own `edits` stays direct.
        assert_eq!(
            setters,
            [vec!["app.state/mode", "app.state/*depth*"], vec![]],
            "{level}"
        );
    }
}

#[test]
fn a_program_without_namespace_sources_is_one_chunk() {
    let source = "(def a 1)\n(defn f [] (set! a (inc a)) a)\n(do (f) (f))\n";
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    for (level, plan) in plans(&program) {
        assert_eq!(
            membership(&program, &plan),
            ["cljs.user.js: cljs.user | roots cljs.user | imports "],
            "{level}"
        );
        assert!(plan.chunks[0].setters.is_empty(), "{level}");
    }
}

/// A read of a global is an import of the chunk owning it even without a
/// require behind it: here a feature probes a Var of a namespace with no
/// source, which startup owns.
#[test]
fn a_global_read_imports_its_owner_without_a_require() {
    static PROBE: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main)\n\
             (def editor (light.lazy/loadable app.editor/start))\n\
             (fn [] editor)\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor)\n(defn start [] (exists? missing.ns/value))\n",
        ),
    ];
    let program = analyze(PROBE);
    let editor = program
        .namespaces
        .iter()
        .position(|n| n == "app.editor")
        .unwrap();
    assert!(program.namespace_requires[editor].is_empty());
    let probe = program
        .globals
        .iter()
        .position(|global| global.namespace == "missing.ns")
        .unwrap();
    for (level, plan) in plans(&program) {
        assert_eq!(plan.global_chunks[probe], STARTUP, "{level}");
        assert_eq!(
            membership(&program, &plan),
            [
                "app.main.js: light.lazy app.main | roots app.main | imports ",
                "app.editor.js: app.editor | roots app.editor | imports app.main.js",
            ],
            "{level}"
        );
    }
}

/// A namespace may be spelled like a hashed shared chunk; the shared chunk
/// then takes another name.
#[test]
fn a_hashed_shared_name_does_not_collide_with_a_namespace() {
    let editor = "app.an-editor-feature-with-a-long-namespace-name";
    let reports = "app.a-reports-feature-with-a-long-namespace-name";
    let files = |entry: &str| {
        let path =
            |namespace: &str| format!("{}.cljs", namespace.replace('.', "/").replace('-', "_"));
        vec![
            (
                path(entry),
                format!(
                    "(ns {entry})\n\
                     (def editor (light.lazy/loadable {editor}/start))\n\
                     (def reports (light.lazy/loadable {reports}/start))\n\
                     (fn [] [editor reports])\n"
                ),
            ),
            (
                "app/helper.cljs".to_owned(),
                "(ns app.helper)\n(defn shout [s] (str s \"!\"))\n".to_owned(),
            ),
            (
                path(editor),
                format!(
                    "(ns {editor} (:require [app.helper :as helper]))\n\
                     (defn start [] (helper/shout \"editor\"))\n"
                ),
            ),
            (
                path(reports),
                format!(
                    "(ns {reports} (:require [app.helper :as helper]))\n\
                     (defn start [] (helper/shout \"reports\"))\n"
                ),
            ),
        ]
    };
    let names = |entry: &str| {
        let files = files(entry);
        let files = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect::<Vec<_>>();
        let program = analyze_entry(entry, &files);
        plans(&program)
            .into_iter()
            .map(|(_, plan)| {
                plan.chunks
                    .into_iter()
                    .map(|chunk| chunk.name)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let shared = names("app.main")[0][1].clone();
    assert!(shared.starts_with("shared-"), "{shared}");
    let stem = shared.strip_suffix(".js").unwrap();
    let editor_file = format!("{editor}.js");
    let reports_file = format!("{reports}.js");
    for names in names(stem) {
        assert_eq!(
            names,
            [
                shared.clone(),
                format!("{stem}-2.js"),
                editor_file.clone(),
                reports_file.clone()
            ]
        );
    }
}
