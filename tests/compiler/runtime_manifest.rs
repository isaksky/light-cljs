//! The runtime unit manifest (src/runtime_manifest.rs) over the real runtime:
//! headers validate against the bodies, the units concatenate back to both the
//! annotated file and the embedded runtime, and every unit is reachable from
//! the export list.
//!
//! After editing src/js_runtime.js, rewrite every `requires:` list with
//! `LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST=1 cargo test --test compiler runtime_manifest::`
//! and review the header diff.

use light_cljs::evaluator::RUNTIME_SOURCE;
use light_cljs::runtime_manifest::{
    ANNOTATED_RUNTIME, ECMASCRIPT_GLOBALS, RUNTIME, RootUse, RuntimeManifest, UnitId,
    is_header_line,
};

fn manifest() -> &'static RuntimeManifest<'static> {
    RuntimeManifest::embedded()
}

#[test]
fn every_unit_declares_exactly_the_runtime_bindings_it_references() {
    let manifest = manifest();
    if std::env::var_os("LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST").is_some() {
        let updated = manifest.with_computed_requires();
        if updated != ANNOTATED_RUNTIME {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/js_runtime.js");
            std::fs::write(&path, &updated).unwrap();
        }
        let reparsed = RuntimeManifest::parse(&updated).unwrap();
        if let Err(errors) = reparsed.validate() {
            panic!(
                "requires were rewritten, but the manifest is still invalid:\n{}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        return;
    }
    if let Err(errors) = manifest.validate() {
        panic!(
            "the runtime manifest is out of date with src/js_runtime.js:\n{}\n\nFix the headers or run \
             LIGHT_CLJS_UPDATE_RUNTIME_MANIFEST=1 cargo test --test compiler runtime_manifest:: \
             and review the diff.",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

#[test]
fn hidden_edges_and_stale_requires_in_the_runtime_fail_validation() {
    let messages = |source: &str| {
        RuntimeManifest::parse(source)
            .unwrap()
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
    };
    let unit = "  //@unit transient requires: asTransientProtocol\n  function transient(value) { return asTransientProtocol(value); }\n";
    assert!(ANNOTATED_RUNTIME.contains(unit));
    // A new reference to the printer is a hidden edge until it is declared.
    let hidden = ANNOTATED_RUNTIME.replace(
        unit,
        &unit.replace(
            "return asTransientProtocol",
            "print; return asTransientProtocol",
        ),
    );
    assert_eq!(
        messages(&hidden),
        ["unit `transient`: references `print` (declared by unit `print`) but does not require it"]
    );
    // A local that shadows a runtime binding is not an edge.
    let shadowed = ANNOTATED_RUNTIME.replace(
        unit,
        &unit.replace(
            "return asTransientProtocol",
            "const print = 1; return print && asTransientProtocol",
        ),
    );
    RuntimeManifest::parse(&shadowed)
        .unwrap()
        .validate()
        .unwrap();
    // Dropping a reference leaves a stale require.
    let stale =
        ANNOTATED_RUNTIME.replace(unit, &unit.replace("asTransientProtocol(value)", "value"));
    assert_eq!(
        messages(&stale),
        ["unit `transient`: requires `asTransientProtocol` but never references it"]
    );
}

/// Load-time code that mutates state is an effect the header must declare:
/// top-level statements, and calls of a `mutator` (installing methods,
/// registering a core protocol dispatcher, setting a builtin type's prototype).
#[test]
fn undeclared_load_time_effects_fail_validation() {
    let messages = |source: &str| {
        RuntimeManifest::parse(source)
            .unwrap()
            .validate()
            .unwrap_err()
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
    };
    let header = "  //@unit write effect requires: coreMethod\n";
    assert!(ANNOTATED_RUNTIME.contains(header));
    let undeclared = ANNOTATED_RUNTIME.replace(header, &header.replace(" effect", ""));
    assert_eq!(
        messages(&undeclared),
        ["unit `write`: calls mutator `coreMethod` while loading but is not marked `effect`"]
    );
    // Calling a mutator inside a function body is not a load-time effect.
    let unit = "  //@unit transient requires: asTransientProtocol\n  function transient(value) { return asTransientProtocol(value); }\n";
    let deferred = ANNOTATED_RUNTIME.replace(
        unit,
        "  //@unit transient requires: asTransientProtocol, installMethods\n  function transient(value) { installMethods(value, []); return asTransientProtocol(value); }\n",
    );
    RuntimeManifest::parse(&deferred)
        .unwrap()
        .validate()
        .unwrap();
    // A type's prototype is set by an effect attached to its constructor.
    let setup = "  //@unit KeywordValue.type effect: KeywordValue requires: builtinType, keywordPrototype,\n  //@ KeywordValue\n  builtinType(KeywordValue, keywordPrototype);\n";
    assert!(ANNOTATED_RUNTIME.contains(setup));
    let hidden = ANNOTATED_RUNTIME.replace(
        setup,
        "  //@unit KeywordType requires: builtinType, KeywordValue, keywordPrototype\n  const KeywordType = builtinType(KeywordValue, keywordPrototype);\n",
    );
    assert_eq!(
        messages(&hidden),
        [
            "unit `KeywordType`: calls mutator `builtinType` while loading but is not marked `effect`"
        ]
    );
    // Every dispatcher registration is a declarator the manifest can list.
    let registration = "const write = coreMethod(\"IWriter/-write\"";
    assert!(ANNOTATED_RUNTIME.contains(registration));
    let indirect = ANNOTATED_RUNTIME.replace(
        registration,
        "const write = (0, coreMethod)(\"IWriter/-write\"",
    );
    let errors = RuntimeManifest::parse(&indirect).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0]
            .message
            .starts_with("unit `write`: `coreMethod` registers a core protocol dispatcher by key"),
        "{}",
        errors[0]
    );
}

#[test]
fn units_concatenate_back_to_the_annotated_file_and_the_embedded_runtime() {
    let manifest = manifest();
    assert_eq!(manifest.annotated_text(), ANNOTATED_RUNTIME);
    // The embedded runtime is the annotated file without its header lines,
    // which is what build.rs writes and what the shared path embeds.
    let stripped = ANNOTATED_RUNTIME
        .split_inclusive('\n')
        .filter(|line| !is_header_line(line))
        .collect::<String>();
    assert_eq!(manifest.runtime_text(), RUNTIME);
    assert_eq!(stripped, RUNTIME);
    assert_eq!(RUNTIME_SOURCE, RUNTIME);
    assert!(!RUNTIME.contains("//@"));
    // The seam that anchor-patching tests and the emitter rely on.
    assert!(RUNTIME.starts_with("// Original bounded runtime"));
    assert_eq!(RUNTIME.matches("// Original bounded runtime").count(), 1);
    assert!(RUNTIME.trim_end().ends_with("})()"));
    // Every byte belongs to exactly one part, in order.
    let mut offset = manifest.prologue.end;
    for unit in &manifest.units {
        assert_eq!(unit.header_range.start, offset, "unit `{}`", unit.name);
        assert_eq!(
            unit.header_range.end, unit.body_range.start,
            "unit `{}`",
            unit.name
        );
        offset = unit.body_range.end;
    }
    assert_eq!(manifest.exports.header_range.start, offset);
    assert_eq!(manifest.exports.body_range.end, ANNOTATED_RUNTIME.len());
    // A slice that keeps every unit and exports the export list is the
    // runtime itself, so slices differ from it only in what they leave out.
    assert_eq!(
        manifest.sliced_text(&vec![true; manifest.units.len()], &manifest.exports.names),
        RUNTIME
    );
}

#[test]
fn slices_keep_units_in_runtime_order_and_attribute_them_to_the_first_root() {
    let manifest = manifest();
    let unit = |name| manifest.unit_named(name).unwrap();
    let roots = [
        unit("jsObject"),
        unit("protocol"),
        unit("core"),
        unit("field"),
    ];
    let first = manifest.reached_by(&roots);
    assert_eq!(
        first.iter().map(Option::is_some).collect::<Vec<_>>(),
        manifest.reachable(roots)
    );
    assert_eq!(first[unit("jsObject").0], Some(0));
    for (index, reached) in manifest.reachable([unit("protocol")]).iter().enumerate() {
        if *reached {
            assert_eq!(
                first[index],
                Some(1),
                "unit `{}`",
                manifest.units[index].name
            );
        }
    }
    // A root whose closure earlier roots already cover adds nothing.
    let core = manifest.reachable([unit("core")]);
    assert!(core[unit("field").0]);
    assert!(!first.contains(&Some(3)));
    for (index, reached) in first.iter().enumerate() {
        assert_eq!(
            *reached == Some(2),
            core[index] && *reached != Some(1) && *reached != Some(0)
        );
    }

    let kept = manifest.reachable([unit("protocol")]);
    let sliced = manifest.sliced_text(&kept, &["protocol"]);
    let mut expected = RUNTIME[..manifest.prologue.len()].to_owned();
    for (index, unit) in manifest.units.iter().enumerate() {
        if kept[index] {
            expected.push_str(&ANNOTATED_RUNTIME[unit.body_range.clone()]);
        }
    }
    expected.push_str("  return Object.freeze({protocol});\n})()\n");
    assert_eq!(sliced, expected);
}

/// Compile-time units the export list does not reach; any other unreached
/// unit is dead code. None today: the compile-time entry points
/// (`compilerForms`, `compilerFormBridge`, `setDeclarationDemand`, ...) are
/// exported, and `core` reaches the reader and the namespace registry. A
/// pellet that stops exporting compile-time machinery lists its units here.
const UNREACHABLE_FROM_EXPORTS: &[&str] = &[];

#[test]
fn every_unit_is_reachable_from_the_export_list() {
    let manifest = manifest();
    let reachable = manifest.reachable(manifest.export_roots());
    let unreachable = manifest
        .units
        .iter()
        .zip(&reachable)
        .filter(|(_, reached)| !**reached)
        .map(|(unit, _)| unit.name)
        .collect::<Vec<_>>();
    assert_eq!(unreachable, UNREACHABLE_FROM_EXPORTS);
}

#[test]
fn names_bindings_and_effect_targets_resolve_to_units() {
    let manifest = manifest();
    assert!(ECMASCRIPT_GLOBALS.is_sorted());
    for (index, unit) in manifest.units.iter().enumerate() {
        let id = UnitId(index);
        assert_eq!(manifest.unit_named(unit.name), Some(id));
        for binding in &unit.declares {
            assert_eq!(manifest.unit_of(binding), Some(id), "binding `{binding}`");
        }
        if let Some(effect) = &unit.effect {
            for target in &effect.targets {
                let owner = manifest.unit_of(target).unwrap();
                let attached = if effect.install {
                    manifest.attached_installs(owner)
                } else {
                    manifest.attached_effects(owner)
                };
                assert!(attached.contains(&id), "unit `{}`", unit.name);
            }
        }
    }
    let unit = |name| manifest.unit_named(name).unwrap();
    // Effects travel with the bindings they mutate, and only with them.
    assert_eq!(
        manifest.attached_effects(unit("ListValue")),
        [
            unit("ListValue.type"),
            unit("ListValue.members"),
            unit("ListValue.statics")
        ]
    );
    let list = manifest.reachable([unit("ListValue")]);
    assert!(list[unit("ListValue.members").0] && !list[unit("Eduction.members").0]);
    // Installs travel with a binding whose value escapes: a value root or a
    // kept unit that requires it. A root the program only calls keeps none.
    assert!(manifest.attached_effects(unit("add")).is_empty());
    assert_eq!(
        manifest.attached_installs(unit("add")),
        [unit("add.variadic")]
    );
    assert!(manifest.reachable([unit("add")])[unit("add.variadic").0]);
    let called = manifest.reached_by_uses(&[(unit("add"), RootUse::Call)]);
    assert_eq!(
        called.iter().filter(|first| first.is_some()).count(),
        1,
        "a called `add` keeps only itself"
    );
    let escaped = manifest.reached_by_uses(&[
        (unit("add"), RootUse::Call),
        (unit("uncheckedAdd"), RootUse::Call),
    ]);
    assert_eq!(escaped[unit("add.variadic").0], Some(1));
    assert!(!manifest.reachable([unit("keywords")])[unit("add.variadic").0]);
    // The core hub reaches every operation; its cells reach none of the hub.
    let core = manifest.reachable([unit("core")]);
    for kept in ["reader", "coreCells", "core.seal", "namespaces.cljsUser"] {
        assert!(core[unit(kept).0], "{kept}");
    }
    assert!(!manifest.reachable([unit("coreCells")])[unit("core").0]);
    let namespaces = manifest.reachable([unit("namespaces")]);
    for effect in ["namespaces.cljsUser", "namespaces.cljsCore"] {
        assert!(namespaces[unit(effect).0], "{effect}");
    }
    // Every `coreMethod` registration is in the dispatcher table.
    let dispatchers = ANNOTATED_RUNTIME.matches(" = coreMethod(\"").count();
    assert_eq!(dispatchers, 65);
    assert_eq!(
        manifest.core_dispatcher("ICounted/-count"),
        Some("countProtocol")
    );
    assert_eq!(manifest.core_dispatcher("IWriter/-write"), Some("write"));
    assert_eq!(manifest.core_dispatcher("ICounted"), None);
}

/// Per-unit sizes for benchmarks/compiler.md, as TSV: body bytes (with the
/// unit's comments and blank lines), the number of required bindings, effect
/// targets (`own` for bare `effect`), and the units and bytes that keeping
/// this unit alone keeps. Run with
/// `cargo test --test compiler runtime_manifest::print_runtime_unit_sizes -- --ignored --nocapture`.
#[test]
#[ignore = "measurement; prints the unit table"]
fn print_runtime_unit_sizes() {
    let manifest = manifest();
    println!("unit\tbytes\trequires\teffect\treach_units\treach_bytes");
    for (index, unit) in manifest.units.iter().enumerate() {
        let effect = match &unit.effect {
            None => String::new(),
            Some(effect) if effect.targets.is_empty() => "own".into(),
            Some(effect) if effect.install => format!("install:{}", effect.targets.join(",")),
            Some(effect) => effect.targets.join(","),
        };
        let reach = manifest.reachable([UnitId(index)]);
        let (units, bytes) = manifest
            .units
            .iter()
            .zip(&reach)
            .filter(|(_, kept)| **kept)
            .fold((0, 0), |(units, bytes), (unit, _)| {
                (units + 1, bytes + unit.body_range.len())
            });
        println!(
            "{}\t{}\t{}\t{effect}\t{units}\t{bytes}",
            unit.name,
            unit.body_range.len(),
            unit.requires.len()
        );
    }
}
