//! Completed namespaces captured as self-contained artifacts where their
//! analysis returns (docs/compiler-cache.md, "Binary artifacts").
use crate::temp_dir::TempDir;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::{
    analyze_namespace_entry_with_artifacts, analyze_namespace_entry_with_sources,
};
use light_cljs::namespace_artifact::*;
use light_cljs::{
    ApplicationOptions, CompileStats, MacroHost, OptimizationLevel, prepare_application,
};
use std::fs;

struct Sources(TempDir);

impl Sources {
    fn new(files: &[(&str, &str)]) -> Self {
        let sources = Self(TempDir::new("cljs-artifacts"));
        for (path, source) in files {
            let path = sources.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        sources
    }

    /// Every namespace's artifact, in source order, after checking that
    /// capture changed neither the program nor its warnings.
    fn capture(&self, entry: &str, options: AnalysisOptions) -> Vec<NamespaceArtifact> {
        let roots = std::slice::from_ref(self.0.path());
        let mut plain_warnings = Vec::new();
        let (plain, _) = analyze_namespace_entry_with_sources(
            entry,
            roots,
            &mut MacroHost::new(),
            options,
            &mut plain_warnings,
        )
        .unwrap();
        let mut captures = Vec::new();
        let mut warnings = Vec::new();
        let mut stats = CompileStats::new();
        let (program, _) = analyze_namespace_entry_with_artifacts(
            entry,
            roots,
            &mut MacroHost::new(),
            options,
            &mut warnings,
            Some(&mut stats),
            &mut |capture| captures.push(capture),
        )
        .unwrap();
        assert_eq!(warnings, plain_warnings);
        for level in [OptimizationLevel::None, OptimizationLevel::Advanced] {
            assert_eq!(
                prepare_application(&program, ApplicationOptions::new(level)).emit_js(),
                prepare_application(&plain, ApplicationOptions::new(level)).emit_js(),
                "capture must not change the output at {level:?}"
            );
        }
        let counts = stats.capture.expect("capture counts");
        assert_eq!(counts.captured, captures.len());
        assert_eq!(counts.skipped, 0);
        assert!(counts.bytes > 0);
        let report = stats.report();
        assert!(report.contains(&format!("\ncache.capture.namespaces {}\n", captures.len())));
        captures
            .into_iter()
            .map(|capture| {
                let artifact = capture
                    .artifact
                    .unwrap_or_else(|reason| panic!("{}: {reason}", capture.namespace));
                assert_eq!(artifact.header.namespace, capture.namespace);
                artifact.validate().unwrap();
                artifact
            })
            .collect()
    }
}

fn find<'a>(artifacts: &'a [NamespaceArtifact], namespace: &str) -> &'a NamespaceArtifact {
    artifacts
        .iter()
        .find(|artifact| artifact.header.namespace == namespace)
        .unwrap_or_else(|| panic!("no artifact for {namespace}"))
}

fn text(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// The keyword `name` of a map datum, if present.
fn entry<'a>(map: &'a Datum, namespace: Option<&str>, name: &str) -> Option<&'a Datum> {
    let DatumKind::Collection {
        kind: CollectionKind::Map,
        children,
    } = &map.kind
    else {
        return None;
    };
    children.chunks(2).find_map(|pair| match &pair[0].kind {
        DatumKind::Keyword(ns, key)
            if ns.as_deref().map(text).as_deref() == namespace && text(key) == name =>
        {
            Some(&pair[1])
        }
        _ => None,
    })
}

#[test]
fn artifact_types_name_no_arena_ids_handles_or_shared_pointers() {
    // By construction: the artifact's types live in one module, which takes
    // only plain value enums from the compiler and spells no program ID,
    // source index, unit ID, reference count or engine handle.
    let source = include_str!("../../src/namespace_artifact.rs");
    let code = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "ExprId",
        "GlobalId",
        "LocalId",
        "StringId",
        "NamedLiteralId",
        "JsGlobalId",
        "RecurTargetId",
        "LazyTargetId",
        "FormId",
        "SourceId",
        "ValueId",
        "source_index",
        "Rc<",
        "Arc<",
        "Weak<",
        "Shared<",
        "Arena<",
        "Layered<",
        "Program",
        "Form",
        "SourceSpan",
        "Evaluator",
        "MacroHost",
        "&'static",
    ] {
        assert!(
            !code.contains(forbidden),
            "namespace_artifact.rs names {forbidden}"
        );
    }
    let imports = code
        .lines()
        .filter(|line| line.contains("crate::"))
        .map(str::trim)
        .collect::<Vec<_>>();
    assert_eq!(
        imports,
        [
            "pub use crate::analyzer::{CoreFnId, ExprContext, MethodDispatch};",
            "pub use crate::compiler_data::CompilerLiteralKind;",
            "pub use crate::parser::CollectionKind;",
        ]
    );
}

#[test]
fn macros_carry_their_generation_metadata_code_and_definition_site() {
    let sources = Sources::new(&[
        (
            "lib/macros.cljs",
            "(ns lib.macros)\n\
             (defn helper [x] (list 'inc x))\n\
             (defmacro twice [x] (list 'do (helper x) (helper x)))\n\
             (defmacro twice \"Twice the increment.\" [x] (list '* 2 (helper x)))\n\
             (def forty-two (twice 20))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [lib.macros :refer [twice]]))\n(js/console.log (twice 20))\n",
        ),
    ]);
    let artifacts = sources.capture("app", AnalysisOptions::default());
    assert_eq!(
        artifacts
            .iter()
            .map(|artifact| artifact.header.namespace.as_str())
            .collect::<Vec<_>>(),
        ["lib.macros", "app"]
    );
    let macros = find(&artifacts, "lib.macros");
    let [twice] = macros.declarations.macros.as_slice() else {
        panic!(
            "one macro, the later twice: {:?}",
            macros.declarations.macros
        )
    };
    assert_eq!(twice.name, "twice");
    assert_eq!(twice.generation, 2);
    let generation = entry(&twice.metadata, Some("light-cljs"), "generation").unwrap();
    assert_eq!(generation.kind, DatumKind::Integer(2));
    assert_eq!(
        entry(&twice.metadata, None, "macro").unwrap().kind,
        DatumKind::Boolean(true)
    );
    assert_eq!(
        entry(&twice.metadata, None, "doc").unwrap().kind,
        DatumKind::String("Twice the increment.".encode_utf16().collect())
    );
    assert!(twice.definition.source.ends_with("lib/macros.cljs"));
    let site = twice.definition.bytes;
    let source = fs::read_to_string(sources.0.join("lib/macros.cljs")).unwrap();
    assert!(source[site.0 as usize..site.1 as usize].starts_with("(defmacro twice \"Twice"));
    assert!(!twice.code.text.is_empty());
    // The helper is a deferred unit forty-two's expansion ran.
    let helper = macros
        .units
        .units
        .iter()
        .find(|unit| unit.name == UnitName::Var("lib.macros/helper".into()))
        .expect("helper's unit");
    assert!(helper.emitted && helper.analyses == 1 && !helper.code.text.is_empty());
    let global = |name: &str| {
        macros
            .declarations
            .globals
            .iter()
            .find(|global| global.name == name)
            .unwrap()
    };
    assert_eq!(global("helper").private, Some(false));
    assert_eq!(global("helper").binding, Some(BindingKind::Value));
    assert_eq!(
        macros.declarations.reader_context.macros,
        ["twice".to_owned()]
    );
    // The application never sees a macro: app expanded twice into its IR.
    let app = find(&artifacts, "app");
    assert!(app.declarations.macros.is_empty());
    assert_eq!(
        app.declarations.refers,
        [("twice".into(), "lib.macros".into(), "twice".into())]
    );
    let described = macros.describe();
    for section in [
        "[header]",
        "[declarations]",
        "[ir]",
        "[patches]",
        "[deferred-units]",
        "[diagnostics]",
        "[references]",
        "[names]\nnot recorded",
        "[accounting]\nnot recorded",
    ] {
        assert!(described.contains(section), "{section} in\n{described}");
    }
    assert!(
        described.contains("macro twice generation 2"),
        "{described}"
    );
}

#[test]
fn helpers_record_prepared_units_bundles_and_definition_frontiers() {
    let sources = Sources::new(&[
        (
            "lib/helpers.cljs",
            "(ns lib.helpers)\n\
             (defprotocol Shape (area [s]))\n\
             (extend-type number Shape (area [n] (* n n)))\n\
             (defmulti describe :kind)\n\
             (defmethod describe :square [s] (area (:side s)))\n\
             (defn label [s] (str \"area \" (describe s)))\n\
             (defmacro static-label [side] (label {:kind :square :side side}))\n\
             (def nine (static-label 3))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [lib.helpers :as h]))\n\
             (js/console.log h/nine (h/static-label 3) (h/label {:kind :square :side 4}))\n",
        ),
    ]);
    let artifacts = sources.capture("app", AnalysisOptions::default());
    let helpers = find(&artifacts, "lib.helpers");
    let units = &helpers.units;
    assert!(!units.units.is_empty());
    for unit in &units.units {
        assert!(!unit.code.text.is_empty(), "{unit:?}");
        assert!(unit.view.own_globals as usize <= helpers.declarations.globals.len());
    }
    let bundles = units
        .registrations
        .iter()
        .filter_map(|registration| match registration {
            Registration::Bundle {
                products,
                unit,
                extensions,
                ..
            } => Some((products, *unit, extensions)),
            _ => None,
        })
        .collect::<Vec<_>>();
    // defprotocol, extend-type and defmethod each register a bundle.
    assert!(
        bundles
            .iter()
            .any(|(products, _, _)| products.contains(&"lib.helpers/Shape".to_owned())),
        "{bundles:?}"
    );
    assert!(bundles.iter().any(|(_, _, extensions)| {
        extensions
            .iter()
            .any(|extension| extension.protocol.ends_with("Shape"))
    }));
    let method = bundles
        .iter()
        .flat_map(|(_, _, extensions)| extensions.iter())
        .find_map(|extension| match extension.identity {
            ExtensionIdentity::Method(offset) => Some(offset),
            _ => None,
        })
        .expect("the defmethod's extension, by offset in its own source");
    let source = fs::read_to_string(sources.0.join("lib/helpers.cljs")).unwrap();
    assert!(source[method as usize..].starts_with("(defmethod describe"));
    for (_, unit, _) in &bundles {
        assert_eq!(
            units.units[unit.0 as usize].name,
            UnitName::Declaration,
            "bundle units are numbered by unit ID, so the name is not kept"
        );
    }
    // Each unit's frontier counts the bundles registered before it here.
    let frontiers = units
        .units
        .iter()
        .map(|unit| unit.extension_frontier)
        .collect::<Vec<_>>();
    assert!(frontiers.windows(2).all(|w| w[0] <= w[1]), "{frontiers:?}");
    assert!(*frontiers.last().unwrap() > 0);
    // nine's expansion ran label, which ran the bundles' code.
    let label = units
        .units
        .iter()
        .find(|unit| unit.name == UnitName::Var("lib.helpers/label".into()))
        .unwrap();
    assert!(label.emitted);
    assert!(
        units
            .registrations
            .iter()
            .any(|registration| matches!(registration,
                Registration::Root { name, unit: Some(_), .. } if name == "lib.helpers/label"))
    );
    // app refers to the helpers by identity, not by ID.
    let app = find(&artifacts, "app");
    assert!(app.references.globals.iter().any(|global| {
        global.namespace == "lib.helpers"
            && global.name == "label"
            && global.declared
            && global.private == Some(false)
    }));
    assert!(app.units.units.is_empty() || app.units.units.iter().all(|u| !u.code.text.is_empty()));
}

#[test]
fn protocols_and_records_record_their_tables() {
    let sources = Sources::new(&[
        (
            "geo/shapes.cljs",
            "(ns geo.shapes)\n\
             (defprotocol Area (area [s]) (scaled [s k]))\n\
             (defrecord Square [side] Area (area [_] (* side side)) (scaled [_ k] (->Square (* k side))))\n\
             (deftype Box [^:mutable w] Object (toString [_] (str \"box \" w)))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [geo.shapes :as s]))\n\
             (js/console.log (s/area (s/scaled (s/->Square 2) 3)) (str (s/Box. 1)))\n",
        ),
    ]);
    let artifacts = sources.capture("app", AnalysisOptions::default());
    let shapes = find(&artifacts, "geo.shapes");
    let d = &shapes.declarations;
    let own = |name: &str| {
        GlobalRef::Own(
            d.globals
                .iter()
                .position(|global| global.name == name)
                .unwrap_or_else(|| panic!("{name}")) as u32,
        )
    };
    let [(protocol, methods)] = d.protocols.as_slice() else {
        panic!("{:?}", d.protocols)
    };
    assert_eq!(*protocol, own("Area"));
    assert_eq!(
        methods
            .iter()
            .map(|m| m.key.as_str())
            .collect::<Vec<_>>()
            .len(),
        2
    );
    assert!(d.record_constructors.contains(&own("Square")));
    assert!(!d.field_bindings.is_empty());
    assert!(d.field_bindings.iter().any(|(_, _, mutable)| *mutable));
    assert!(!d.type_methods.is_empty());
    assert!(d.globals[0..].iter().any(|g| g.flags.type_descriptor));
    assert!(shapes.ir.exprs.iter().any(|expr| matches!(
        &expr.kind,
        ExprKind::DefType { record: true, layout, .. } if layout.fields == ["side"]
    )));
    let app = find(&artifacts, "app");
    assert!(
        app.references
            .globals
            .iter()
            .any(|global| global.namespace == "geo.shapes" && global.name == "->Square")
    );
}

#[test]
fn lazy_targets_and_patches_of_an_earlier_namespace_global() {
    let sources = Sources::new(&[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def editor (lazy/loadable app.editor/start))\n\
             (fn [] editor)\n",
        ),
        (
            "app/editor.cljs",
            "(ns app.editor)\n(defn ^{:doc \"Start.\"} start [] :editing)\n",
        ),
    ]);
    let artifacts = sources.capture("app.main", AnalysisOptions::default());
    let order = artifacts
        .iter()
        .map(|artifact| artifact.header.namespace.as_str())
        .collect::<Vec<_>>();
    assert_eq!(order.last(), Some(&"app.editor"), "{order:?}");
    let main = find(&artifacts, "app.main");
    assert!(main.header.entry);
    let [target] = main.declarations.lazy_targets.as_slice() else {
        panic!("{:?}", main.declarations.lazy_targets)
    };
    assert_eq!(target.namespace, "app.editor");
    assert!(matches!(
        main.ir.exprs[target.site.0 as usize].kind,
        ExprKind::Loadable {
            target: LazyRef(0),
            ..
        }
    ));
    // app.main's reference reserved app.editor/start before app.editor ran.
    let Some(GlobalRef::Own(reserved)) = target.global else {
        panic!("{:?}", target.global)
    };
    let reserved = &main.declarations.globals[reserved as usize];
    assert_eq!(
        (reserved.namespace.as_str(), reserved.name.as_str()),
        ("app.editor", "start")
    );
    assert!(!reserved.flags.declared);
    // Defining it patches that earlier global: the editor's artifact records
    // the change by identity, with metadata from its own IR.
    let editor = find(&artifacts, "app.editor");
    assert!(!editor.header.entry);
    let [patch] = editor.patches.as_slice() else {
        panic!("{:?}", editor.patches)
    };
    let GlobalRef::External(index) = patch.global else {
        panic!("{patch:?}")
    };
    let external = &editor.references.globals[index as usize];
    assert_eq!(
        (
            external.namespace.as_str(),
            external.name.as_str(),
            external.declared
        ),
        ("app.editor", "start", true)
    );
    assert!(patch.flags.declared);
    let metadata = patch.metadata.expect("start's metadata");
    assert!((metadata.0 as usize) < editor.ir.exprs.len());
    assert!(editor.describe().contains("patch External(0)"));
}

#[test]
fn warnings_keep_their_order_spans_and_sources() {
    let sources = Sources::new(&[
        (
            "lib/a.cljs",
            "(ns lib.a)\n(defn f [x] (+ x undeclared-one))\n(defn g [] (f undeclared-two))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [lib.a :as a]))\n(a/f missing-in-app)\n(a/g)\n",
        ),
    ]);
    let options = AnalysisOptions {
        warning_mode: true,
        ..AnalysisOptions::default()
    };
    let mut expected = Vec::new();
    analyze_namespace_entry_with_sources(
        "app",
        std::slice::from_ref(sources.0.path()),
        &mut MacroHost::new(),
        options,
        &mut expected,
    )
    .unwrap();
    let artifacts = sources.capture("app", options);
    let captured = artifacts
        .iter()
        .flat_map(|artifact| {
            artifact.diagnostics.warnings.iter().map(|warning| {
                (
                    warning.kind.clone(),
                    warning.message.clone(),
                    warning.span,
                    artifact.header.namespace.clone(),
                    warning.source,
                )
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(captured.len(), 3, "{captured:?}");
    assert_eq!(
        captured
            .iter()
            .map(|(kind, message, span, _, _)| (kind.as_str(), message.as_str(), *span))
            .collect::<Vec<_>>(),
        expected
            .iter()
            // The loader reports the `ns` forms' warnings; analysis, the rest.
            .filter(|w| w.kind != "single-segment-namespace")
            .map(|w| (
                w.kind,
                w.message.as_str(),
                (w.span.start as u32, w.span.end as u32)
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        captured
            .iter()
            .all(|(_, _, _, _, source)| *source == SourceRef::Own)
    );
    let lib = find(&artifacts, "lib.a");
    assert!(
        lib.diagnostics
            .warnings
            .iter()
            .any(|w| w.message.contains("undeclared-one"))
    );
}

#[test]
fn every_namespace_of_a_program_is_captured_in_source_order() {
    let sources = Sources::new(&[
        ("a/b.cljs", "(ns a.b)\n(def x 1)\n"),
        (
            "a/c.cljs",
            "(ns a.c (:require [a.b :as b]))\n(def y (inc b/x))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [a.c :as c] [a.b :as b]))\n(+ c/y b/x)\n",
        ),
    ]);
    let artifacts = sources.capture("app", AnalysisOptions::default());
    assert_eq!(
        artifacts
            .iter()
            .map(|a| a.header.namespace.as_str())
            .collect::<Vec<_>>(),
        ["a.b", "a.c", "app"]
    );
    let c = find(&artifacts, "a.c");
    assert_eq!(c.declarations.requires, ["a.b"]);
    assert_eq!(c.declarations.aliases, [("b".to_owned(), "a.b".to_owned())]);
    assert_eq!(c.references.globals.len(), 1);
    assert!(c.patches.is_empty() && c.units.units.iter().all(|u| !u.code.text.is_empty()));
    assert!(c.header.path.ends_with("a/c.cljs"));
}

#[test]
fn a_var_form_shares_an_earlier_global_metadata_by_identity() {
    let sources = Sources::new(&[
        ("lib/a.cljs", "(ns lib.a)\n(defn f \"Doc of f.\" [] 1)\n"),
        (
            "app.cljs",
            "(ns app (:require [lib.a :as a]))\n(js/console.log (:doc (meta #'a/f)) (:doc (meta (var a/f))))\n",
        ),
    ]);
    let artifacts = sources.capture("app", AnalysisOptions::default());
    let app = find(&artifacts, "app");
    let shared = app
        .ir
        .exprs
        .iter()
        .filter_map(|expr| match &expr.kind {
            ExprKind::Var {
                metadata: Some(VarMetadata::Global(GlobalRef::External(index))),
                ..
            } => Some(&app.references.globals[*index as usize]),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(shared.len(), 2, "{}", app.describe());
    assert!(
        shared
            .iter()
            .all(|g| g.namespace == "lib.a" && g.name == "f")
    );
}

/// Every namespace's capture, artifacts and refusals alike, in source order.
fn raw_captures(sources: &Sources, entry: &str) -> Vec<NamespaceCapture> {
    let mut captures = Vec::new();
    analyze_namespace_entry_with_artifacts(
        entry,
        std::slice::from_ref(sources.0.path()),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
        None,
        &mut |capture| captures.push(capture),
    )
    .unwrap();
    captures
}

#[test]
fn unit_relocations_ignore_literal_text() {
    let sources = Sources::new(&[
        (
            "lib/tested.cljs",
            "(ns lib.tested)\n\
             (def s \"globalThis.__lightCompilerTest(0)\")\n\
             (def t \"globalThis.__lightCompilerTest(99999)\")\n\
             (defn f {:test (fn [] :checked)} [] s)\n\
             (defmacro run-test [] ((:test (meta (var f)))))\n\
             (def checked (run-test))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [lib.tested :as t]))\n(js/console.log t/s t/t t/checked)\n",
        ),
    ]);
    let captures = raw_captures(&sources, "app");
    let tested = captures
        .iter()
        .find(|capture| capture.namespace == "lib.tested")
        .unwrap();
    let artifact = match &tested.artifact {
        Ok(artifact) => artifact,
        Err(reason) => panic!("{reason}"),
    };
    artifact.validate().unwrap();
    let mut literal = 0;
    let mut references = Vec::new();
    for code in artifact
        .units
        .units
        .iter()
        .map(|unit| &unit.code)
        .chain(artifact.declarations.macros.iter().map(|m| &m.code))
    {
        literal += code.text.matches("__lightCompilerTest(0)").count()
            + code.text.matches("__lightCompilerTest(99999)").count();
        for &((start, end), unit) in &code.units {
            let id = &code.text[start as usize..end as usize];
            assert!(id.bytes().all(|b| b.is_ascii_digit()), "{id}");
            assert!(code.text[..start as usize].ends_with("globalThis.__lightCompilerTest("));
            references.push(unit);
        }
    }
    assert!(literal >= 2, "the strings are in the compile-time code");
    // Only the call the test metadata compiled to names a unit: its own.
    assert!(!references.is_empty(), "{}", artifact.describe());
    for unit in references {
        assert!(matches!(
            artifact.units.units[unit.0 as usize].name,
            UnitName::Test(ref owner) if owner == "lib.tested/f"
        ));
    }
}

#[test]
fn nested_macro_expansion_origins_are_captured() {
    let sources = Sources::new(&[
        (
            "lib/inner.cljs",
            "(ns lib.inner)\n(defmacro inner [x] (list 'inc x))\n",
        ),
        (
            "lib/outer.cljs",
            "(ns lib.outer (:require [lib.inner]))\n(defmacro outer [x] (list 'lib.inner/inner x))\n",
        ),
        (
            "app.cljs",
            "(ns app (:require [lib.outer :refer [outer]]))\n(js/console.log (outer 41))\n",
        ),
    ]);
    let artifacts = sources.capture("app", AnalysisOptions::default());
    let app = find(&artifacts, "app");
    let expansions = app
        .diagnostics
        .expansions
        .iter()
        .map(|e| {
            (
                e.macro_namespace.as_str(),
                e.name.as_str(),
                e.generation,
                e.source,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        expansions,
        [
            ("lib.outer", "outer", 1, SourceRef::Own),
            ("lib.inner", "inner", 1, SourceRef::Own)
        ]
    );
    let source = fs::read_to_string(sources.0.join("app.cljs")).unwrap();
    let outer = &app.diagnostics.expansions[0];
    assert_eq!(
        &source[outer.call.0 as usize..outer.call.1 as usize],
        "(outer 41)"
    );
    assert!(outer.definition.source.ends_with("lib/outer.cljs"));
    let inner = &app.diagnostics.expansions[1];
    assert!(inner.definition.source.ends_with("lib/inner.cljs"));
    assert!(
        app.describe()
            .contains("expansion lib.outer/outer generation 1")
    );
    // The defining namespaces expanded nothing of their own.
    assert!(
        find(&artifacts, "lib.outer")
            .diagnostics
            .expansions
            .is_empty()
    );
}

#[test]
fn lazy_targets_capture_reserved_protocols_and_records() {
    let sources = Sources::new(&[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy]))\n\
             (def p (lazy/loadable app.types/P))\n\
             (def r (lazy/loadable app.types/R))\n\
             (fn [] [p r])\n",
        ),
        (
            "app/types.cljs",
            "(ns app.types)\n(defprotocol P (f [x]))\n(defrecord R [a] P (f [_] a))\n",
        ),
    ]);
    let artifacts = sources.capture("app.main", AnalysisOptions::default());
    let types = find(&artifacts, "app.types");
    let external = |name: &str| {
        GlobalRef::External(
            types
                .references
                .globals
                .iter()
                .position(|g| g.namespace == "app.types" && g.name == name)
                .unwrap_or_else(|| panic!("{name} in {}", types.describe())) as u32,
        )
    };
    let protocol = external("P");
    assert!(
        types
            .declarations
            .protocols
            .iter()
            .any(|(global, methods)| *global == protocol && methods.len() == 1),
        "{}",
        types.describe()
    );
    for name in ["P", "R"] {
        let global = external(name);
        assert!(
            types
                .patches
                .iter()
                .any(|patch| patch.global == global && patch.flags.declared),
            "{name}: {}",
            types.describe()
        );
    }
    assert!(
        types
            .declarations
            .record_constructors
            .contains(&external("R")),
        "{}",
        types.describe()
    );
}
