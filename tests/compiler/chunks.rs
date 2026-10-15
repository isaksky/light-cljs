//! The namespace-level chunk planner, `light_cljs::chunks::plan`
//! (docs/code-splitting.md, "The model" and "Phase 2"), at every level, each
//! plan checked against the invariants the chunked emitter relies on
//! (`check`); the chunk files `PreparedApplication::emit_chunks` writes,
//! run in Node against the single `--esm` file (`runs_as_chunks`); their
//! manifest, `chunks.json`; and the CLI's `--out-dir`, which writes both.
use crate::chunked_execution::{run_directory, run_modules};
use crate::json::Json;
use crate::temp_dir::TempDir;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::chunks::{ChunkId, ChunkPlan, ChunkedOutput, STARTUP, plan};
use light_cljs::compiler::{analyze_namespace_entry, analyze_namespace_entry_with_provider};
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
use light_cljs::{
    ApplicationOptions, ExprId, ExprKind, MacroHost, OptimizationLevel, PreparedApplication,
    Program, prepare_application,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
/// prepared program in program order, each pinned item is in its
/// namespace's chunk and each movable one in a chunk of a signature its
/// namespace's contains, startup imports nothing, and every import precedes
/// its importer and has a signature strictly containing the importer's.
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
    // A pinned item is in its namespace's chunk. A movable one, only at
    // `Advanced`, moves only toward its readers: into a chunk that fewer
    // roots, or the same ones, load.
    for item in &plan.items {
        let home = plan.namespace_chunks[item.namespace];
        if item.movable {
            assert!(level.advanced(), "{level}");
            assert!(
                plan.chunks[home.0]
                    .signature
                    .is_superset(&plan.chunks[item.chunk.0].signature),
                "{level}: {}",
                plan.chunks[item.chunk.0].name
            );
        } else {
            assert_eq!(item.chunk, home, "{level}");
        }
    }
    assert!(level.advanced() || !plan.moves_definitions(), "{level}");
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
    // so at namespace granularity they share nothing startup lacks.
    let namespaces = [
        "app.main.js: app.shared light.lazy app.main | roots app.main | imports ",
        "app.editor.js: app.editor | roots app.editor | imports app.main.js",
        "app.reports.js: clojure.string app.reports | roots app.reports | imports app.main.js",
    ];
    for (level, plan) in plans(&program) {
        if !level.advanced() {
            assert_eq!(membership(&program, &plan), namespaces, "{level}");
        }
    }
    // At `Advanced` the definitions of app.shared that only the features
    // read move toward them (Phase 3): `documents`, which both read, to the
    // `app.editor+app.reports` chunk of the Goal, and `index-by` to the
    // editor's chunk (`pluck` is inlined into its one reader); `label`,
    // which the menu reads, stays in startup.
    let prepared = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let namespace_plan = light_cljs::chunks::namespace_plan(&program, &prepared);
    check(
        &program,
        &prepared,
        &namespace_plan,
        OptimizationLevel::Advanced,
    );
    assert_eq!(membership(&program, &namespace_plan), namespaces);
    let plan = plan(&program, &prepared);
    assert_eq!(
        membership(&program, &plan),
        [
            "app.main.js: app.shared light.lazy app.main | roots app.main | imports ",
            "app.editor+app.reports.js:  | roots  | imports ",
            "app.editor.js: app.editor | roots app.editor | imports app.main.js app.editor+app.reports.js",
            "app.reports.js: clojure.string app.reports | roots app.reports | imports app.main.js app.editor+app.reports.js",
        ]
    );
    assert_eq!(
        moved(&prepared, &plan),
        [
            "app.main.js: ",
            "app.editor+app.reports.js: app.shared/documents",
            "app.editor.js: app.shared/index-by",
            "app.reports.js: ",
        ]
    );
    for (chunk, roots) in [
        (0, &["app.main", "app.editor", "app.reports"][..]),
        (1, &["app.editor", "app.reports"]),
        (2, &["app.editor"]),
        (3, &["app.reports"]),
    ] {
        assert_eq!(signature_roots(&program, &plan, chunk), roots);
    }
    // Each loadable site of app.main loads its feature's chunk.
    let targets = (0..program.lazy_targets.len())
        .map(|target| plan.target_chunk(&program, light_cljs::analyzer::LazyTargetId(target)))
        .collect::<Vec<_>>();
    assert_eq!(targets, [ChunkId(2), ChunkId(3)]);
    // No feature assigns a global of another chunk.
    assert!(plan.chunks.iter().all(|chunk| chunk.setters.is_empty()));
}

/// Each chunk of `plan` with the definitions it holds of namespaces another
/// chunk holds, by the global each defines.
fn moved(prepared: &PreparedApplication, plan: &ChunkPlan) -> Vec<String> {
    let program = prepared.program();
    plan.chunks
        .iter()
        .enumerate()
        .map(|(id, chunk)| {
            let definitions = plan
                .moved_items(ChunkId(id))
                .map(|item| {
                    let global = match &program.exprs[item.expr.0].kind {
                        ExprKind::Def { global, .. }
                        | ExprKind::DefProtocol { global, .. }
                        | ExprKind::DefType { global, .. } => *global,
                        other => panic!("{} holds {other:?}", chunk.name),
                    };
                    let global = &program.globals[global.0];
                    format!("{}/{}", global.namespace, global.name)
                })
                .collect::<Vec<_>>();
            format!("{}: {}", chunk.name, definitions.join(" "))
        })
        .collect()
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

// ---------------------------------------------------------------------------
// Chunked output: `PreparedApplication::emit_chunks` (light-cljs-274).

/// Run `program` at every level as chunks and as one `--esm` file, its
/// oracle, through the Node driver, and require the same record; link every
/// chunk in Node, where a missing export is a link error; and check that
/// each name a chunk imports is one the chunk it names exports. Returns the
/// record, the same at every level, and the files at each level.
fn runs_as_chunks(program: &Program) -> (String, Vec<(OptimizationLevel, ChunkedOutput)>) {
    let mut records = Vec::new();
    let mut outputs = Vec::new();
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(program, ApplicationOptions::new(level));
        let output = prepared.emit_chunks();
        let startup = output.files[0].name.as_str();
        let single = prepared.emit_esm();
        let oracle = run_modules(&[(startup, single.as_str())], startup)
            .unwrap_or_else(|error| panic!("{level}: {error}"));
        let mut files = output
            .files
            .iter()
            .map(|file| (file.name.as_str(), file.source.as_str()))
            .collect::<Vec<_>>();
        let listing = shown(&files);
        let record = run_modules(&files, startup)
            .unwrap_or_else(|error| panic!("{level}: {error}\n{listing}"));
        assert_eq!(record, oracle, "{level}:\n{listing}");
        check_imports(&output, level);
        // Import every chunk statically: Node links the whole graph before
        // it evaluates any of it.
        let link = output
            .files
            .iter()
            .map(|file| format!("import \"./{}\";\n", file.name))
            .collect::<String>();
        files.push(("link-every-chunk.js", &link));
        let linked = run_modules(&files, "link-every-chunk.js")
            .unwrap_or_else(|error| panic!("{level}: {error}"));
        assert!(
            linked.contains(r#""outcome":{"status":"returned""#),
            "{level}: {linked}\n{listing}"
        );
        records.push(record);
        outputs.push((level, output));
    }
    for (record, level) in records.iter().zip(OptimizationLevel::ALL) {
        assert_eq!(record, &records[0], "{level}");
    }
    (records.swap_remove(0), outputs)
}

/// Chunk files for a failure message, without the runtime's text, which is
/// the longest line of a terse startup chunk and its own lines in a readable
/// one.
fn shown<T: AsRef<str>>(files: &[(T, T)]) -> String {
    files
        .iter()
        .map(|(name, source)| {
            let shown = source
                .as_ref()
                .lines()
                .filter(|line| line.len() < 2_000 && !line.starts_with("    "))
                .collect::<Vec<_>>()
                .join("\n");
            format!("// {}\n{shown}\n", name.as_ref())
        })
        .collect()
}

/// The file and names of each `import { a, b } from "./x.js";` line of a
/// chunk, readable or terse.
fn imported_names(source: &str) -> Vec<(String, Vec<String>)> {
    source
        .lines()
        .filter_map(|line| {
            let (names, file) = line.strip_prefix("import")?.split_once("from")?;
            let names = names.trim().strip_prefix('{')?.strip_suffix('}')?;
            let file = file.trim().trim_end_matches(';').trim_matches('"');
            Some((
                file.strip_prefix("./")?.to_owned(),
                names
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .collect(),
            ))
        })
        .collect()
}

/// The names of the `export let` and `export { }` lines of a chunk.
fn exported_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let list = if let Some(list) = line.strip_prefix("export let ") {
            list
        } else if let Some(list) = line
            .strip_prefix("export")
            .and_then(|rest| rest.trim_start().strip_prefix('{'))
        {
            list.trim_end_matches(';').trim_end().trim_end_matches('}')
        } else {
            continue;
        };
        names.extend(
            list.trim_end_matches(';')
                .split(',')
                .map(|name| name.trim().to_owned()),
        );
    }
    names
}

/// Each name a chunk imports, the chunk it names exports under that name,
/// and that chunk is in the importer's `imports`.
fn check_imports(output: &ChunkedOutput, level: OptimizationLevel) {
    for file in &output.files {
        for (from, names) in imported_names(&file.source) {
            let exporter = output
                .files
                .iter()
                .find(|other| other.name == from)
                .unwrap_or_else(|| panic!("{level}: {} imports {from}", file.name));
            assert!(file.imports.contains(&from), "{level}: {}", file.name);
            let exported = exported_names(&exporter.source);
            for name in names {
                assert!(
                    exported.contains(&name),
                    "{level}: {} imports {name} from {from}, which exports {exported:?}",
                    file.name
                );
            }
        }
    }
}

/// Each file's name with its namespaces, roots and imports.
fn files(output: &ChunkedOutput) -> Vec<String> {
    output
        .files
        .iter()
        .map(|file| {
            format!(
                "{}: {} | roots {} | imports {}",
                file.name,
                file.namespaces.join(" "),
                file.roots.join(" "),
                file.imports.join(" ")
            )
        })
        .collect()
}

/// `output.manifest()` without the `"bytes"` of each chunk, which must be
/// the length of its file.
fn unsized_manifest(output: &ChunkedOutput) -> String {
    let mut files = output.files.iter();
    output
        .manifest()
        .lines()
        .map(|line| {
            let (chunk, comma) = match line.strip_suffix(',') {
                Some(chunk) => (chunk, ","),
                None => (line, ""),
            };
            let Some((head, bytes)) = chunk.rsplit_once(", \"bytes\": ") else {
                return format!("{line}\n");
            };
            let file = files.next().expect("a line per file");
            assert!(
                head.contains(&format!("\"file\": \"{}\"", file.name)),
                "{line}"
            );
            assert_eq!(bytes, format!("{}}}", file.source.len()), "{}", file.name);
            format!("{head}}}{comma}\n")
        })
        .collect()
}

/// The record of a run whose default export returned nil after printing
/// `stdout`.
fn printed(startup: &str, stdout: &str) -> String {
    format!(
        r#"{{"id":"{startup}","outcome":{{"status":"returned","value":null}},"trace":[],"stdout":"{stdout}"}}"#
    )
}

/// The record of `program` as chunks, which [`runs_as_chunks`] checks
/// against the single file at every level, and its files at `none`. The
/// files hold the same namespaces at every level; what they import differs
/// where `Advanced` code reads nothing of a chunk, and `Advanced` adds the
/// chunks that hold only definitions moved toward their readers.
fn chunk_files(program: &Program) -> (String, Vec<String>) {
    let (record, outputs) = runs_as_chunks(program);
    let members = |output: &ChunkedOutput| {
        output
            .files
            .iter()
            .zip(files(output))
            .filter(|(chunk, _)| !chunk.namespaces.is_empty() || chunk.definitions.is_empty())
            .map(|(_, file)| file.split(" | imports").next().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    for (level, output) in &outputs {
        assert_eq!(members(output), members(&outputs[0].1), "{level}");
    }
    (record, files(&outputs[0].1))
}

/// A feature loads a sub-feature, which requires it back (the
/// `sub-feature` fixture, which [`matrix`] runs): the loadable in the
/// feature's chunk imports the sub-feature's chunk, and the manifest gives
/// the site to the editor.
#[test]
fn a_feature_loads_its_sub_feature_chunk() {
    let program = fixture_program("sub-feature");
    for level in OptimizationLevel::ALL {
        let output = prepare_application(&program, ApplicationOptions::new(level)).emit_chunks();
        assert_eq!(
            unsized_manifest(&output),
            r#"{
  "startup": "app.main.js",
  "chunks": [
    {"name": "app.main", "file": "app.main.js", "imports": [], "namespaces": ["light.lazy", "app.main"], "definitions": [], "roots": ["app.main"], "loadables": []},
    {"name": "app.editor+app.spell", "file": "app.editor+app.spell.js", "imports": ["app.main.js"], "namespaces": ["app.text", "app.editor"], "definitions": [], "roots": ["app.editor"], "loadables": [{"namespace": "app.main", "target": "app.editor/start"}]},
    {"name": "app.spell", "file": "app.spell.js", "imports": ["app.main.js", "app.editor+app.spell.js"], "namespaces": ["app.spell"], "definitions": [], "roots": ["app.spell"], "loadables": [{"namespace": "app.editor", "target": "app.spell/check"}]}
  ]
}
"#,
            "{level}"
        );
    }
}

/// A lazy target that requires the loading namespace reads startup's
/// definitions through imports, and a loadable of a namespace startup
/// requires is resident: no import (the `requires-loader` fixture, which
/// [`matrix`] runs).
#[test]
fn a_feature_reads_the_namespace_that_loads_it() {
    let program = fixture_program("requires-loader");
    for level in OptimizationLevel::ALL {
        let output = prepare_application(&program, ApplicationOptions::new(level)).emit_chunks();
        assert_eq!(
            output.files[0].source.matches("import(").count(),
            1,
            "{level}: only the editor's loadable imports"
        );
        // The resident loadable resolves to startup, which holds its target.
        assert_eq!(
            unsized_manifest(&output),
            r#"{
  "startup": "app.main.js",
  "chunks": [
    {"name": "app.main", "file": "app.main.js", "imports": [], "namespaces": ["app.shared", "app.util", "light.lazy", "app.main"], "definitions": [], "roots": ["app.main", "app.util"], "loadables": [{"namespace": "app.main", "target": "app.util/twice"}]},
    {"name": "app.editor", "file": "app.editor.js", "imports": ["app.main.js"], "namespaces": ["app.text", "app.editor"], "definitions": [], "roots": ["app.editor"], "loadables": [{"namespace": "app.main", "target": "app.editor/start"}]}
  ]
}
"#,
            "{level}"
        );
    }
}

/// A feature that rebinds a core function the program reads assigns
/// startup's output-level cell of it through the cell's setter at
/// `Advanced`, and the runtime's core table below it.
#[test]
fn a_feature_rebinds_a_core_function_through_startup() {
    static CORE: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (defn bump [x] (inc x))\n\
             (defn start []\n\
               (-> (lazy/load editor)\n\
                   (.then (fn [f] (js/console.log (f) (bump 1)) nil))))\n\
             start\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.main :as main]))\n\
             (defn start [] (with-redefs [inc dec] (main/bump 1)))\n",
        ),
    ];
    let (record, _) = chunk_files(&analyze(CORE));
    assert_eq!(record, printed("app.main.js", r"0 2\n"));
}

/// A feature probing a Var of a namespace without a source imports the
/// binding from startup, which declares it.
#[test]
fn a_feature_probes_a_var_startup_declares() {
    static PROBE: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (defn start []\n\
               (-> (lazy/load editor) (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor)\n(defn start [] (exists? missing.ns/value))\n",
        ),
    ];
    let (record, _) = chunk_files(&analyze(PROBE));
    assert_eq!(record, printed("app.main.js", r"false\n"));
}

/// A loadable of a namespace that only adds methods to a multimethod of
/// startup loads its chunk and resolves to nil (the `methods` fixture, which
/// [`matrix`] runs); the manifest writes a namespace target as its
/// namespace.
#[test]
fn a_namespace_loadable_runs_its_methods_when_loaded() {
    let program = fixture_program("methods");
    for level in OptimizationLevel::ALL {
        let output = prepare_application(&program, ApplicationOptions::new(level)).emit_chunks();
        assert!(
            unsized_manifest(&output).contains(
                r#""file": "app.shapes.js", "imports": ["app.main.js"], "namespaces": ["app.shapes"], "definitions": [], "roots": ["app.shapes"], "loadables": [{"namespace": "app.main", "target": "app.shapes"}]}"#
            ),
            "{level}: {}",
            output.manifest()
        );
    }
}

/// Raw `js*` text in a feature: at `Advanced` the short names avoid its
/// identifiers in every chunk; a hole inside a raw string makes every chunk
/// keep the hygiene prefix, one prefix for all of them.
#[test]
fn raw_javascript_in_a_feature_keeps_names_consistent_across_chunks() {
    let main = "(ns app.main (:require [light.lazy :as lazy]))\n\
                (def editor (lazy/loadable app.editor/start))\n\
                (def base 20)\n\
                (defn start []\n\
                  (-> (lazy/load editor) (.then (fn [f] (js/console.log (f 2)) nil))))\n\
                start\n";
    for (raw, prefixed) in [("", false), ("(js* \"void '~{}'\" a)", true)] {
        let editor = format!(
            "(ns app.editor (:require [app.main :as main]))\n\
             (defn start [x]\n\
               (let [a (js* \"(~{{}} * 21)\" x)] {raw} (str a \" \" (+ a main/base))))\n"
        );
        let program = analyze(&[("app/main.cljs", main), ("app/editor.cljs", &editor)]);
        let (record, _) = chunk_files(&program);
        assert_eq!(record, printed("app.main.js", r"42 62\n"), "{raw}");
        let terse = prepare_application(
            &program,
            ApplicationOptions::new(OptimizationLevel::Advanced),
        )
        .emit_chunks();
        for file in &terse.files {
            assert_eq!(
                file.source.contains("__light"),
                prefixed,
                "{raw}: {}",
                file.name
            );
        }
    }
}

/// A program without namespace sources is one chunk, which runs as the
/// single file does.
#[test]
fn a_program_without_namespace_sources_runs_as_one_chunk() {
    let source = "(def a 1)\n(defn f [] (set! a (inc a)) a)\n(do (f) (f))\n";
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    let (record, files) = chunk_files(&program);
    assert_eq!(
        record,
        r#"{"id":"cljs.user.js","outcome":{"status":"returned","value":3},"trace":[],"stdout":""}"#
    );
    assert_eq!(
        files,
        ["cljs.user.js: cljs.user | roots cljs.user | imports "]
    );
}

/// An entry without forms is nil, in startup, while a dependency of the
/// entry holds a loadable of a namespace that only it reaches.
#[test]
fn an_entry_without_forms_returns_nil_from_startup() {
    static EMPTY: &[(&str, &str)] = &[
        ("app/main.cljs", "(ns app.main (:require [app.dep]))\n"),
        (
            "app/dep.cljs",
            "(ns app.dep)\n(def l (light.lazy/loadable app.lib))\n",
        ),
        ("app/lib.cljs", "(ns app.lib)\n(def x 1)\n"),
    ];
    let (record, files) = chunk_files(&analyze(EMPTY));
    assert_eq!(record, printed("app.main.js", ""));
    assert_eq!(
        files,
        [
            "app.main.js: light.lazy app.dep app.main | roots app.main | imports ",
            // Below `Advanced` it switches `*ns*` through startup's runtime.
            "app.lib.js: app.lib | roots app.lib | imports app.main.js",
        ]
    );
}

/// A feature that reaches reflection finds every namespace, the lazy ones
/// too: startup registers them all before any form runs, as the single file
/// does, and at `Advanced` every chunk's forms decide whether it does.
#[test]
fn a_feature_reaching_reflection_finds_every_namespace() {
    static REFLECTION: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (def base 1)\n\
             (defn start []\n\
               (-> (lazy/load editor) (.then (fn [f] (js/console.log (pr-str (f))) nil))))\n\
             start\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor)\n\
             (def words 2)\n\
             (defn start []\n\
               [(ns-name (find-ns 'app.main)) (ns-name (find-ns 'app.editor))\n\
                (some? (find-ns 'app.missing))])\n",
        ),
    ];
    let (record, _) = chunk_files(&analyze(REFLECTION));
    assert_eq!(
        record,
        printed("app.main.js", r"[app.main app.editor false]\n")
    );
}

/// `loaded?` is whether the target's chunk has been loaded: loading one
/// loadable of a chunk makes every loadable of that chunk loaded, those
/// startup made before and those a feature makes after, as in the single
/// file, where every loadable is.
#[test]
fn loading_one_target_marks_other_targets_in_the_same_chunk_loaded() {
    static SAME_CHUNK: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def reports (lazy/loadable app.reports/start))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (def stop (lazy/loadable app.editor/stop))\n\
             (def editor-ns (lazy/loadable app.editor))\n\
             (defn start []\n\
               (-> (lazy/load reports)\n\
                   (.then (fn [f] (js/console.log (f)) (lazy/load editor)))\n\
                   (.then (fn [f]\n\
                            (js/console.log (f) (lazy/loaded? stop) (lazy/loaded? editor-ns)\n\
                                            (@stop))\n\
                            (lazy/load stop)))\n\
                   (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [light.lazy :as lazy]))\n\
             (def reports (lazy/loadable app.reports/start))\n\
             (defn start [] (str \"started, reports loaded \" (lazy/loaded? reports)))\n\
             (defn stop [] \"stopped\")\n",
        ),
        (
            "app/reports.cljs",
            "(ns app.reports)\n(defn start [] \"reported\")\n",
        ),
    ];
    let (record, _) = chunk_files(&analyze(SAME_CHUNK));
    assert_eq!(
        record,
        printed(
            "app.main.js",
            r"reported\nstarted, reports loaded true true true stopped\nstopped\n"
        )
    );
}

/// A package only a feature requires is imported by the feature's chunk
/// alone, under its terse name at `Advanced` too (the `package` fixture,
/// which [`matrix`] runs with the package installed).
#[test]
fn a_package_only_a_feature_requires_is_imported_by_its_chunk() {
    let program = fixture_program("package");
    for level in OptimizationLevel::ALL {
        let output = prepare_application(&program, ApplicationOptions::new(level)).emit_chunks();
        let importers = output
            .files
            .iter()
            .filter(|file| file.source.contains("from \"stub-package\""))
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(importers, ["app.editor.js"], "{level}");
    }
}

/// At `Advanced` definitions move toward their readers (the `definitions`
/// fixture, which [`matrix`] runs at every level): `fmt`, which both
/// features read, to their shared chunk, and the helper and the protocol
/// only the editor reads to the editor's chunk, where the protocol's
/// dispatcher is defined before the types that name it. Each `defmethod`
/// stays in its namespace's chunk. Their `defmulti` stays pinned in
/// app.shared's chunk, startup: removal never drops it, since its
/// initializer builds a `MultiFn` on the global hierarchy, which `Inert`
/// does not accept, and only what removal could drop moves.
#[test]
fn definitions_move_toward_their_readers_and_methods_stay() {
    let program = fixture_program("definitions");
    let options = ApplicationOptions {
        pseudo_names: true,
        ..ApplicationOptions::new(OptimizationLevel::Advanced)
    };
    let prepared = prepare_application(&program, options);
    let plan = plan(&program, &prepared);
    check(&program, &prepared, &plan, OptimizationLevel::Advanced);
    assert_eq!(
        moved(&prepared, &plan),
        [
            "app.main.js: ",
            "app.editor+app.reports.js: app.shared/fmt",
            "app.editor.js: app.shared/outline app.shared/Shape",
            "app.reports.js: ",
        ]
    );
    let emitted = prepared.program();
    let render = plan
        .items
        .iter()
        .find(|item| {
            matches!(&emitted.exprs[item.expr.0].kind,
                ExprKind::Def { global, .. } if emitted.globals[global.0].name == "render")
        })
        .unwrap();
    assert!(!render.movable && render.chunk == STARTUP);
    let output = prepared.emit_chunks();
    let source = |name: &str| {
        output
            .files
            .iter()
            .find(|file| file.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .source
            .as_str()
    };
    let (startup, editor, reports) = (
        source("app.main.js"),
        source("app.editor.js"),
        source("app.reports.js"),
    );
    assert!(startup.contains("new rt$MultiFn("), "{startup}");
    for (chunk, method) in [(editor, "\"<p>\""), (reports, "\" rows\"")] {
        assert!(
            chunk.contains("rt$addMethodProtocol(") && chunk.contains(method),
            "{chunk}"
        );
        assert!(!startup.contains(method));
    }
    let dispatcher = editor
        .find("rt$protocolMethod(\"app.shared/Shape/area\"")
        .unwrap_or_else(|| panic!("{editor}"));
    assert!(
        dispatcher < editor.find("rt$defineType(").unwrap(),
        "{editor}"
    );
    assert!(!startup.contains("app.shared/Shape"));
}

/// What moves at `Advanced`, and what stays: a record and the definition
/// that constructs it move together to the one feature reading them, a
/// forward-declared helper too; a definition a feature assigns moves with
/// its readers and is assigned through its setter; and a protocol a startup
/// `extend-type` names, a Var defined twice and a function holding a
/// loadable stay in their namespace. The chunks print what the single file
/// prints at every level.
#[test]
fn definitions_move_with_their_constructors_and_setters() {
    static EDGES: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.shared :as shared] [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (def reports (lazy/loadable app.reports/start))\n\
             (extend-type number shared/Sized (size [n] (* 2 n)))\n\
             (defn start []\n\
               (-> (lazy/load editor)\n\
                   (.then (fn [f] (js/console.log (f)) (js/console.log (f)) (lazy/load reports)))\n\
                   (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
        ),
        (
            "app/shared.cljs",
            "(ns app.shared (:require [light.lazy :as lazy]))\n\
             (defprotocol Sized (size [x]))\n\
             (defrecord Point [x y])\n\
             (def origin (->Point 1 2))\n\
             (def counter 0)\n\
             (def twice 1)\n\
             (def twice 2)\n\
             (declare helper)\n\
             (defn describe [p]\n\
               (str (apply str (map helper [(:x p) (:y p)])) \" at \" (:x p) \",\" (:y p)))\n\
             (defn helper [n] (str \"<\" (size n) \">\"))\n\
             (defn spelling [] (lazy/loadable app.editor/start))\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.shared :as shared]))\n\
             (defn start []\n\
               (set! shared/counter (inc shared/counter))\n\
               (str (shared/describe (update shared/origin :x + 2)) \"; \"\n\
                    (shared/describe shared/origin) \"; \" shared/counter \" \" shared/twice))\n",
        ),
        (
            "app/reports.cljs",
            "(ns app.reports (:require [app.shared :as shared] [light.lazy :as lazy]))\n\
             (defn start [] (str \"reports \" shared/counter \" \" (lazy/loaded? (shared/spelling))))\n",
        ),
    ];
    let program = analyze(EDGES);
    let (record, _) = chunk_files(&program);
    assert_eq!(
        record,
        printed(
            "app.main.js",
            r"<6><4> at 3,2; <2><4> at 1,2; 1 2\n<6><4> at 3,2; <2><4> at 1,2; 2 2\nreports 2 true\n"
        )
    );
    let prepared = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let plan = plan(&program, &prepared);
    check(&program, &prepared, &plan, OptimizationLevel::Advanced);
    assert_eq!(
        moved(&prepared, &plan),
        [
            "app.main.js: ",
            "app.editor+app.reports.js: app.shared/counter",
            "app.editor.js: app.shared/Point app.shared/origin app.shared/describe app.shared/helper",
            "app.reports.js: ",
        ]
    );
    // The shared chunk owns `counter` and exports its setter, which the
    // editor's `set!` calls.
    assert_eq!(plan.chunks[1].setters.len(), 1);
}

/// A raw `deftype*` runs its body when it defines the type, so it stays in
/// its namespace's chunk although only a feature constructs the type: the
/// body's line prints before startup's, as in the single file.
#[test]
fn raw_type_bodies_stay_in_their_namespace() {
    static RAW: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.shared :as shared] [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (js/console.log \"menu\")\n\
             (defn start [] (-> (lazy/load editor) (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
        ),
        (
            "app/shared.cljs",
            "(ns app.shared)\n(deftype* T [x] {} (js/console.log \"type\"))\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.shared :as shared]))\n\
             (defn start [] (.-x (shared/T. 5)))\n",
        ),
    ];
    let program = analyze(RAW);
    let (record, _) = chunk_files(&program);
    assert_eq!(record, printed("app.main.js", r"type\nmenu\n5\n"));
    let prepared = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let plan = plan(&program, &prepared);
    assert!(!plan.moves_definitions(), "{:?}", moved(&prepared, &plan));
}

/// A definition that only an explicit export keeps is reached by no root:
/// it stays in its namespace, startup's here, and the walks run again from
/// it, so the helper it shares with a feature stays in startup too instead
/// of moving to the feature's chunk, which startup cannot import.
#[test]
fn explicit_exports_root_their_movable_dependencies() {
    static EXPORTS: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.shared :as shared] [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (defn start [] (-> (lazy/load editor) (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
        ),
        (
            "app/shared.cljs",
            "(ns app.shared)\n\
             (defn helper [x] (str \"<\" x \">\"))\n\
             (defn exported [x] (helper (str x \"!\")))\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.shared :as shared]))\n\
             (defn start [] (shared/helper \"editor\"))\n",
        ),
    ];
    let program = analyze(EXPORTS);
    let global = |name: &str| {
        light_cljs::GlobalId(
            program
                .globals
                .iter()
                .position(|global| global.namespace == "app.shared" && global.name == name)
                .unwrap(),
        )
    };
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.exports = vec![global("exported")];
    options.no_inlining = true;
    let prepared = prepare_application(&program, options);
    let plan = plan(&program, &prepared);
    check(&program, &prepared, &plan, OptimizationLevel::Advanced);
    assert_eq!(
        moved(&prepared, &plan),
        ["app.main.js: ", "app.editor.js: "]
    );
    for name in ["helper", "exported"] {
        assert_eq!(plan.global_chunks[global(name).0], STARTUP, "{name}");
    }
    // Without the export, the helper moves to the editor, its one reader.
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.no_inlining = true;
    let alone = prepare_application(&program, options);
    assert_eq!(
        moved(&alone, &plan_of(&program, &alone)),
        ["app.main.js: ", "app.editor.js: app.shared/helper"]
    );
    let output = prepared.emit_chunks();
    let files = output
        .files
        .iter()
        .map(|file| (file.name.as_str(), file.source.as_str()))
        .collect::<Vec<_>>();
    let record = run_modules(&files, "app.main.js").unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(record, printed("app.main.js", r"<editor>\n"));
    check_imports(&output, OptimizationLevel::Advanced);
}

/// The plan of `prepared`, checked.
fn plan_of(program: &Program, prepared: &PreparedApplication) -> ChunkPlan {
    let plan = plan(program, prepared);
    check(program, prepared, &plan, prepared.level());
    plan
}

/// A program that reaches reflection stays namespace granular at
/// `Advanced` (the `reflection` fixture, which [`matrix`] runs at every
/// level): the plan would move app.shared's helper to the editor, which
/// alone reads it, but the editor looks namespaces up, so `emit_chunks`
/// keeps every form in its namespace's chunk.
#[test]
fn a_program_reaching_reflection_keeps_definitions_in_their_namespaces() {
    let program = fixture_program("reflection");
    let prepared = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let plan = plan(&program, &prepared);
    assert_eq!(
        moved(&prepared, &plan),
        ["app.main.js: ", "app.editor.js: app.shared/outline"]
    );
    let namespaces = light_cljs::chunks::namespace_plan(&program, &prepared);
    check(
        &program,
        &prepared,
        &namespaces,
        OptimizationLevel::Advanced,
    );
    assert!(!namespaces.moves_definitions());
    let output = prepared.emit_chunks();
    assert_eq!(
        files(&output),
        [
            "app.main.js: app.shared light.lazy app.main | roots app.main | imports ",
            "app.editor.js: app.editor | roots app.editor | imports app.main.js",
        ]
    );
    assert!(output.files.iter().all(|file| file.definitions.is_empty()));
    assert!(output.files[0].source.contains(" words, first "));
    assert!(!output.files[1].source.contains(" words, first "));
}

/// The CLI's output for `args`: whether it succeeded, its stdout and stderr.
fn cli(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// The names of the files in `dir`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// `--out-dir` writes the chunk files of examples/features and chunks.json
/// into a directory it creates, at every level: the files the manifest
/// names, which hold what the library returns, and which print in Node what
/// the single `--esm` file prints. Files the manifest does not name stay.
#[test]
fn out_dir_writes_the_features_example_chunks_and_manifest_at_every_level() {
    let dir = TempDir::new("chunks-out-dir");
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
    let program = analyze_namespace_entry(
        "app.main",
        &[PathBuf::from(root)],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    for level in OptimizationLevel::ALL {
        // A directory below one that does not exist either.
        let out = dir.join(level.name()).join("out");
        if level == OptimizationLevel::Advanced {
            // A file of an earlier build is overwritten; another stays.
            fs::create_dir_all(&out).unwrap();
            fs::write(out.join("app.editor.js"), "stale").unwrap();
            fs::write(out.join("notes.txt"), "kept").unwrap();
        }
        let args = [
            "--compile",
            "--optimize",
            level.name(),
            "--entry",
            "app.main",
            "--source-root",
            root,
            "--out-dir",
            out.to_str().unwrap(),
        ];
        assert_eq!(cli(&args), (true, String::new(), String::new()), "{level}");
        let mut expected = vec![
            "app.editor.js",
            "app.main.js",
            "app.reports.js",
            "chunks.json",
        ];
        if level == OptimizationLevel::Advanced {
            expected.insert(0, "app.editor+app.reports.js");
            expected.push("notes.txt");
            assert_eq!(fs::read_to_string(out.join("notes.txt")).unwrap(), "kept");
        }
        assert_eq!(listing(&out), expected, "{level}");
        let output = prepare_application(&program, ApplicationOptions::new(level)).emit_chunks();
        for file in &output.files {
            assert_eq!(
                fs::read_to_string(out.join(&file.name)).unwrap(),
                file.source,
                "{level}: {}",
                file.name
            );
        }
        let manifest = fs::read_to_string(out.join("chunks.json")).unwrap();
        assert_eq!(manifest, output.manifest(), "{level}");
        // At `Advanced` the definitions of app.shared that only the
        // features read move toward them (docs/code-splitting.md, Phase 3).
        let expected = if level == OptimizationLevel::Advanced {
            r#"{
  "startup": "app.main.js",
  "chunks": [
    {"name": "app.main", "file": "app.main.js", "imports": [], "namespaces": ["app.shared", "light.lazy", "app.main"], "definitions": [], "roots": ["app.main"], "loadables": []},
    {"name": "app.editor+app.reports", "file": "app.editor+app.reports.js", "imports": ["app.main.js"], "namespaces": [], "definitions": ["app.shared/documents"], "roots": [], "loadables": []},
    {"name": "app.editor", "file": "app.editor.js", "imports": ["app.main.js", "app.editor+app.reports.js"], "namespaces": ["app.editor"], "definitions": ["app.shared/index-by"], "roots": ["app.editor"], "loadables": [{"namespace": "app.main", "target": "app.editor/start"}]},
    {"name": "app.reports", "file": "app.reports.js", "imports": ["app.main.js", "app.editor+app.reports.js"], "namespaces": ["clojure.string", "app.reports"], "definitions": [], "roots": ["app.reports"], "loadables": [{"namespace": "app.main", "target": "app.reports/start"}]}
  ]
}
"#
        } else {
            r#"{
  "startup": "app.main.js",
  "chunks": [
    {"name": "app.main", "file": "app.main.js", "imports": [], "namespaces": ["app.shared", "light.lazy", "app.main"], "definitions": [], "roots": ["app.main"], "loadables": []},
    {"name": "app.editor", "file": "app.editor.js", "imports": ["app.main.js"], "namespaces": ["app.editor"], "definitions": [], "roots": ["app.editor"], "loadables": [{"namespace": "app.main", "target": "app.editor/start"}]},
    {"name": "app.reports", "file": "app.reports.js", "imports": ["app.main.js"], "namespaces": ["clojure.string", "app.reports"], "definitions": [], "roots": ["app.reports"], "loadables": [{"namespace": "app.main", "target": "app.reports/start"}]}
  ]
}
"#
        };
        assert_eq!(unsized_manifest(&output), expected, "{level}");
        // `node tests/support/chunked_execution.mjs OUT app.main.js` prints
        // the single file's lines.
        let single = crate::features_example::compile_features(level, true);
        let oracle = run_modules(&[("app.main.js", &single)], "app.main.js")
            .unwrap_or_else(|error| panic!("{level}: {error}"));
        let record =
            run_directory(&out, "app.main.js").unwrap_or_else(|error| panic!("{level}: {error}"));
        assert_eq!(record, oracle, "{level}");
        assert_eq!(record, crate::features_example::FEATURES_RECORD, "{level}");
    }
}

/// `--out-dir` on a program without loadables writes one file, the `--esm`
/// output, and its manifest, at every level; `--esm` may be given too, and
/// `--size-report` prints the single file's report.
#[test]
fn out_dir_of_a_program_without_loadables_is_its_esm_file() {
    let dir = TempDir::new("chunks-out-dir-one");
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/namespaces");
    for level in OptimizationLevel::ALL {
        let out = dir.join(level.name());
        let out = out.to_str().unwrap();
        let base = [
            "--compile",
            "--optimize",
            level.name(),
            "--size-report",
            "--entry",
            "demo.main",
            "--source-root",
            root,
        ];
        let (success, esm, report) = cli(&[&base[..], &["--esm"]].concat());
        assert!(
            success && !esm.is_empty() && !report.is_empty(),
            "{level}: {report}"
        );
        for extra in [&["--out-dir", out][..], &["--esm", "--out-dir", out]] {
            assert_eq!(
                cli(&[&base[..], extra].concat()),
                (true, String::new(), report.clone()),
                "{level}: {extra:?}"
            );
            assert_eq!(listing(Path::new(out)), ["chunks.json", "demo.main.js"]);
            assert_eq!(
                fs::read_to_string(Path::new(out).join("demo.main.js")).unwrap(),
                esm,
                "{level}"
            );
            assert_eq!(
                fs::read_to_string(Path::new(out).join("chunks.json")).unwrap(),
                format!(
                    r#"{{
  "startup": "demo.main.js",
  "chunks": [
    {{"name": "demo.main", "file": "demo.main.js", "imports": [], "namespaces": ["demo.math", "demo.main"], "definitions": [], "roots": ["demo.main"], "loadables": [], "bytes": {}}}
  ]
}}
"#,
                    esm.len()
                ),
                "{level}"
            );
        }
    }
}

/// `--out-dir` writes several files without source maps, and comes after
/// `--entry`; the refused flags leave no directory behind.
#[test]
fn out_dir_refuses_the_source_map_flags() {
    let dir = TempDir::new("chunks-out-dir-flags");
    let out = dir.join("out");
    let out = out.to_str().unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
    let map = dir.join("app.js.map");
    for flags in [
        &["--source-map", map.to_str().unwrap()][..],
        &["--source-map-inline"],
        &["--source-map-include-sources"],
    ] {
        let args = [
            &["--compile"][..],
            flags,
            &[
                "--entry",
                "app.main",
                "--source-root",
                root,
                "--out-dir",
                out,
            ],
        ]
        .concat();
        let (success, stdout, stderr) = cli(&args);
        assert!(
            !success
                && stdout.is_empty()
                && stderr.contains(
                    "--out-dir writes one ES module per chunk without source maps; it excludes --source-map, --source-map-inline and --source-map-include-sources"
                ),
            "{flags:?}: {stderr}"
        );
    }
    let (success, _, stderr) = cli(&["--compile", "--text", "1", "--out-dir", out]);
    assert!(!success && stderr.contains("Usage:"), "{stderr}");
    assert_eq!(listing(&dir), Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// The fixture matrix (light-cljs-276): each program of tests/fixtures/chunks
// written by `--out-dir` at every level and checked through its chunks.json.

/// One `#[test]` per fixture, and [`FIXTURES`], their directories.
macro_rules! fixture_matrix {
    ($($test:ident: $fixture:literal),* $(,)?) => {
        const FIXTURES: &[&str] = &[$($fixture),*];
        $(
            #[test]
            fn $test() {
                matrix($fixture);
            }
        )*
    };
}

fixture_matrix! {
    matrix_features_example: "features",
    matrix_definitions_move_toward_their_readers: "definitions",
    matrix_reflection_keeps_namespaces_whole: "reflection",
    matrix_diamond: "diamond",
    matrix_sub_feature: "sub-feature",
    matrix_target_requiring_its_loader: "requires-loader",
    matrix_shared_top_level_effect: "shared-effect",
    matrix_feature_assigning_startup_vars: "startup-vars",
    matrix_package_only_a_feature_requires: "package",
    matrix_namespace_of_methods: "methods",
}

/// Each directory of tests/fixtures/chunks is a fixture of the matrix.
#[test]
fn the_matrix_runs_every_fixture_directory() {
    let mut fixtures = FIXTURES.to_vec();
    fixtures.sort();
    assert_eq!(listing(&fixture_dir("")), fixtures);
}

/// Fixture `name`'s directory: a program of entry `app.main` (`features`
/// takes examples/features instead); `stdout.txt`, what it prints; and
/// `membership.tsv`, its chunks in manifest order, a row each: the file, its
/// namespaces, its roots, its imports below `Advanced` (`n/a` for a chunk
/// only `Advanced` has) and at `Advanced`, and the definitions it holds at
/// `Advanced` of namespaces other chunks hold, `-` for none. `packages`, if
/// present, holds the JavaScript packages it imports, which a run finds in
/// `node_modules`.
fn fixture_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/chunks")
        .join(name)
}

fn fixture_root(name: &str) -> PathBuf {
    match name {
        "features" => Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/features"),
        _ => fixture_dir(name),
    }
}

fn fixture_program(name: &str) -> Program {
    analyze_namespace_entry(
        "app.main",
        &[fixture_root(name)],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// Copies fixture `name`'s packages, if it has any, into `dir/node_modules`.
fn install_packages(name: &str, dir: &Path) {
    fn copy(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let packages = fixture_dir(name).join("packages");
    if packages.is_dir() {
        copy(&packages, &dir.join("node_modules"));
    }
}

/// A chunk of `chunks.json`.
pub(super) struct ManifestChunk {
    name: String,
    file: String,
    imports: Vec<String>,
    namespaces: Vec<String>,
    /// The definitions it holds of namespaces another chunk holds.
    definitions: Vec<String>,
    roots: Vec<String>,
    /// Each loadable site's namespace and target.
    loadables: Vec<(String, String)>,
    bytes: usize,
}

/// The startup file and the chunks of `dir/chunks.json`.
pub(super) fn read_manifest(dir: &Path) -> (String, Vec<ManifestChunk>) {
    let manifest = Json::parse(&fs::read_to_string(dir.join("chunks.json")).unwrap());
    let strings = |list: &Json| {
        list.items()
            .iter()
            .map(|item| item.str().to_owned())
            .collect::<Vec<_>>()
    };
    let chunks = manifest
        .get("chunks")
        .items()
        .iter()
        .map(|chunk| ManifestChunk {
            name: chunk.get("name").str().to_owned(),
            file: chunk.get("file").str().to_owned(),
            imports: strings(chunk.get("imports")),
            namespaces: strings(chunk.get("namespaces")),
            definitions: strings(chunk.get("definitions")),
            roots: strings(chunk.get("roots")),
            loadables: chunk
                .get("loadables")
                .items()
                .iter()
                .map(|site| {
                    (
                        site.get("namespace").str().to_owned(),
                        site.get("target").str().to_owned(),
                    )
                })
                .collect(),
            bytes: match chunk.get("bytes") {
                Json::Number(bytes) => *bytes as usize,
                other => panic!("bytes: {other:?}"),
            },
        })
        .collect();
    (manifest.get("startup").str().to_owned(), chunks)
}

/// The chunk files the leading `import` lines of `source` name, in order:
/// `import { a } from "./x.js";` and `import "./x.js";`, readable or terse,
/// but not a package's `import ... from "name";`.
fn static_imports(source: &str) -> Vec<String> {
    source
        .lines()
        .take_while(|line| line.starts_with("import"))
        .filter_map(|line| {
            let specifier = line.trim_end_matches(';').rsplit('"').nth(1)?;
            Some(specifier.strip_prefix("./")?.to_owned())
        })
        .collect()
}

/// The chunk files `source` imports dynamically, `import("./x.js")`,
/// sorted.
fn dynamic_imports(source: &str) -> Vec<String> {
    let mut files = source
        .split("import(\"./")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_owned())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

/// For each pair of chunks, whether the second is the first or one the first
/// imports transitively, after checking that every import names an earlier
/// chunk.
fn reach(chunks: &[ManifestChunk], level: OptimizationLevel) -> Vec<Vec<bool>> {
    let mut reach = vec![vec![false; chunks.len()]; chunks.len()];
    for (i, chunk) in chunks.iter().enumerate() {
        reach[i][i] = true;
        for import in &chunk.imports {
            let j = chunks
                .iter()
                .position(|other| &other.file == import)
                .unwrap_or_else(|| panic!("{level}: {} imports {import}", chunk.file));
            assert!(j < i, "{level}: {} imports {import}", chunk.file);
            let (earlier, rest) = reach.split_at_mut(i);
            for (own, imported) in rest[0].iter_mut().zip(&earlier[j]) {
                *own |= *imported;
            }
        }
    }
    reach
}

/// What `chunks.json` in `out` says against the files: they are the files
/// in `out`, of the sizes it gives; each file's import lines name the chunks
/// it lists, which export every name imported from them, and its dynamic
/// imports are the chunks of the loadables its namespaces hold that it does
/// not reach statically; the namespaces partition the program's, with each
/// root and loadable target in its chunk; and every import edge targets a
/// strictly larger signature, the roots whose chunks reach a chunk. The
/// first chunk is startup, whose first root is `entry`, and every chunk
/// holds a namespace or a definition.
pub(super) fn check_manifest(
    out: &Path,
    startup: &str,
    chunks: &[ManifestChunk],
    level: OptimizationLevel,
    entry: &str,
) {
    assert_eq!(startup, chunks[0].file, "{level}");
    assert_eq!(chunks[0].roots[0], entry, "{level}");
    let mut files = chunks
        .iter()
        .map(|chunk| chunk.file.clone())
        .chain(["chunks.json".to_owned()])
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(listing(out), files, "{level}");
    let reach = reach(chunks, level);
    let index = |file: &str| chunks.iter().position(|chunk| chunk.file == file).unwrap();
    let chunk_of = |namespace: &str| {
        chunks
            .iter()
            .position(|chunk| chunk.namespaces.iter().any(|n| n == namespace))
            .unwrap_or_else(|| panic!("{level}: no chunk holds {namespace}"))
    };
    let sources = chunks
        .iter()
        .map(|chunk| fs::read_to_string(out.join(&chunk.file)).unwrap())
        .collect::<Vec<_>>();
    for (i, chunk) in chunks.iter().enumerate() {
        let source = &sources[i];
        assert_eq!(chunk.file, format!("{}.js", chunk.name), "{level}");
        // No chunk is empty in the manifest. Its code may still be, at
        // `Advanced`, `(()=>{})();` (light-cljs-299 drops such chunks).
        assert!(
            !chunk.namespaces.is_empty() || !chunk.definitions.is_empty(),
            "{level}: {} holds neither a namespace nor a definition",
            chunk.file
        );
        assert_eq!(source.len(), chunk.bytes, "{level}: {}", chunk.file);
        assert_eq!(
            static_imports(source),
            chunk.imports,
            "{level}: {}",
            chunk.file
        );
        for (from, names) in imported_names(source) {
            let exported = exported_names(&sources[index(&from)]);
            for name in names {
                assert!(
                    exported.contains(&name),
                    "{level}: {} imports {name} from {from}, which exports {exported:?}",
                    chunk.file
                );
            }
        }
        for namespace in &chunk.namespaces {
            assert_eq!(chunk_of(namespace), i, "{level}: {namespace} twice");
        }
        // A definition moves, at `Advanced` only, out of its namespace's
        // chunk into one its namespace's chunk does not import.
        assert!(
            level.advanced() || chunk.definitions.is_empty(),
            "{level}: {}",
            chunk.file
        );
        for definition in &chunk.definitions {
            let (namespace, _) = definition.rsplit_once('/').unwrap();
            let home = chunk_of(namespace);
            assert!(home != i && !reach[home][i], "{level}: {definition}");
        }
        for root in &chunk.roots {
            assert_eq!(chunk_of(root), i, "{level}: root {root}");
        }
        for (_, target) in &chunk.loadables {
            let namespace = target.split('/').next().unwrap();
            assert_eq!(chunk_of(namespace), i, "{level}: target {target}");
        }
        // A loadable is resident, without an import, where its site's chunk
        // reaches its target's chunk, which includes startup's.
        let mut lazy = chunks
            .iter()
            .enumerate()
            .filter(|&(target, other)| {
                target != 0
                    && !reach[i][target]
                    && other
                        .loadables
                        .iter()
                        .any(|(namespace, _)| chunk_of(namespace) == i)
            })
            .map(|(_, other)| other.file.clone())
            .collect::<Vec<_>>();
        lazy.sort();
        assert_eq!(dynamic_imports(source), lazy, "{level}: {}", chunk.file);
    }
    let signature = |chunk: usize| {
        chunks
            .iter()
            .enumerate()
            .filter(|&(other, _)| reach[other][chunk])
            .flat_map(|(_, other)| other.roots.iter().map(String::as_str))
            .collect::<std::collections::BTreeSet<_>>()
    };
    for (i, chunk) in chunks.iter().enumerate() {
        let inner = signature(i);
        assert!(!inner.is_empty(), "{level}: no root reaches {}", chunk.file);
        for import in &chunk.imports {
            let outer = signature(index(import));
            assert!(
                outer.is_superset(&inner) && outer != inner,
                "{level}: {} {inner:?} imports {import} {outer:?}",
                chunk.file
            );
        }
    }
}

/// Each chunk's file, namespaces, roots, imports and moved definitions.
fn membership_rows(chunks: &[ManifestChunk]) -> Vec<String> {
    chunks
        .iter()
        .map(|chunk| {
            format!(
                "{} | {} | roots {} | imports {} | definitions {}",
                chunk.file,
                chunk.namespaces.join(" "),
                chunk.roots.join(" "),
                chunk.imports.join(" "),
                chunk.definitions.join(" ")
            )
        })
        .collect()
}

/// The rows of fixture `name`'s `membership.tsv` at `level`, as
/// [`membership_rows`] writes them. A chunk whose imports below `Advanced`
/// are `n/a` holds only definitions that move at `Advanced`, and exists only
/// there; below it no definition moves.
fn expected_membership(name: &str, level: OptimizationLevel) -> Vec<String> {
    let table = fs::read_to_string(fixture_dir(name).join("membership.tsv")).unwrap();
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let cells = line
                .split('\t')
                .map(|cell| if cell == "-" { "" } else { cell })
                .collect::<Vec<_>>();
            assert_eq!(cells.len(), 6, "{name}: {line}");
            if !level.advanced() && cells[3] == "n/a" {
                return None;
            }
            let (imports, definitions) = if level.advanced() {
                (cells[4], cells[5])
            } else {
                (cells[3], "")
            };
            Some(format!(
                "{} | {} | roots {} | imports {imports} | definitions {definitions}",
                cells[0], cells[1], cells[2]
            ))
        })
        .collect()
}

/// Fixture `name` at every level: `--out-dir` writes its chunks and
/// `chunks.json`, which [`check_manifest`] checks against the files and
/// `membership.tsv` describes; run by the Node driver, the chunks print
/// `stdout.txt` and return nil, as the single `--esm` file does; and Node
/// links every chunk, which fails on an import of a missing export. The
/// planner's own invariants (`check`) hold of the program at every level.
fn matrix(name: &str) {
    let expected = fs::read_to_string(fixture_dir(name).join("stdout.txt")).unwrap();
    let root = fixture_root(name);
    let root = root.to_str().unwrap();
    plans(&fixture_program(name));
    let dir = TempDir::new("chunks-matrix");
    for level in OptimizationLevel::ALL {
        let base = [
            "--compile",
            "--optimize",
            level.name(),
            "--entry",
            "app.main",
            "--source-root",
            root,
        ];
        let (success, single, stderr) = cli(&[&base[..], &["--esm"]].concat());
        assert!(success && stderr.is_empty(), "{name} {level}: {stderr}");
        let oracle_dir = dir.join(level.name()).join("single");
        fs::create_dir_all(&oracle_dir).unwrap();
        fs::write(oracle_dir.join("app.main.js"), single).unwrap();
        install_packages(name, &oracle_dir);
        let oracle = run_directory(&oracle_dir, "app.main.js")
            .unwrap_or_else(|error| panic!("{name} {level}: {error}"));

        let out = dir.join(level.name()).join("chunks");
        let args = [&base[..], &["--out-dir", out.to_str().unwrap()]].concat();
        assert_eq!(
            cli(&args),
            (true, String::new(), String::new()),
            "{name} {level}"
        );
        let (startup, chunks) = read_manifest(&out);
        check_manifest(&out, &startup, &chunks, level, "app.main");
        assert_eq!(
            membership_rows(&chunks),
            expected_membership(name, level),
            "{name} {level}"
        );

        let files = || {
            let files = chunks
                .iter()
                .map(|chunk| {
                    let source = fs::read_to_string(out.join(&chunk.file)).unwrap();
                    (chunk.file.clone(), source)
                })
                .collect::<Vec<_>>();
            shown(&files)
        };
        install_packages(name, &out);
        let record = run_directory(&out, &startup)
            .unwrap_or_else(|error| panic!("{name} {level}: {error}\n{}", files()));
        assert_eq!(record, oracle, "{name} {level}:\n{}", files());
        let record = Json::parse(&record);
        assert_eq!(
            record.get("outcome"),
            &Json::parse(r#"{"status": "returned", "value": null}"#),
            "{name} {level}"
        );
        assert_eq!(record.get("stdout").str(), expected, "{name} {level}");

        let link = chunks
            .iter()
            .map(|chunk| format!("import \"./{}\";\n", chunk.file))
            .collect::<String>();
        fs::write(out.join("link-every-chunk.js"), link).unwrap();
        let linked = run_directory(&out, "link-every-chunk.js")
            .unwrap_or_else(|error| panic!("{name} {level}: {error}"));
        assert!(
            linked.contains(r#""outcome":{"status":"returned""#),
            "{name} {level}: {linked}"
        );
    }
}
