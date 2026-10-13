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
/// unit is dead code. None today: the compiler, macro host and REPL reach the
/// `compile-time` units through the shared runtime's exports (`compilerForms`,
/// `compilerFormBridge`, `setDeclarationDemand`, ...), and `core` reaches the
/// reader and the namespace registry. A pellet that stops exporting
/// compile-time machinery lists its units here.
const UNREACHABLE_FROM_EXPORTS: &[&str] = &[];

/// The string-literal protocol method keys of `text` (`"IHash/-hash"`), each
/// with whether it is the first item of a method entry (`["IHash/-hash", 1,
/// ...]`), which installs an implementation rather than dispatching it.
fn method_keys(text: &str) -> Vec<(&str, bool)> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let literal = &after[..end];
        let entry = rest[..start].trim_end().ends_with('[');
        if literal.starts_with('I')
            && literal.split_once("/-").is_some_and(|(protocol, method)| {
                protocol.chars().all(char::is_alphanumeric) && !method.is_empty()
            })
        {
            keys.push((literal, entry));
        }
        rest = &after[end + 1..];
    }
    keys
}

/// The hook tables a conditional effect may fill in (`typeHooks.rest =
/// fnRest`), each read as `<table>.<key>`.
const HOOK_TABLES: [&str; 3] = ["typeHooks", "kindHooks", "protocolHooks"];

/// The `<table>.<key>` hooks a unit body fills in.
fn installed_hooks(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.trim().split_once(" = ").map(|(hook, _)| hook))
        .filter(|hook| {
            hook.split_once('.').is_some_and(|(table, key)| {
                HOOK_TABLES.contains(&table)
                    && !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_alphanumeric())
            })
        })
        .collect()
}

/// Whether `text` reads `hook`: `kindHooks.seqMap`, but not
/// `kindHooks.seqMapX` or an assignment to it.
fn reads_hook(text: &str, hook: &str) -> bool {
    text.match_indices(hook).any(|(start, _)| {
        let after = &text[start + hook.len()..];
        let assigned = after.trim_start().starts_with('=') && !after.trim_start().starts_with("==");
        let before = &text[..start];
        let member = before.ends_with('.') && !before.ends_with("...");
        !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            && !assigned
            && !member
            && !before.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    })
}

/// An `effect: ... when:` unit installs what only some code can use, and its
/// `when:` list must name exactly that code. A unit installing protocol
/// methods (`recordMethods.hash` adds `IHash/-hash` for records) is needed
/// where one of them can be dispatched: its list names every unit whose body
/// holds one of its keys other than as a method entry. A unit filling in
/// hooks (`typeHooks.rest = fnRest`, `kindHooks.seqVector = ...`) names every
/// unit that reads one of them; a reader that is itself an effect stands for
/// its own `when:` list, or else its targets. A new dispatch site or hook
/// read fails here until the list names it.
#[test]
fn conditional_effects_name_every_unit_that_can_use_them() {
    let manifest = manifest();
    let body = |unit: UnitId| &ANNOTATED_RUNTIME[manifest.units[unit.0].body_range.clone()];
    let mut checked = 0;
    for (index, unit) in manifest.units.iter().enumerate() {
        let id = UnitId(index);
        if unit
            .effect
            .as_ref()
            .is_none_or(|effect| effect.when.is_empty())
        {
            continue;
        }
        let installed: Vec<&str> = method_keys(body(id))
            .into_iter()
            .filter_map(|(key, entry)| entry.then_some(key))
            .collect();
        let users: Vec<UnitId> = if !installed.is_empty() {
            (0..manifest.units.len())
                .map(UnitId)
                .filter(|&other| {
                    other != id
                        && method_keys(body(other))
                            .iter()
                            .any(|(key, entry)| !entry && installed.contains(key))
                })
                .collect()
        } else {
            let hooks = installed_hooks(body(id));
            assert!(
                !hooks.is_empty(),
                "`{}` installs neither protocol methods nor a hook",
                unit.name
            );
            (0..manifest.units.len())
                .map(UnitId)
                .filter(|&other| {
                    other != id && hooks.iter().any(|hook| reads_hook(body(other), hook))
                })
                .flat_map(|reader| match &manifest.units[reader.0].effect {
                    Some(effect) if effect.when.is_empty() => effect
                        .targets
                        .iter()
                        .filter_map(|target| manifest.unit_of(target))
                        .collect(),
                    Some(_) => manifest.conditions(reader).to_vec(),
                    None => vec![reader],
                })
                .collect()
        };
        let mut expected: Vec<&str> = users
            .iter()
            .map(|user| manifest.units[user.0].name)
            .collect();
        let mut listed: Vec<&str> = manifest
            .conditions(id)
            .iter()
            .map(|condition| manifest.units[condition.0].name)
            .collect();
        expected.sort_unstable();
        expected.dedup();
        listed.sort_unstable();
        assert_eq!(listed, expected, "`when:` of unit `{}`", unit.name);
        checked += 1;
    }
    // The record methods, the four type hooks, the kind hooks and the
    // protocol hooks.
    assert_eq!(checked, 60);
}

/// What values a kind hook handles: those of payload stores, built by every
/// unit that writes to one, or a narrower kind only the listed units build.
enum Kind {
    Stores(&'static [&'static str]),
    Units(&'static [&'static str]),
}

/// Every `kindHooks` entry and its kind.
const KIND_HOOKS: &[(&str, Kind)] = &[
    ("seqLazy", Kind::Stores(&["lazyValues"])),
    ("seqQueue", Kind::Stores(&["queues"])),
    ("seqVector", Kind::Stores(&["vectors"])),
    ("seqMap", Kind::Stores(&["maps"])),
    // Maps with a `tree` slot and sorted maps.
    ("seqTree", Kind::Units(&["treeMapConstructor"])),
    ("sortedMapSeq", Kind::Units(&["treeMapConstructor"])),
    ("seqSet", Kind::Stores(&["sets"])),
    // Views with a `vec` slot, which the public ChunkedSeq constructor builds.
    ("restChunkedSeq", Kind::Units(&["ChunkedSeqValue"])),
    // Views of queues.
    ("restQueue", Kind::Stores(&["queues"])),
    ("equivMap", Kind::Stores(&["maps"])),
    ("equivSet", Kind::Stores(&["sets"])),
    (
        "equivSequential",
        Kind::Stores(&["vectors", "cells", "views", "queues", "lazyValues"]),
    ),
    ("metaDeferred", Kind::Stores(&["deferredVarMetadata"])),
    ("conjQueue", Kind::Stores(&["queues"])),
    ("conjVector", Kind::Stores(&["vectors"])),
    // Vectors with slots and a source, which the public Subvec constructor
    // builds.
    ("conjSubvec", Kind::Units(&["SubvecValue"])),
    ("assocSubvec", Kind::Units(&["SubvecValue"])),
    ("conjMap", Kind::Stores(&["maps"])),
    ("conjSet", Kind::Stores(&["sets"])),
    // Maps with an `arr` slot: array maps from the public constructor.
    (
        "assocArrayMapSlots",
        Kind::Units(&["PersistentArrayMapValue"]),
    ),
    ("assocVector", Kind::Stores(&["vectors"])),
    ("withMetaFn", Kind::Stores(&["metaFunctions"])),
    ("entryVector", Kind::Stores(&["mapEntries"])),
    ("withMetaLazy", Kind::Stores(&["lazyValues"])),
    // Cells of one kind, each built only by the listed unit.
    ("withMetaRange", Kind::Units(&["rangeNode"])),
    ("withMetaChunkedCons", Kind::Units(&["ChunkedCons"])),
    ("withMetaRepeat", Kind::Units(&["RepeatValue"])),
    ("withMetaIterate", Kind::Units(&["iterateNode"])),
    ("withMetaCycle", Kind::Units(&["cycleNode"])),
    ("withMetaChunkedSeq", Kind::Units(&["ChunkedSeqValue"])),
    ("withMetaRecord", Kind::Stores(&["recordTypes"])),
    ("updatedRecord", Kind::Stores(&["recordTypes"])),
    ("lookupTransient", Kind::Stores(&["transients"])),
    ("lookupTree", Kind::Units(&["treeMapConstructor"])),
    ("entryIndex", Kind::Stores(&["maps", "sets"])),
    // Maps and sets of the hash-map kind, and records.
    (
        "hashTraversal",
        Kind::Units(&["PersistentHashMapValue", "PersistentHashSetValue"]),
    ),
    (
        "hashEntryIndex",
        Kind::Units(&[
            "PersistentHashMapValue",
            "PersistentHashSetValue",
            "makeType",
        ]),
    ),
    (
        "putHashEntry",
        Kind::Units(&[
            "PersistentHashMapValue",
            "PersistentHashSetValue",
            "makeTransient",
        ]),
    ),
    ("nthChunk", Kind::Stores(&["chunks"])),
    ("nthTransient", Kind::Stores(&["transients"])),
    ("countTransient", Kind::Stores(&["transients"])),
    (
        "hashSequential",
        Kind::Stores(&["vectors", "cells", "views", "queues", "lazyValues"]),
    ),
];

/// A kind hook is the branch of a generic operation for one kind of value,
/// installed by an effect whose targets build that kind (`effect: makeVector
/// when: seq`). A unit that builds the kind but is not a target could build a
/// value whose branch is missing, so every unit that writes to a kind's store
/// must be a target. (Effect units that write to a store only update values
/// of the type they belong to.) A new hook, or a new unit building a kind,
/// fails here until the targets name it.
#[test]
fn kind_hooks_are_installed_wherever_their_kind_can_be_built() {
    let manifest = manifest();
    let body = |unit: usize| &ANNOTATED_RUNTIME[manifest.units[unit].body_range.clone()];
    let writers = |store: &str| -> Vec<&str> {
        let calls = [format!("{store}.set("), format!("{store}.add(")];
        (0..manifest.units.len())
            .filter(|&unit| {
                !manifest.units[unit].declares.is_empty()
                    && calls.iter().any(|call| {
                        body(unit).match_indices(call.as_str()).any(|(start, _)| {
                            !body(unit)[..start].ends_with(|c: char| {
                                c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.'
                            })
                        })
                    })
            })
            .map(|unit| manifest.units[unit].name)
            .collect()
    };
    let mut seen = Vec::new();
    for (index, unit) in manifest.units.iter().enumerate() {
        let hooks: Vec<&str> = installed_hooks(body(index))
            .into_iter()
            .filter_map(|hook| hook.strip_prefix("kindHooks."))
            .collect();
        if hooks.is_empty() {
            continue;
        }
        let effect = unit
            .effect
            .as_ref()
            .expect("a kind hook install is an effect");
        assert!(!effect.when.is_empty(), "`{}` needs `when:`", unit.name);
        for hook in hooks {
            let (_, kind) = KIND_HOOKS
                .iter()
                .find(|(key, _)| *key == hook)
                .unwrap_or_else(|| panic!("kind hook `{hook}` is not in KIND_HOOKS"));
            let builders = match kind {
                Kind::Stores(stores) => stores.iter().flat_map(|store| writers(store)).collect(),
                Kind::Units(units) => units.to_vec(),
            };
            for builder in builders {
                assert!(
                    effect.targets.contains(&builder),
                    "`{}` installs `kindHooks.{hook}` but `{builder}`, which builds its kind, is not a target",
                    unit.name
                );
            }
            seen.push(hook);
        }
    }
    seen.sort_unstable();
    let mut listed: Vec<&str> = KIND_HOOKS.iter().map(|(key, _)| *key).collect();
    listed.sort_unstable();
    assert_eq!(seen, listed);
}

#[test]
fn conditional_effects_wait_for_a_condition() {
    let manifest = manifest();
    let unit = |name: &str| manifest.unit_named(name).unwrap();
    // A record type alone keeps no record method; `count` keeps its own.
    let kept = manifest.reachable([unit("recordMethods")]);
    assert!(!kept[unit("recordMethods.count").0]);
    for roots in [
        [unit("recordMethods"), unit("count")],
        [unit("count"), unit("recordMethods")],
    ] {
        let kept = manifest.reachable(roots);
        assert!(kept[unit("recordMethods.count").0]);
        assert!(!kept[unit("recordMethods.print").0]);
    }
    // ... and `count` alone keeps none.
    assert!(!manifest.reachable([unit("count")])[unit("recordMethodTable").0]);
}

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
    // Effects travel with the bindings they mutate, and only with them; kind
    // hooks also with the bindings that build their kind, here the sequential
    // branches of structural equality and hashing.
    assert_eq!(
        manifest.attached_effects(unit("ListValue")),
        [
            unit("ListValue.type"),
            unit("makeVector.equiv"),
            unit("makeVector.hash"),
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
