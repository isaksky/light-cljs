//! The namespace-level chunk planner, `light_cljs::chunks::plan`
//! (docs/code-splitting.md, "The model" and "Phase 2"), at every level, each
//! plan checked against the invariants the chunked emitter relies on
//! (`check`); and the chunk files `PreparedApplication::emit_chunks` writes,
//! run in Node against the single `--esm` file (`runs_as_chunks`).
use crate::chunked_execution::run_modules;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::chunks::{ChunkId, ChunkPlan, ChunkedOutput, STARTUP, plan};
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

// ---------------------------------------------------------------------------
// Chunked output: `PreparedApplication::emit_chunks` (light-cljs-274).

/// Run `program` at every level as chunks and as one `--esm` file, its
/// oracle, through the Node driver, and require the same record; link every
/// chunk in Node, where a missing export is a link error; and check that
/// each name a chunk imports is one the chunk it names exports. Returns the
/// record, the same at every level, and the files at each level.
fn runs_as_chunks(program: &Program) -> (String, Vec<(OptimizationLevel, ChunkedOutput)>) {
    runs_as_chunks_beside(program, &[])
}

/// [`runs_as_chunks`] with `packages`, files the modules import beside
/// them (`node_modules/...`).
fn runs_as_chunks_beside(
    program: &Program,
    packages: &[(&str, &str)],
) -> (String, Vec<(OptimizationLevel, ChunkedOutput)>) {
    let mut records = Vec::new();
    let mut outputs = Vec::new();
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(program, ApplicationOptions::new(level));
        let output = prepared.emit_chunks();
        let startup = output.files[0].name.as_str();
        let single = prepared.emit_esm();
        let mut oracle_files = vec![(startup, single.as_str())];
        oracle_files.extend_from_slice(packages);
        let oracle =
            run_modules(&oracle_files, startup).unwrap_or_else(|error| panic!("{level}: {error}"));
        let mut files = output
            .files
            .iter()
            .map(|file| (file.name.as_str(), file.source.as_str()))
            .collect::<Vec<_>>();
        files.extend_from_slice(packages);
        // The files for a failure message, without the runtime's text, which
        // is the longest line of a terse startup chunk and its own lines in
        // a readable one.
        let listing = files
            .iter()
            .map(|(name, source)| {
                let shown = source
                    .lines()
                    .filter(|line| line.len() < 2_000 && !line.starts_with("    "))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("// {name}\n{shown}\n")
            })
            .collect::<String>();
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

#[test]
fn the_features_example_runs_as_three_chunks_at_every_level() {
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
    let (record, outputs) = runs_as_chunks(&program);
    assert_eq!(record, crate::features_example::FEATURES_RECORD);
    for (level, output) in outputs {
        assert_eq!(
            files(&output),
            [
                "app.main.js: app.shared light.lazy app.main | roots app.main | imports ",
                "app.editor.js: app.editor | roots app.editor | imports app.main.js",
                "app.reports.js: clojure.string app.reports | roots app.reports | imports app.main.js",
            ],
            "{level}"
        );
        // Each loadable imports its feature's chunk.
        let startup = &output.files[0].source;
        for feature in ["app.editor.js", "app.reports.js"] {
            assert!(
                startup.contains(&format!("import(\"./{feature}\")")),
                "{level}: {}",
                startup
                    .lines()
                    .filter(|line| line.contains("import"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
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
/// where `Advanced` code reads nothing of a chunk.
fn chunk_files(program: &Program) -> (String, Vec<String>) {
    let (record, outputs) = runs_as_chunks(program);
    let members = |output: &ChunkedOutput| {
        files(output)
            .into_iter()
            .map(|file| file.split(" | imports").next().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    for (level, output) in &outputs {
        assert_eq!(members(output), members(&outputs[0].1), "{level}");
    }
    (record, files(&outputs[0].1))
}

/// Two features share a helper with a top-level effect, which runs once,
/// when the first of them loads, as it runs once at startup in the single
/// file: before the default export is called.
#[test]
fn features_sharing_a_helper_load_its_chunk_once() {
    static DIAMOND: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (def reports (lazy/loadable app.reports/start))\n\
             (defn start []\n\
               (-> (lazy/load editor)\n\
                   (.then (fn [f] (js/console.log (f)) (lazy/load reports)))\n\
                   (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
        ),
        (
            "app/helper.cljs",
            "(ns app.helper)\n\
             (js/console.log \"helper loads\")\n\
             (def calls (atom 0))\n\
             (defn shout [s] (swap! calls inc) (str s \"!\" @calls))\n",
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
    let (record, files) = chunk_files(&analyze(DIAMOND));
    assert_eq!(
        record,
        printed("app.main.js", r"helper loads\neditor!1\nreports!2\n")
    );
    assert_eq!(
        files,
        [
            "app.main.js: light.lazy app.main | roots app.main | imports ",
            "app.editor+app.reports.js: app.helper | roots  | imports app.main.js",
            "app.editor.js: app.editor | roots app.editor | imports app.main.js app.editor+app.reports.js",
            "app.reports.js: app.reports | roots app.reports | imports app.main.js app.editor+app.reports.js",
        ]
    );
}

/// A feature loads a sub-feature, which requires it back: the loadable in
/// the feature's chunk imports the sub-feature's chunk.
#[test]
fn a_feature_loads_its_sub_feature_chunk() {
    static CHAIN: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (defn start []\n\
               (-> (lazy/load editor)\n\
                   (.then (fn [f] (f)))\n\
                   (.then (fn [text] (js/console.log text) nil))))\n\
             start\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.text :as text] [light.lazy :as lazy]))\n\
             (def spelling (lazy/loadable app.spell/check))\n\
             (defn start []\n\
               (.then (lazy/load spelling) (fn [check] (str (text/greet) \", \" (check)))))\n",
        ),
        ("app/text.cljs", "(ns app.text)\n(defn greet [] \"hi\")\n"),
        (
            "app/spell.cljs",
            "(ns app.spell (:require [app.editor :as editor] [app.text :as text]))\n\
             (defn check [] (str \"spelled \" (text/greet) \" \" (fn? editor/start)))\n",
        ),
    ];
    let (record, files) = chunk_files(&analyze(CHAIN));
    assert_eq!(record, printed("app.main.js", r"hi, spelled hi true\n"));
    assert_eq!(
        files,
        [
            "app.main.js: light.lazy app.main | roots app.main | imports ",
            "app.editor+app.spell.js: app.text app.editor | roots app.editor | imports app.main.js",
            "app.spell.js: app.spell | roots app.spell | imports app.main.js app.editor+app.spell.js",
        ]
    );
}

/// A lazy target that requires the loading namespace reads startup's
/// definitions through imports, and a loadable of a namespace startup
/// requires is resident: no import.
#[test]
fn a_feature_reads_the_namespace_that_loads_it() {
    static BACK: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main\n  (:require [app.shared :as shared] [app.util :as util] [light.lazy :as lazy]))\n\
             (def editor (light.lazy/loadable app.editor/start))\n\
             (def twice (light.lazy/loadable app.util/twice))\n\
             (defn ready [] (shared/label :main))\n\
             (defn start []\n\
               (-> (lazy/load twice)\n\
                   (.then (fn [f] (js/console.log (f \"ab\")) (lazy/load editor)))\n\
                   (.then (fn [f] (js/console.log (f)) nil))))\n\
             start\n",
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
             (defn start [] (str (main/ready) \" \" text/greeting))\n",
        ),
        ("app/text.cljs", "(ns app.text)\n(def greeting \"hi\")\n"),
    ];
    let (record, files) = chunk_files(&analyze(BACK));
    assert_eq!(record, printed("app.main.js", r"abab\nlabel main hi\n"));
    assert_eq!(
        files,
        [
            "app.main.js: app.shared app.util light.lazy app.main | roots app.main app.util | imports ",
            "app.editor.js: app.text app.editor | roots app.editor | imports app.main.js",
        ]
    );
    let program = analyze(BACK);
    for level in OptimizationLevel::ALL {
        let output = prepare_application(&program, ApplicationOptions::new(level)).emit_chunks();
        assert_eq!(
            output.files[0].source.matches("import(").count(),
            1,
            "{level}: only the editor's loadable imports"
        );
    }
}

/// `set!`, `binding` and `with-redefs` of another chunk's Var from a
/// feature go through the setters its owner exports; the feature's own
/// assignments stay direct.
#[test]
fn a_feature_assigns_startup_vars_through_their_setters() {
    static SETTERS: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.state :as state] [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (defn show [] [state/mode state/*depth* (state/fixed-value)])\n\
             (defn start []\n\
               (-> (lazy/load editor)\n\
                   (.then (fn [f]\n\
                            (js/console.log (pr-str (f)) (pr-str (show)) (pr-str (f)))\n\
                            nil))))\n\
             start\n",
        ),
        (
            "app/state.cljs",
            "(ns app.state)\n\
             (def mode :plain)\n\
             (def ^:dynamic *depth* 0)\n\
             (def fixed 1)\n\
             (defn depth [] *depth*)\n\
             (defn fixed-value [] fixed)\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [app.state :as state]))\n\
             (def edits 0)\n\
             (defn start []\n\
               (set! state/mode :edit)\n\
               (set! edits (inc edits))\n\
               (binding [state/*depth* edits]\n\
                 (with-redefs [state/fixed 2]\n\
                   [state/mode (state/depth) (state/fixed-value) edits])))\n",
        ),
    ];
    let (record, files) = chunk_files(&analyze(SETTERS));
    assert_eq!(
        record,
        printed("app.main.js", r"[:edit 1 2 1] [:edit 0 1] [:edit 2 2 2]\n")
    );
    assert_eq!(
        files,
        [
            "app.main.js: app.state light.lazy app.main | roots app.main | imports ",
            "app.editor.js: app.editor | roots app.editor | imports app.main.js",
        ]
    );
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
/// startup loads its chunk and resolves to nil.
#[test]
fn a_namespace_loadable_runs_its_methods_when_loaded() {
    static METHODS: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (defmulti area :shape)\n\
             (defmethod area :default [_] :unknown)\n\
             (def shapes (lazy/loadable app.shapes))\n\
             (defn start []\n\
               (-> (lazy/load shapes)\n\
                   (.then (fn [v]\n\
                            (js/console.log (pr-str v) (area {:shape :square :side 3})\n\
                                            (pr-str (area {:shape :circle})))\n\
                            nil))))\n\
             start\n",
        ),
        (
            "app/shapes.cljs",
            "(ns app.shapes (:require [app.main :as main]))\n\
             (defmethod main/area :square [{:keys [side]}] (* side side))\n",
        ),
    ];
    let (record, files) = chunk_files(&analyze(METHODS));
    assert_eq!(record, printed("app.main.js", r"nil 9 :unknown\n"));
    assert_eq!(
        files,
        [
            "app.main.js: light.lazy app.main | roots app.main | imports ",
            "app.shapes.js: app.shapes | roots app.shapes | imports app.main.js",
        ]
    );
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
/// alone, under its terse name at `Advanced` too.
#[test]
fn a_package_only_a_feature_requires_is_imported_by_its_chunk() {
    static PACKAGE: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (defn start []\n\
               (-> (lazy/load editor) (.then (fn [f] (js/console.log (f 2)) nil))))\n\
             start\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor (:require [\"stub-package\" :as stub :refer [twice]]))\n\
             (defn start [x] (str (twice x) \" \" (.-label stub)))\n",
        ),
    ];
    let packages = [
        (
            "node_modules/stub-package/package.json",
            "{\"name\": \"stub-package\", \"type\": \"module\", \"main\": \"index.js\"}\n",
        ),
        (
            "node_modules/stub-package/index.js",
            "export const twice = (x) => 2 * x;\nexport const label = \"stub\";\n",
        ),
    ];
    let program = analyze(PACKAGE);
    let (record, outputs) = runs_as_chunks_beside(&program, &packages);
    assert_eq!(record, printed("app.main.js", r"4 stub\n"));
    for (level, output) in outputs {
        let importers = output
            .files
            .iter()
            .filter(|file| file.source.contains("from \"stub-package\""))
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(importers, ["app.editor.js"], "{level}");
    }
}
