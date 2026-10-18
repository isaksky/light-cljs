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
    let header = "  //@unit write effect requires: coreMethod, writerValue\n";
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
    // The compile-time evaluators run it with the compiler's own hooks,
    // whole lines that output never embeds (build.rs `compiler_runtime`).
    let hook = |line: &str| {
        ["gensymLog", "encode.peak", "encode.active"]
            .iter()
            .any(|hook| line.contains(hook))
    };
    for name in ["gensymLog", "encode.peak", "encode.active"] {
        assert!(
            RUNTIME_SOURCE.contains(name) && !RUNTIME.contains(name),
            "{name}"
        );
    }
    assert_eq!(
        RUNTIME_SOURCE
            .split_inclusive('\n')
            .filter(|line| !hook(line))
            .collect::<String>(),
        RUNTIME
    );
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

/// Units the export list does not reach; any other unreached unit is dead
/// code. The compiler, macro host and REPL reach the `compile-time` units
/// through the shared runtime's exports (`compilerForms`, `compilerFormBridge`,
/// `setDeclarationDemand`, ...), and `core` reaches the reader and the
/// namespace registry; a pellet that stops exporting compile-time machinery
/// lists its units here. `upstreamMembers` is the condition of the types'
/// `.fields` effects, which only an `Advanced` program that names one of
/// `UPSTREAM_MEMBERS` roots (light-cljs-238). (`defineType`, which decorates the
/// constructors only `Advanced` emission writes, is reached through
/// `Loadable.type` since light-cljs-272.)
const UNREACHABLE_FROM_EXPORTS: &[&str] = &["upstreamMembers"];

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
/// fnRest`), each read as `<table>.<key>`, and the core cells, whose initial
/// value such an effect may set (`coreCells.rand = hostRand`).
const HOOK_TABLES: [&str; 6] = [
    "typeHooks",
    "kindHooks",
    "protocolHooks",
    "printHooks",
    "hostHooks",
    "coreCells",
];

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

/// The runtime binding a unit body wraps in a top-level statement, if any:
/// replacing it with a wrapper of itself, as `seq = behindProtocol(seq, ...)`
/// and `nth = (operation => ...)(nth)` do, or replacing a method of it, as
/// `maps.has = ...` does (light-cljs-258). Assigning a member of a hook
/// table installs a hook instead.
fn wrapped_binding(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let statement = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' '))?;
        let (target, _) = statement.split_once(" = ")?;
        let (name, method) = target.split_once('.').unwrap_or((target, ""));
        let identifier = |text: &str| text.chars().all(|c| c.is_ascii_alphanumeric());
        (!name.is_empty()
            && identifier(name)
            && identifier(method)
            && if method.is_empty() {
                text.contains(&format!("({name}, ")) || text.contains(&format!("({name});"))
            } else {
                !HOOK_TABLES.contains(&name)
            })
        .then_some(name)
    })
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

/// The kind table entries a unit body fills in (`kindTables[2][1] = ...`,
/// light-cljs-237), as the kind's number and the operation's index.
fn installed_kind_entries(text: &str) -> Vec<(u32, u32)> {
    text.lines()
        .filter_map(|line| {
            let (target, _) = line.trim().split_once(" = ")?;
            let (kind, index) = target
                .strip_prefix("kindTables[")?
                .strip_suffix(']')?
                .split_once("][")?;
            Some((kind.parse().ok()?, index.parse().ok()?))
        })
        .collect()
}

/// The operation a unit wraps behind its kinds' branches, with the index of
/// its entries: `seq = (operation => ...)(seq)`, whose function calls
/// `kindTables[...][0]`.
fn branches_wrapper(text: &str) -> Option<(&str, u32)> {
    let name = text.lines().find_map(|line| {
        let (name, _) = line.strip_prefix("  ")?.split_once(" = (operation => ")?;
        Some(name)
    })?;
    let (_, after) = text.split_once("kindTables[kind & 3][")?;
    let index = after[..after.find(']')?].parse().ok()?;
    text.trim_end()
        .ends_with(&format!("({name});"))
        .then_some((name, index))
}

/// An `effect: ... when:` unit installs what only some code can use, and its
/// `when:` list must name exactly that code. A unit wrapping an operation
/// names the operation. A unit installing protocol
/// methods (`recordMethods.hash` adds `IHash/-hash` for records) is needed
/// where one of them can be dispatched: its list names every unit whose body
/// holds one of its keys other than as a method entry, or for a unit wrapping
/// an operation, the operation. A unit filling in
/// hooks (`typeHooks.rest = fnRest`, `kindHooks.seqVector = ...`) names every
/// unit that reads one of them; a reader that is itself an effect stands for
/// its own `when:` list, or else its targets, unless the list names the
/// effect itself (`when: makeMap.equiv`), which only that effect keeps. A new
/// dispatch site or hook read fails here until the list names it.
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
            || OWNED_TYPE_DECORATIONS.contains(&unit.name)
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
                // A dispatch site in a unit wrapping an operation is needed
                // wherever the operation is.
                .flat_map(|user| match wrapped_binding(body(user)) {
                    Some(_) => manifest.conditions(user).to_vec(),
                    None => vec![user],
                })
                .collect()
        } else if let Some(wrapped) = wrapped_binding(body(id)) {
            // A unit wrapping an operation (`seq = behindProtocol(seq, ...)`)
            // is needed wherever the operation is.
            vec![
                manifest
                    .unit_of(wrapped)
                    .expect("a wrapped runtime binding"),
            ]
        } else if let Some((_, after)) = body(id).split_once(".prototype[kindOps] = ") {
            // A type's kind number that waits (views, light-cljs-237) is
            // needed where a wrapper reading one of its kind's entries is.
            let kind: u32 = after[..after.find(';').unwrap()].parse().unwrap();
            let entries: Vec<u32> = (0..manifest.units.len())
                .flat_map(|other| installed_kind_entries(body(UnitId(other))))
                .filter(|(of, _)| *of == kind)
                .map(|(_, index)| index)
                .collect();
            (0..manifest.units.len())
                .map(UnitId)
                .filter(|&other| {
                    branches_wrapper(body(other)).is_some_and(|(_, index)| entries.contains(&index))
                })
                .collect()
        } else if unit.name.ends_with(".fields") {
            // A type's upstream fields and methods, which only code naming
            // them reads (`upstreamMembers`) or `reset-meta!` assigns.
            ["resetMeta", "upstreamMembers"]
                .map(|binding| manifest.unit_of(binding).unwrap())
                .to_vec()
        } else {
            let hooks = installed_hooks(body(id));
            // A kind table entry is read where its operation's wrapper is.
            let entries: Vec<u32> = installed_kind_entries(body(id))
                .into_iter()
                .map(|(_, index)| index)
                .collect();
            assert!(
                !hooks.is_empty() || !entries.is_empty(),
                "`{}` installs neither protocol methods, a hook, a kind table entry nor a wrapper",
                unit.name
            );
            (0..manifest.units.len())
                .map(UnitId)
                .filter(|&other| {
                    other != id
                        && (hooks.iter().any(|hook| reads_hook(body(other), hook))
                            || branches_wrapper(body(other))
                                .is_some_and(|(_, index)| entries.contains(&index)))
                })
                .flat_map(|reader| match &manifest.units[reader.0].effect {
                    Some(effect) if effect.when.is_empty() => effect
                        .targets
                        .iter()
                        .filter_map(|target| manifest.unit_of(target))
                        .collect(),
                    // A list may name the effect itself, kept only with a
                    // target, instead of its conditions.
                    Some(_) if manifest.conditions(id).contains(&reader) => vec![reader],
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
    // The record methods, the five type hooks, the kind hooks (ten of them
    // the printer's), the protocol hooks (two installs each of the protocol
    // tests and one per core dispatcher since light-cljs-251), the printer's
    // hooks, the initial values of four core cells, the protocol wrappers of
    // seven operations, the brand readers of `typeFields` and `recordTypes`
    // and the record wrappers of `maps` and `mapKinds` (light-cljs-258), the
    // hash-map trie wrappers of two operations (light-cljs-235), the hash-set
    // trie wrapper of `lookup` (light-cljs-236), the upstream fields of 28
    // types, the deref wrapper of light.lazy's `Loadable` (light-cljs-272),
    // and the units filling in the kind tables' entries for lazy seqs and
    // cells that no kind hook unit fills in (9), wrapping five operations
    // behind them, and giving two view types and their `first` a kind
    // (light-cljs-237).
    assert_eq!(checked, 194);
}

/// The decorations of a type the runtime owns for a library, `light.lazy`'s
/// `Loadable` (light-cljs-272), which `an_owned_type_is_decorated_wherever_it_can_be_observed`
/// checks instead of the test above.
const OWNED_TYPE_DECORATIONS: &[&str] = &["Loadable.name", "Loadable.type", "Loadable.dispatch"];

/// `Loadable` gets what a `deftype` implementing IDeref has where code can
/// observe it. Its names (`Loadable.name`) are needed by every unit reading
/// a constructor's `cljs$lang$ctorStr`, and by the readers of `typeNames`,
/// which require it. Its protocols as `defineType` installs them
/// (`Loadable.type`) are needed where a program defines types of its own
/// (`decorateType`), and wherever a unit looks a protocol up by mask or by
/// slot: every reader of a protocol mask requires `protocolMaskNames` for the
/// mask's index, and every slot lookup goes through `protocolSlot`. Its IDeref
/// dispatches to another implementation (`Loadable.dispatch`) wherever one can
/// be given, which `extend-type` and `specify!` do through `installMethods`.
#[test]
fn an_owned_type_is_decorated_wherever_it_can_be_observed() {
    let manifest = manifest();
    let body = |unit: UnitId| &ANNOTATED_RUNTIME[manifest.units[unit.0].body_range.clone()];
    let unit = |name: &str| {
        UnitId(
            manifest
                .units
                .iter()
                .position(|unit| unit.name == name)
                .unwrap_or_else(|| panic!("unit `{name}`")),
        )
    };
    let names = |units: &[UnitId]| {
        let mut names: Vec<&str> = units.iter().map(|u| manifest.units[u.0].name).collect();
        names.sort_unstable();
        names
    };
    let reads = |text: &str, member: &str| {
        text.match_indices(member).any(|(start, _)| {
            let after = text[start + member.len()..].trim_start();
            !(after.starts_with('=') && !after.starts_with("=="))
        })
    };
    let others = || {
        (0..manifest.units.len())
            .map(UnitId)
            .filter(|&other| !OWNED_TYPE_DECORATIONS.contains(&manifest.units[other.0].name))
    };
    let mut expected: Vec<UnitId> = others()
        .filter(|&other| reads(body(other), "cljs$lang$ctorStr"))
        .collect();
    expected.push(manifest.unit_of("typeNames").unwrap());
    assert_eq!(
        names(manifest.conditions(unit("Loadable.name"))),
        names(&expected),
        "`when:` of unit `Loadable.name`"
    );
    let (masks, slots) = (unit("protocolMaskNames"), unit("protocolSlot"));
    for other in others() {
        let text = body(other);
        if reads(text, "\"cljs$lang$protocol_mask$partition\"") && !text.contains("] = mask") {
            assert!(
                manifest.dependencies(other).contains(&masks),
                "`{}` reads a protocol mask without `protocolMaskNames`",
                manifest.units[other.0].name
            );
        }
        if text.contains("protocolSlot(") && other != slots {
            assert!(
                manifest.dependencies(other).contains(&slots),
                "`{}` looks up a protocol slot without `protocolSlot`",
                manifest.units[other.0].name
            );
        }
    }
    assert_eq!(
        names(manifest.conditions(unit("Loadable.type"))),
        names(&[unit("decorateType"), masks, slots]),
        "`when:` of unit `Loadable.type`"
    );
    let install = unit("installMethods");
    for extension in ["extendType", "extendInstance"] {
        assert!(
            manifest.dependencies(unit(extension)).contains(&install),
            "`{extension}` installs methods without `installMethods`"
        );
    }
    assert_eq!(
        names(manifest.conditions(unit("Loadable.dispatch"))),
        names(&[install]),
        "`when:` of unit `Loadable.dispatch`"
    );
    // A program can also read the marker and slots, the masks or the names
    // by naming them, where the slice roots the unit `LOADABLE_MEMBERS`
    // lists (light-cljs-296): a condition of the decoration adding them,
    // which keeps it and nothing the decoration does not keep itself.
    let loadable = unit("Loadable");
    let decorations = [
        ("cljs$core$IDeref$", "Loadable.type"),
        ("cljs$lang$protocol_mask$partition", "Loadable.type"),
        ("cljs$lang$ctorStr", "Loadable.name"),
        ("cljs$lang$type", "Loadable.name"),
    ];
    let mut listed = Vec::new();
    for (members, condition) in light_cljs::runtime_slice::LOADABLE_MEMBERS {
        let condition = manifest.unit_of(condition).unwrap();
        for member in *members {
            listed.push(*member);
            let (_, decoration) = decorations
                .iter()
                .find(|(name, _)| name == member)
                .unwrap_or_else(|| panic!("no decoration adds `{member}`"));
            let decoration = unit(decoration);
            assert!(
                manifest.conditions(decoration).contains(&condition),
                "`{member}` roots `{}`, which `{}` does not wait for",
                manifest.units[condition.0].name,
                manifest.units[decoration.0].name
            );
            assert!(!manifest.reachable([loadable])[decoration.0]);
            assert_eq!(
                manifest.reachable([loadable, condition]),
                manifest.reachable([loadable, decoration]),
                "`{member}`"
            );
        }
    }
    listed.sort_unstable();
    let mut added: Vec<&str> = decorations.iter().map(|(name, _)| *name).collect();
    added.sort_unstable();
    assert_eq!(listed, added);
}

/// What values a kind hook handles: those of payload stores, built by every
/// unit that writes to one, or a narrower kind only the listed units build.
/// `Maps` is the payload stores without records, which `maps.records` adds
/// to the `maps` store but which answer the operations reading these hooks
/// through their own methods (`kind_hooks_skip_records`).
enum Kind {
    Stores(&'static [&'static str]),
    Maps(&'static [&'static str]),
    Units(&'static [&'static str]),
}

/// Every `kindHooks` entry and its kind. A hook that several units install,
/// each for its own kind, has an entry per unit, `<hook>/<unit>`.
const KIND_HOOKS: &[(&str, Kind)] = &[
    ("seqLazy", Kind::Stores(&["lazyValues"])),
    ("realizeLazy", Kind::Stores(&["lazyValues"])),
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
    ("equivMap", Kind::Maps(&["maps"])),
    ("equivSet", Kind::Stores(&["sets"])),
    // Collections `str` prints: sequential values, maps and sets.
    (
        "strCollection",
        Kind::Stores(&[
            "vectors",
            "cells",
            "views",
            "queues",
            "lazyValues",
            "maps",
            "sets",
        ]),
    ),
    // Two vectors compare their items where no protocol implementation
    // exists; other sequential values walk their seqs.
    ("equivVectors", Kind::Stores(&["vectors"])),
    (
        "equivSequential",
        Kind::Stores(&["cells", "views", "queues", "lazyValues"]),
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
    ("nthVector", Kind::Stores(&["vectors"])),
    ("lookupTree", Kind::Units(&["treeMapConstructor"])),
    ("entryIndex", Kind::Maps(&["maps", "sets"])),
    // Maps and sets of the hash-map kind, and records. A set has another
    // kind than array-map only where `putMember` promoted it or the public
    // constructor (`publicHashSet`) took its map's kind (light-cljs-231).
    (
        "hashTraversal",
        Kind::Units(&["PersistentHashMapValue", "publicHashSet", "putMember"]),
    ),
    (
        "hashEntryIndex",
        Kind::Units(&[
            "PersistentHashMapValue",
            "publicHashSet",
            "putMember",
            "recordMethods",
        ]),
    ),
    (
        "putHashEntry",
        Kind::Units(&[
            "PersistentHashMapValue",
            "publicHashSet",
            "putMember",
            "makeTransient",
        ]),
    ),
    // Hash maps, whose payload is a trie (light-cljs-235), transient maps
    // that `assoc!` promotes to one, and hash sets, whose members are a trie
    // where `putMember` promoted them or the public constructor took a hash
    // map's kind (light-cljs-236), and their transients.
    (
        "trieEntry",
        Kind::Units(&[
            "PersistentHashMapValue",
            "assocTransient",
            "publicHashSet",
            "putMember",
        ]),
    ),
    ("dissocTrie", Kind::Units(&["PersistentHashMapValue"])),
    ("disjTrie", Kind::Units(&["publicHashSet", "putMember"])),
    // The map of a set of another kind than array-map.
    ("hashSetMap", Kind::Units(&["publicHashSet", "putMember"])),
    // Sets with public slots, which only the public constructors build.
    (
        "countPublicSet",
        Kind::Units(&["publicHashSet", "treeSetConstructor"]),
    ),
    ("nthChunk", Kind::Stores(&["chunks"])),
    ("nthTransient", Kind::Stores(&["transients"])),
    // A transient of a kind comes from a persistent value of that kind.
    ("persistVector", Kind::Stores(&["vectors"])),
    ("persistMap", Kind::Stores(&["maps"])),
    ("persistSet", Kind::Stores(&["sets"])),
    ("transientVector", Kind::Stores(&["vectors"])),
    ("conjTransientVector", Kind::Stores(&["vectors"])),
    ("assocTransientVector", Kind::Stores(&["vectors"])),
    ("conjTransientMap", Kind::Stores(&["maps"])),
    ("conjTransientSet", Kind::Stores(&["sets"])),
    ("transientEntryIndex", Kind::Stores(&["maps", "sets"])),
    ("countTransient", Kind::Stores(&["transients"])),
    // Values `count` walks: cells (those without a count), lazy seqs and host
    // iterators' seqs.
    (
        "countSeq",
        Kind::Stores(&["cells", "lazyValues", "iteratorSeqs"]),
    ),
    ("countTreeSeq", Kind::Units(&["treeMapSeqConstructor"])),
    (
        "hashSequential",
        Kind::Stores(&["vectors", "cells", "views", "queues", "lazyValues"]),
    ),
    // The printer's branch for each kind, chained through one hook, which
    // each unit adding a kind's printer names (`print/<unit>`).
    ("print/ExceptionInfo.print", Kind::Units(&["ExceptionInfo"])),
    ("print/makeTransient.print", Kind::Stores(&["transients"])),
    ("print/Atom.print", Kind::Stores(&["atoms", "volatiles"])),
    ("print/Delay.print", Kind::Stores(&["delays"])),
    (
        "print/makeKeyword.print",
        Kind::Stores(&["keywords", "symbols", "vars"]),
    ),
    ("print/uuidValue.print", Kind::Stores(&["uuids"])),
    ("print/makeVector.print", Kind::Stores(&["vectors"])),
    (
        "print/cons.print",
        Kind::Stores(&["cells", "views", "queues", "lazyValues"]),
    ),
    ("print/makeMap.print", Kind::Stores(&["maps"])),
    ("print/makeSet.print", Kind::Stores(&["sets"])),
    // Values whose metadata the printer prints (`metaValue`).
    (
        "printMetadata",
        Kind::Stores(&[
            "symbols",
            "vectors",
            "maps",
            "sets",
            "cells",
            "views",
            "queues",
            "lazyValues",
            "vars",
            "atoms",
            "metaFunctions",
            "reifyValues",
        ]),
    ),
    (
        "iteratorArrayMap",
        Kind::Units(&["PersistentArrayMapValue"]),
    ),
    ("iteratorVector", Kind::Stores(&["vectors"])),
    // A chunked seq that its public constructor did not build is a vector's.
    ("vectorOfItems", Kind::Stores(&["vectors"])),
    ("dropChunkedSeq", Kind::Units(&["ChunkedSeqValue"])),
    ("dropVector", Kind::Stores(&["vectors"])),
    ("chunkedVectorRest", Kind::Units(&["ChunkedSeqValue"])),
    ("reduceIterate", Kind::Units(&["iterateNode"])),
    ("reduceCycle", Kind::Units(&["cycleNode"])),
    // Keys of these kinds lift a namespace out of a printed map.
    ("namespaceOf", Kind::Stores(&["keywords", "symbols"])),
    ("unqualified", Kind::Stores(&["keywords", "symbols"])),
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
        let mut writers: Vec<&str> = (0..manifest.units.len())
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
            .collect();
        // Since light-cljs-258 `makeType` writes no store: it brands an
        // instance, and the effects wrapping `recordTypes`, `maps` and
        // `mapKinds` (`recordTypes.brands`, `maps.records`) answer those
        // stores for records from the brand, so their targets build those
        // kinds.
        for unit in 0..manifest.units.len() {
            if wrapped_binding(body(unit)) == Some(store)
                && let Some(effect) = &manifest.units[unit].effect
            {
                writers.extend(effect.targets.iter().copied());
            }
        }
        writers
    };
    // Only a record's brand holds a record type, so those effects target
    // `recordMethods`, which emission keeps for every record type, and a
    // hook for records waits for that unit (light-cljs-251).
    let make_type = body(manifest.unit_named("makeType").unwrap().0);
    assert!(make_type.contains("new TypeBrand(this, record === true ? Type : null)"));
    // So the hooks a record reaches (`makeMap.seq`, `makeMap.print`,
    // `str.collections`, ...) wait for `recordMethods`, which only a program
    // with records keeps; none waits for `makeType`, which every `deftype`
    // keeps (light-cljs-254).
    for unit in &manifest.units {
        if let Some(effect) = &unit.effect {
            assert!(
                !effect.targets.contains(&"makeType"),
                "`{}` targets `makeType`; records wait for `recordMethods`",
                unit.name
            );
        }
    }
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
            let (key, kind) = KIND_HOOKS
                .iter()
                .find(|(key, _)| *key == hook || key.split_once('/') == Some((hook, unit.name)))
                .unwrap_or_else(|| panic!("kind hook `{hook}` is not in KIND_HOOKS"));
            let builders = match kind {
                Kind::Stores(stores) => stores.iter().flat_map(|store| writers(store)).collect(),
                Kind::Maps(stores) => stores
                    .iter()
                    .flat_map(|store| writers(store))
                    .filter(|&builder| builder != "recordMethods")
                    .collect(),
                Kind::Units(units) => units.to_vec(),
            };
            for builder in builders {
                assert!(
                    effect.targets.contains(&builder),
                    "`{}` installs `kindHooks.{hook}` but `{builder}`, which builds its kind, is not a target",
                    unit.name
                );
            }
            seen.push(*key);
        }
    }
    seen.sort_unstable();
    let mut listed: Vec<&str> = KIND_HOOKS.iter().map(|(key, _)| *key).collect();
    listed.sort_unstable();
    assert_eq!(seen, listed);
}

/// A kind table entry is the branch of a generic operation for the values of
/// the types whose prototypes hold the kind's number (`builtinType(Cons,
/// sequencePrototype).prototype[kindOps] = 2`, light-cljs-237). The unit
/// installing it is an effect of exactly those types, so a program keeps the
/// branches of the kinds it builds, and no type of the kind can build a value
/// whose branch is missing. Each kind's entry for an operation is installed
/// once, and the operations' wrappers, which read the entries, are effects of
/// every type of the kinds 1 and 2 (lazy seqs and cells). The types of kind 3
/// (views) take their number only where a wrapper of an operation with a
/// kind 3 entry is kept, so a program that builds views and no lazy seq or
/// cell keeps no wrapper.
#[test]
fn kind_table_entries_are_installed_by_the_types_of_their_kind() {
    let manifest = manifest();
    let body = |unit: usize| &ANNOTATED_RUNTIME[manifest.units[unit].body_range.clone()];
    let mut types: std::collections::BTreeMap<u32, Vec<&str>> = Default::default();
    let mut riding: Vec<(&str, Vec<&str>)> = Vec::new();
    for (index, unit) in manifest.units.iter().enumerate() {
        let text = body(index);
        // `builtinType(Cons, sequencePrototype).prototype[kindOps] = 2` or
        // `IndexedSeqValue.prototype[kindOps] = 3`.
        for (start, _) in text.match_indices(".prototype[kindOps] = ") {
            let after = &text[start + ".prototype[kindOps] = ".len()..];
            let kind = after[..after.find(';').unwrap()].parse::<u32>().unwrap();
            let before = &text[..start];
            let binding = match before.strip_suffix(')') {
                Some(call) => {
                    let arguments =
                        &call[call.rfind("builtinType(").unwrap() + "builtinType(".len()..];
                    arguments.split(", ").next().unwrap()
                }
                None => before
                    .rsplit(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap(),
            };
            assert!(
                manifest.unit_of(binding).is_some(),
                "`{binding}` in `{}`",
                unit.name
            );
            let effect = unit.effect.as_ref().expect("a kind number is an effect");
            assert_eq!(effect.targets, [binding], "targets of `{}`", unit.name);
            assert_eq!(
                effect.when.is_empty(),
                kind != 3,
                "`when:` of `{}`",
                unit.name
            );
            if kind == 3 {
                let mut when = effect.when.clone();
                when.sort_unstable();
                riding.push((unit.name, when));
            }
            types.entry(kind).or_default().push(binding);
        }
    }
    assert_eq!(types.keys().copied().collect::<Vec<_>>(), [1, 2, 3]);
    let mut building: Vec<&str> = [1, 2].iter().flat_map(|kind| types[kind].clone()).collect();
    building.sort_unstable();
    let mut wrapped = Vec::new();
    let mut seen = Vec::new();
    for (index, unit) in manifest.units.iter().enumerate() {
        if let Some((operation, entry)) = branches_wrapper(body(index)) {
            let mut targets = unit
                .effect
                .as_ref()
                .expect("a wrapper is an effect")
                .targets
                .clone();
            targets.sort_unstable();
            assert_eq!(targets, building, "targets of `{}`", unit.name);
            wrapped.push((entry, operation, unit.name));
        }
        for (kind, operation) in installed_kind_entries(body(index)) {
            let effect = unit
                .effect
                .as_ref()
                .expect("a kind table entry install is an effect");
            assert!(!effect.when.is_empty(), "`{}` needs `when:`", unit.name);
            let mut targets = effect.targets.clone();
            let mut expected = types[&kind].clone();
            targets.sort_unstable();
            expected.sort_unstable();
            assert_eq!(targets, expected, "targets of `{}`", unit.name);
            assert!(
                !seen.contains(&(kind, operation)),
                "`kindTables[{kind}][{operation}]` twice"
            );
            seen.push((kind, operation));
        }
    }
    wrapped.sort_unstable();
    assert_eq!(
        wrapped
            .iter()
            .map(|(entry, operation, _)| (*entry, *operation))
            .collect::<Vec<_>>(),
        [
            (0, "seq"),
            (1, "first"),
            (2, "rest"),
            (3, "next"),
            (4, "count")
        ]
    );
    seen.sort_unstable();
    assert_eq!(
        seen,
        [
            (1, 0),
            (1, 1),
            (1, 2),
            (1, 3),
            (1, 4),
            (2, 0),
            (2, 1),
            (2, 2),
            (2, 3),
            (2, 4),
            (3, 1),
        ]
    );
    // A view's number waits for the wrappers that read a kind 3 entry.
    let mut readers: Vec<&str> = wrapped
        .iter()
        .filter(|(entry, _, _)| seen.contains(&(3, *entry)))
        .map(|(_, _, name)| *name)
        .collect();
    readers.sort_unstable();
    for (name, when) in riding {
        assert_eq!(when, readers, "`when:` of `{name}`");
    }
}

/// A record is in the `maps` store, but the hooks of `Kind::Maps` need not
/// wait for `makeType` (light-cljs-246): wherever an operation could pass a
/// record to them, the record answers through its own method first.
/// Equality reaches `kindHooks.equivMap` only from `equalPair` and
/// `objectEquiv`, which dispatch `IEquiv` through `protocolHooks.builtin`;
/// `recordMethods.equiv` and that hook wait for `equalPair`, which
/// `structuralEqualPair` requires. `kindHooks.entryIndex` is read by `lookup`,
/// which a record reaches only from `get`, after `protocolHooks.publicMethod`
/// finds `recordMethods.lookup`: both wait for `get`. The other callers of
/// `lookup` take maps and sets: their own prototypes' methods and set
/// equality, whose other value implements `ISet`.
#[test]
fn kind_hooks_skip_records() {
    let manifest = manifest();
    let unit = |name: &str| manifest.unit_named(name).unwrap();
    let body = |name: &str| &ANNOTATED_RUNTIME[manifest.units[unit(name).0].body_range.clone()];
    let waits_for = |effect: &str, condition: &str| {
        assert!(
            manifest.conditions(unit(effect)).contains(&unit(condition)),
            "`{effect}` waits for `{condition}`"
        );
    };
    for effect in ["recordMethods.equiv", "protocolHooks.builtin"] {
        waits_for(effect, "equalPair");
    }
    assert!(
        manifest.units[unit("structuralEqualPair").0]
            .requires
            .contains(&"equalPair")
    );
    let callers = |callee: &str| {
        let call = format!("{callee}(");
        manifest
            .units
            .iter()
            .filter(|other| {
                let text = &ANNOTATED_RUNTIME[other.body_range.clone()];
                text.match_indices(&call).any(|(start, _)| {
                    !text[..start]
                        .ends_with(|c: char| c.is_ascii_alphanumeric() || c == '.' || c == '_')
                })
            })
            .map(|other| other.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        callers("structuralEqualPair"),
        ["objectEquiv", "equalPair", "structuralEqualPair"]
    );
    assert_eq!(
        callers("lookup"),
        [
            "mapPrototype",
            "setPrototype",
            "lookup",
            "get",
            "makeSet.equiv"
        ]
    );
    for effect in [
        "recordMethods.lookup",
        "protocolHooks.ownMethod",
        "protocolHooks.publicMethod",
    ] {
        waits_for(effect, "get");
    }
    assert!(body("get").contains("protocolHooks.publicMethod?.(value, \"ILookup/-lookup\""));
}

/// `PROTOCOL_ROOTS` are exactly the core protocols that conditional effects
/// target, and every runtime unit that installs a marker of one, as
/// `Eduction` does for `ISequential`, is a target of those effects too. Records
/// implement only `ISeqable`, whose one effect, `count.seqs`, they never need:
/// `count` asks their `ICounted` method first.
#[test]
fn protocol_roots_are_the_protocols_effects_wait_for() {
    use light_cljs::runtime_slice::PROTOCOL_ROOTS;
    let manifest = manifest();
    let protocols = &manifest.units[manifest.unit_named("coreProtocolNames").unwrap().0].requires;
    let mut targeted = Vec::new();
    for (index, effect_unit) in manifest.units.iter().enumerate() {
        let Some(effect) = effect_unit.effect.as_ref().filter(|e| !e.when.is_empty()) else {
            continue;
        };
        for &protocol in effect.targets.iter().filter(|t| protocols.contains(t)) {
            targeted.push(protocol);
            let marker = format!("[\"{protocol}\", 0, null]");
            for installer in &manifest.units {
                let text = &ANNOTATED_RUNTIME[installer.body_range.clone()];
                if !text.contains(&marker) {
                    continue;
                }
                let owners: Vec<&str> = match &installer.effect {
                    Some(own) if installer.declares.is_empty() => own.targets.clone(),
                    _ => installer.declares.clone(),
                };
                assert!(
                    owners.iter().any(|owner| effect.targets.contains(owner)),
                    "`{}` implements `{protocol}`, so `{}` targets one of {owners:?}",
                    installer.name,
                    manifest.units[index].name
                );
            }
        }
    }
    targeted.sort_unstable();
    targeted.dedup();
    assert_eq!(targeted, PROTOCOL_ROOTS);
    let record = &ANNOTATED_RUNTIME[manifest.units
        [manifest.unit_named("recordMethods").unwrap().0]
        .body_range
        .clone()];
    let unit = |name: &str| manifest.unit_named(name).unwrap();
    let implements = |protocol: &str| {
        record
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word == protocol)
    };
    for protocol in PROTOCOL_ROOTS {
        let implemented = implements(protocol);
        if *protocol != "ISeqable" {
            assert!(!implemented, "records implement `{protocol}`");
            continue;
        }
        // `count.seqs` installs the branch of `count` for values implementing
        // `ISeqable` without `ICounted`; `count.protocol` asks a value's
        // `ICounted` method first, which `recordMethods.count` gives a record
        // wherever `count` is kept.
        assert!(implemented);
        let effects: Vec<&str> = manifest
            .units
            .iter()
            .filter(|other| {
                other
                    .effect
                    .as_ref()
                    .is_some_and(|effect| effect.targets.contains(protocol))
            })
            .map(|other| other.name)
            .collect();
        assert_eq!(effects, ["count.seqs"]);
        for effect in ["count.seqs", "count.protocol", "recordMethods.count"] {
            assert!(
                manifest.conditions(unit(effect)).contains(&unit("count")),
                "`{effect}` waits for `count`"
            );
        }
        assert_eq!(manifest.conditions(unit("count.seqs")), [unit("count")]);
        assert!(implements("ICounted"));
    }
}

/// `RECORD_METHODS` lists exactly the methods that the effects of
/// `recordMethods` install, at the arities they install, each with a unit of
/// the installing effect's `when:` list, which the slice of a program naming
/// one of its slots roots (light-cljs-250).
#[test]
fn record_methods_name_a_condition_of_the_effect_installing_each() {
    use light_cljs::runtime_slice::RECORD_METHODS;
    let manifest = manifest();
    let mut installed = Vec::new();
    for unit in &manifest.units {
        let Some(effect) = unit
            .effect
            .as_ref()
            .filter(|effect| effect.targets == ["recordMethods"] && !effect.when.is_empty())
        else {
            continue;
        };
        let text = &ANNOTATED_RUNTIME[unit.body_range.clone()];
        // Method entries: `["ILookup/-lookup", 2, function ...]`.
        for (start, _) in text.match_indices("[\"") {
            let Some((key, rest)) = text[start + 2..].split_once("\", ") else {
                continue;
            };
            let arity = rest.split(',').next().unwrap_or("");
            if method_keys(&format!("[\"{key}\"")) != [(key, true)]
                || arity.is_empty()
                || !arity.chars().all(|c| c.is_ascii_digit())
            {
                continue;
            }
            let (_, _, condition) = RECORD_METHODS
                .iter()
                .find(|(listed, ..)| *listed == key)
                .unwrap_or_else(|| panic!("`{}` installs `{key}`", unit.name));
            assert!(
                effect.when.contains(condition),
                "`{}` waits for {:?}, not `{condition}`",
                unit.name,
                effect.when
            );
            installed.push((key, arity.parse::<usize>().unwrap()));
        }
    }
    installed.sort_unstable();
    let mut listed: Vec<(&str, usize)> = RECORD_METHODS
        .iter()
        .flat_map(|(key, arities, _)| arities.iter().map(move |&arity| (*key, arity)))
        .collect();
    listed.sort_unstable();
    assert_eq!(installed, listed);
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

/// The units that share state through another unit's binding
/// (`RuntimeManifest::sharers`), which a chunked build keeps in that unit's
/// chunk (light-cljs-279): the effects that wrap an operation by assigning
/// its binding, the readers of a binding a function assigns after the
/// runtime loads, and the effects that append to a list another unit
/// enumerates (`recordMethods.*` to `recordMethodTable`). A new one moves no
/// unit out of its binding's chunk.
#[test]
fn units_assigning_a_binding_share_its_unit() {
    let manifest = manifest();
    let mut shared = Vec::new();
    for (index, unit) in manifest.units.iter().enumerate() {
        for &sharer in manifest.sharers(UnitId(index)) {
            shared.push(format!("{} <- {}", unit.name, manifest.unit(sharer).name));
        }
    }
    shared.sort();
    assert_eq!(
        shared,
        [
            "copyEntries <- copyEntries.trie",
            "count <- count.branches",
            "count <- count.protocol",
            "declarationJournal <- addMethod",
            "declarationJournal <- declarationTransaction",
            "declarationJournal <- installMethods",
            "declarationJournal <- multiState",
            "demandProtocol <- addMethod",
            "demandProtocol <- declarationReceiver",
            "demandProtocol <- directMethod",
            "demandProtocol <- getMethod",
            "demandProtocol <- methods",
            "demandProtocol <- preferMethod",
            "demandProtocol <- prefers",
            "demandProtocol <- protocolPredicate",
            "demandProtocol <- publicMethod",
            "demandProtocol <- removeMethod",
            "demandProtocol <- resetMulti",
            "demandProtocol <- satisfies",
            "demandProtocol <- setDeclarationDemand",
            "derefProtocol <- Loadable.deref",
            "first <- first.branches",
            "first <- first.protocol",
            "gensymCounter <- gensym",
            "globalHierarchy <- getGlobalHierarchy",
            "identityCounter <- hash",
            "lookup <- lookup.members",
            "lookup <- lookup.trie",
            "macroexpandServices <- expansionService",
            "macroexpandServices <- setMacroexpandServices",
            "next <- next.branches",
            "next <- next.protocol",
            "nth <- nth.protocol",
            "patchDeclaration <- declarationTransaction",
            "patchDeclaration <- extendType",
            "patchDeclaration <- installMethods",
            "recordMethodTable <- recordMethods.assoc",
            "recordMethodTable <- recordMethods.clone",
            "recordMethodTable <- recordMethods.conj",
            "recordMethodTable <- recordMethods.contains",
            "recordMethodTable <- recordMethods.count",
            "recordMethodTable <- recordMethods.dissoc",
            "recordMethodTable <- recordMethods.equiv",
            "recordMethodTable <- recordMethods.hash",
            "recordMethodTable <- recordMethods.iterator",
            "recordMethodTable <- recordMethods.kvReduce",
            "recordMethodTable <- recordMethods.lookup",
            "recordMethodTable <- recordMethods.meta",
            "recordMethodTable <- recordMethods.print",
            "recordMethodTable <- recordMethods.seq",
            "recordMethodTable <- recordMethods.withMeta",
            "reduce <- reduce.protocol",
            "rest <- rest.branches",
            "rest <- rest.protocol",
            "seq <- seq.branches",
            "seq <- seq.protocol",
            "setDeclaration <- declarationTransaction",
            "setDeclaration <- extendType",
            "setDeclaration <- protocol",
            "setDeclaration <- protocolMethod",
            "tapDraining <- drainTaps",
            "tapDraining <- tapGreater",
            "tapLimit <- configureHostHooks",
            "tapLimit <- drainTaps",
            "tapLimit <- tapGreater",
        ]
    );
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
    // branches of `count`, structural equality, hashing, `str` and the
    // printer, and the cells' kind table entries (light-cljs-237).
    assert_eq!(
        manifest.attached_effects(unit("ListValue")),
        [
            unit("ListValue.type"),
            unit("seq.branches"),
            unit("seq.cells"),
            unit("first.branches"),
            unit("first.cells"),
            unit("rest.branches"),
            unit("rest.cells"),
            unit("next.branches"),
            unit("next.cells"),
            unit("count.branches"),
            unit("count.seqs"),
            unit("count.cells"),
            unit("cons.equiv"),
            unit("makeVector.hash"),
            unit("printsMeta.print"),
            unit("cons.print"),
            unit("printSequential.seq"),
            unit("str.collections"),
            unit("ListValue.fields"),
            unit("ListValue.members"),
            unit("ListValue.statics")
        ]
    );
    let list = manifest.reachable([unit("ListValue")]);
    assert!(list[unit("ListValue.members").0] && !list[unit("Eduction.members").0]);
    // A type's upstream fields wait for a program that names one, or assigns
    // `meta` through `reset-meta!`.
    assert!(!list[unit("ListValue.fields").0]);
    for condition in ["upstreamMembers", "resetMeta"] {
        let kept = manifest.reachable([unit("ListValue"), unit(condition)]);
        assert!(kept[unit("ListValue.fields").0] && !kept[unit("Cons.fields").0]);
    }
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

/// `Advanced` output brands the instances of a type's own constructor only
/// where its slice keeps a unit that reads a brand (light-cljs-262), so the
/// list of those units is every unit that requires `TypeBrand` except
/// `makeType`, which writes the brand of its own instances.
#[test]
fn brand_readers_are_the_units_that_require_type_brand() {
    let readers = manifest()
        .units
        .iter()
        .filter(|unit| unit.name != "makeType" && unit.requires.contains(&"TypeBrand"))
        .map(|unit| unit.name)
        .collect::<Vec<_>>();
    assert_eq!(readers, light_cljs::runtime_slice::BRAND_READERS);
}
