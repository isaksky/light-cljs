//! The dev build (docs/dev-workflow.md, "The dev build"): a namespace program
//! emitted as a directory whose namespaces a running application can replace
//! one at a time.
//!
//! The directory holds
//!
//! - `runtime.js`, the whole runtime once:
//!   `globalThis.__lightRuntime = (factory)();`;
//! - `client.js`, the dev client ([`CLIENT`]), which defines
//!   `globalThis.__lightDev.define`, which records each namespace's unit and
//!   runs it against the runtime at once, keeping its value, and in a page
//!   served by `--serve` reads the server's event stream;
//! - one `<namespace>.js` per namespace ([`unit_file`]), in dependency
//!   order: the namespace's forms as an interactive unit (the storage model
//!   of the session, [`crate::emitter`]) inside a `define` call, with its
//!   source map, `<namespace>.js.map`;
//! - `manifest.json`: the entry namespace, the namespaces in load order with
//!   their files, requires and whether they declare macros, and the
//!   functions marked `^:dev/before-load` and `^:dev/after-load` (or their
//!   `-async` variants, which take a continuation), in definition order;
//! - `index.js`, which loads the files in that order (script elements in a
//!   page, `require` in Node) and sets `globalThis.__lightDevLoaded` to a
//!   promise of the entry namespace's value, the program's value, which a
//!   file that throws while it runs rejects instead.
//!
//! Units are emitted at `none` semantics with namespace registration on,
//! from the one whole-program [`Program`] by namespace entry, and never go
//! through [`crate::prepare_application`]. Definitions live in
//! `globalThis.__lightApplicationRoots["ns/name"]`, read at each use, so a
//! unit evaluated again replaces its namespace's definitions for every caller.
//! Each unit registers its namespace's reader context, and the first one
//! registers every namespace of the build, as single-file output does before
//! its first form runs.

use std::collections::HashMap;
use std::fmt::Write;

use crate::analyzer::{ExprId, ExprKind, Program};
use crate::source_map::{SourceFile, json_string};

/// One file of a dev build: its name in the directory and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevFile {
    pub name: String,
    pub text: String,
}

/// The dev client, `client.js` (docs/dev-workflow.md, "The server and the
/// client"): `define`, and the interpreter of a dev server's event stream
/// ([`crate::dev_server`]), which a page served over HTTP connects to.
pub const CLIENT: &str = include_str!("dev_client.js");

/// The files of the dev build of `program`, a program analyzed from
/// namespace sources ([`crate::compiler::analyze_namespace_entry_with_sources`]),
/// in the order to write them. `sources` name the program's sources by source
/// index for the source maps, as URLs relative to the build directory.
///
/// # Panics
///
/// When `program` was not analyzed from namespace sources.
pub fn emit_dev_build(program: &Program, sources: &[SourceFile<'_>]) -> Vec<DevFile> {
    let entries = program
        .namespace_entries
        .iter()
        .map(|(id, namespace)| (namespace.as_str(), *id))
        .collect::<HashMap<_, _>>();
    assert!(
        !program.namespaces.is_empty()
            && program
                .namespaces
                .iter()
                .all(|namespace| entries.contains_key(namespace.as_str())),
        "a dev build needs a program analyzed from namespace sources"
    );
    let mut files = vec![
        DevFile {
            name: "runtime.js".into(),
            text: format!(
                "globalThis.__lightRuntime = {};\n",
                crate::runtime_manifest::RUNTIME.trim()
            ),
        },
        DevFile {
            name: "client.js".into(),
            text: CLIENT.into(),
        },
    ];
    let manifest = DevManifest::new(program);
    for (index, namespace) in manifest.namespaces.iter().enumerate() {
        let map = format!("{}.map", namespace.file);
        let mut define = String::from("globalThis.__lightDev.define(");
        json_string(&mut define, &namespace.name);
        define.push_str(", ");
        json_list(&mut define, &namespace.requires);
        define.push_str(", ");
        let (mut unit, mapping) = crate::emitter::emit_namespace_unit(
            program,
            entries[namespace.name.as_str()],
            index == 0,
            [&define, ");\n"],
        );
        writeln!(unit, "//# sourceMappingURL={map}").unwrap();
        files.push(DevFile {
            name: namespace.file.clone(),
            text: unit,
        });
        files.push(DevFile {
            name: map,
            text: mapping.to_json(Some(&namespace.file), sources, false),
        });
    }
    files.push(DevFile {
        name: "manifest.json".into(),
        text: manifest.to_json(),
    });
    files.push(DevFile {
        name: "index.js".into(),
        text: manifest.index(),
    });
    files
}

/// What `manifest.json` of a dev build says: the entry namespace, the
/// namespaces in load order and the hook functions. A watch step
/// (`--watch`) changes it as namespaces change and are added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevManifest {
    /// The namespace whose value is the program's value.
    pub entry: String,
    /// The namespaces in load order: each after those it requires.
    pub namespaces: Vec<DevNamespace>,
    /// The functions marked `^:dev/before-load` or
    /// `^:dev/before-load-async`, in definition order.
    pub before_load: Vec<DevHook>,
    /// The functions marked `^:dev/after-load` or `^:dev/after-load-async`,
    /// in definition order.
    pub after_load: Vec<DevHook>,
}

/// One namespace of a [`DevManifest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevNamespace {
    pub name: String,
    /// Its unit's file in the build directory ([`unit_file`]).
    pub file: String,
    /// The namespaces of the build it requires.
    pub requires: Vec<String>,
    /// Whether it declares macros, which the units of its requirers expand,
    /// so a change to it recompiles them.
    pub macros: bool,
}

/// A hook function of a [`DevManifest`]: `ns/name`, and whether it is an
/// `-async` hook, which takes a continuation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevHook {
    pub name: String,
    pub asynchronous: bool,
}

impl DevManifest {
    /// The manifest of the dev build of `program`, a program analyzed from
    /// namespace sources.
    pub fn new(program: &Program) -> Self {
        let namespaces = program
            .namespaces
            .iter()
            .enumerate()
            .map(|(index, namespace)| DevNamespace {
                name: namespace.clone(),
                file: unit_file(namespace),
                requires: program
                    .namespace_requires
                    .get(index)
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .map(|&required| program.namespaces[required].clone())
                    .collect(),
                macros: program.declares_macros(namespace),
            })
            .collect();
        let [before_load, after_load] = program_hooks(program, &program.namespaces);
        Self {
            entry: entry_namespace(program).to_owned(),
            namespaces,
            before_load,
            after_load,
        }
    }

    /// The position of `namespace` in load order.
    pub fn position(&self, namespace: &str) -> Option<usize> {
        self.namespaces
            .iter()
            .position(|entry| entry.name == namespace)
    }

    /// Put the namespaces in an order where each follows the namespaces of
    /// the build it requires, keeping the present order where it already
    /// does: a namespace that a changed `ns` form added (which a step places
    /// ahead of its requirer) moves after its own requires. A cycle keeps
    /// the order its first namespace reaches.
    pub fn sort(&mut self) {
        fn visit(
            index: usize,
            namespaces: &[DevNamespace],
            seen: &mut [bool],
            order: &mut Vec<usize>,
        ) {
            if std::mem::replace(&mut seen[index], true) {
                return;
            }
            for required in &namespaces[index].requires {
                if let Some(at) = namespaces.iter().position(|entry| entry.name == *required) {
                    visit(at, namespaces, seen, order);
                }
            }
            order.push(index);
        }
        let mut seen = vec![false; self.namespaces.len()];
        let mut order = Vec::with_capacity(self.namespaces.len());
        for index in 0..self.namespaces.len() {
            visit(index, &self.namespaces, &mut seen, &mut order);
        }
        let mut namespaces = std::mem::take(&mut self.namespaces)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        self.namespaces = order
            .into_iter()
            .map(|index| namespaces[index].take().unwrap())
            .collect();
    }

    /// `manifest.json`.
    pub fn to_json(&self) -> String {
        let mut manifest = String::from("{\n  \"entry\": ");
        json_string(&mut manifest, &self.entry);
        manifest.push_str(",\n  \"namespaces\": [");
        for (index, namespace) in self.namespaces.iter().enumerate() {
            manifest.push_str(if index == 0 { "\n" } else { ",\n" });
            manifest.push_str("    {\"name\": ");
            json_string(&mut manifest, &namespace.name);
            manifest.push_str(", \"file\": ");
            json_string(&mut manifest, &namespace.file);
            manifest.push_str(", \"requires\": ");
            json_list(&mut manifest, &namespace.requires);
            write!(manifest, ", \"macros\": {}}}", namespace.macros).unwrap();
        }
        manifest.push_str("\n  ]");
        for (phase, hooks) in [
            ("before-load", &self.before_load),
            ("after-load", &self.after_load),
        ] {
            write!(manifest, ",\n  \"{phase}\": [").unwrap();
            for (index, hook) in hooks.iter().enumerate() {
                manifest.push_str(if index == 0 { "\n    " } else { ",\n    " });
                manifest.push_str("{\"name\": ");
                json_string(&mut manifest, &hook.name);
                write!(manifest, ", \"async\": {}}}", hook.asynchronous).unwrap();
            }
            manifest.push_str(if hooks.is_empty() { "]" } else { "\n  ]" });
        }
        manifest.push_str("\n}\n");
        manifest
    }
}

/// The hook functions of `program` (docs/dev-workflow.md, "The reload set"):
/// the definitions whose metadata marks them `^:dev/before-load` (or
/// `-async`), then those it marks `^:dev/after-load` (or `-async`), each list
/// in definition order: by namespace in the order of `namespaces`, the load
/// order, then in the order of their definitions' metadata maps, which are
/// analyzed with the definitions (a declaration may have allocated the
/// global earlier). A definition of a namespace not in `namespaces` comes
/// last. In a session's retained program, a redefinition that carries
/// metadata replaces the global's, so a step that recompiles a namespace
/// puts its hooks in source order and drops the flag a definition lost; a
/// definition removed from the source keeps its last metadata, as it stays
/// defined. A client runs the before-load hooks in reverse.
pub(crate) fn program_hooks(program: &Program, namespaces: &[String]) -> [Vec<DevHook>; 2] {
    let position = |namespace: &str| {
        namespaces
            .iter()
            .position(|known| known == namespace)
            .unwrap_or(namespaces.len())
    };
    let mut definitions = program
        .globals
        .iter()
        .filter_map(|global| {
            let metadata = global.metadata?;
            Some((position(&global.namespace), metadata, global))
        })
        .collect::<Vec<_>>();
    definitions.sort_by_key(|&(namespace, metadata, _)| (namespace, metadata.0));
    ["before-load", "after-load"].map(|phase| {
        let asynchronous = format!("{phase}-async");
        definitions
            .iter()
            .filter_map(|&(_, metadata, global)| {
                let asynchronous = if dev_flag(program, metadata, &asynchronous) {
                    true
                } else if dev_flag(program, metadata, phase) {
                    false
                } else {
                    return None;
                };
                Some(DevHook {
                    name: format!("{}/{}", global.namespace, global.name),
                    asynchronous,
                })
            })
            .collect()
    })
}

/// Whether the namespace symbol `symbol`, with the metadata of its `ns`
/// form, marks the namespace `:dev/<name>` with a value other than `nil` or
/// `false`: `^:dev/once` (load once, never reload) or `^:dev/always` (reload
/// with every load).
pub(crate) fn namespace_flag(symbol: &crate::compiler_data::Datum, name: &str) -> bool {
    use crate::compiler_data::{DatumKind, Name};
    use crate::parser::CollectionKind;
    let Some(metadata) = symbol.metadata() else {
        return false;
    };
    let DatumKind::Collection {
        kind: CollectionKind::Map,
        children,
    } = metadata.kind()
    else {
        return false;
    };
    let flag = Name::new(Some("dev"), name);
    children.as_chunks::<2>().0.iter().any(|[key, value]| {
        matches!(key.kind(), DatumKind::Keyword(keyword) if *keyword == flag)
            && !matches!(value.kind(), DatumKind::Nil | DatumKind::Boolean(false))
    })
}

/// The entry namespace: the last one the program loads statically, which
/// namespaces reached only through lazy references follow.
fn entry_namespace(program: &Program) -> &str {
    let entry = program
        .namespace_lazy_only
        .iter()
        .position(|&lazy| lazy)
        .unwrap_or(program.namespaces.len())
        - 1;
    &program.namespaces[entry]
}

/// The unit file of `namespace` written from the session's units of its
/// forms (a watch step), and its source map: the `define` call of an
/// interactive unit, as [`emit_dev_build`] writes, that registers the
/// namespaces `registered` as `session` knows them now, makes `namespace`
/// the current namespace, as a `Namespace` action does, then runs each
/// form's unit in order. With `value` it returns the last one's value, else
/// nil: the value of a namespace whose last form is a directive, or that has
/// no forms. `registered` is `namespace`, or every namespace of the build
/// for the first unit in load order, as the build's units register them;
/// the forms' units of [`crate::dev_session::DevSession::load_namespace_file`]
/// register none, so the file registers its namespace once, with what all
/// its forms defined. The forms' units share the file's prelude: the file's function
/// runs each form's statements in a scope of its own. Where raw JavaScript
/// renamed a unit's names, whose raw text may then name a global `rt`, the
/// file runs each unit whole instead, as a function of the runtime.
/// `requires` are the namespaces of the build it requires; `url` names a
/// unit's source, as the source provider named it, in the map, relative to
/// the directory.
pub fn namespace_unit(
    session: &crate::dev_session::DevSession,
    namespace: &str,
    requires: &[String],
    registered: &[&str],
    units: &[&crate::dev_session::Unit],
    value: bool,
    url: &dyn Fn(&str) -> String,
) -> [DevFile; 2] {
    let file = unit_file(namespace);
    let map = format!("{file}.map");
    let mut text = crate::source_map::MappedText::default();
    let prelude = crate::emitter::interactive_prelude();
    // The lines of the prelude: those of a form's unit before its statements.
    let prelude_lines = prelude.matches('\n').count() as u32;
    // Each unit without its `sourceURL`: the file's `sourceMappingURL` maps
    // it, and the file is what the comment named.
    let texts = units
        .iter()
        .map(|unit| match unit.js.rfind("\n//# sourceURL=") {
            Some(end) => &unit.js[..end],
            None => unit.js.trim_end(),
        })
        .collect::<Vec<_>>();
    let statements = texts
        .iter()
        .map(|js| {
            js.strip_prefix(&prelude)
                .and_then(|rest| rest.strip_suffix(crate::emitter::INTERACTIVE_CLOSE))
        })
        .collect::<Option<Vec<_>>>();
    let mut head = String::from("globalThis.__lightDev.define(");
    json_string(&mut head, namespace);
    head.push_str(", ");
    json_list(&mut head, requires);
    head.push_str(", ");
    let mut setup = session.registration(registered);
    setup.push_str("  rt.core.currentNs = rt.core.createNs(rt.symbol(null, ");
    json_string(&mut setup, namespace);
    setup.push_str("));\n");
    if statements.is_some() {
        head.push_str(&prelude);
        head.push_str(&setup);
    } else {
        write!(
            head,
            "(__lightInteractiveRuntime) => {{\n  ((rt) => {{\n{setup}}})(__lightInteractiveRuntime);\n"
        )
        .unwrap();
    }
    text.push_str(&head);
    // The sources of the units, by their names.
    let mut sources: Vec<&crate::compiler_data::Source> = Vec::new();
    for (index, (unit, js)) in units.iter().zip(&texts).enumerate() {
        let returns = value && index + 1 == units.len();
        let source = unit.location.source();
        let at = match sources
            .iter()
            .position(|known| known.name() == source.name())
        {
            Some(at) => at,
            None => {
                sources.push(source);
                sources.len() - 1
            }
        };
        text.push_str(if returns { "  return (" } else { "  (" });
        match &statements {
            Some(statements) => {
                text.push_str("() => {\n");
                match &unit.mapping {
                    Some(mapping) => {
                        text.push_mapped_lines(statements[index], mapping, &[at], prelude_lines);
                    }
                    None => text.push_str(statements[index]),
                }
                text.push_str("})();\n");
            }
            None => {
                match &unit.mapping {
                    Some(mapping) => text.push_mapped(js, mapping, &[at]),
                    None => text.push_str(js),
                }
                text.push_str(")(__lightInteractiveRuntime);\n");
            }
        }
    }
    if !value || units.is_empty() {
        text.push_str("  return null;\n");
    }
    if statements.is_some() {
        text.push_str(crate::emitter::INTERACTIVE_CLOSE);
        text.push_str(");\n");
    } else {
        text.push_str("});\n");
    }
    let urls = sources
        .iter()
        .map(|source| url(source.name()))
        .collect::<Vec<_>>();
    let files = urls
        .iter()
        .zip(&sources)
        .map(|(url, source)| SourceFile {
            url,
            text: source.text(),
        })
        .collect::<Vec<_>>();
    let json = text.mapping.to_json(Some(&file), &files, false);
    let mut unit = text.text;
    writeln!(unit, "//# sourceMappingURL={map}").unwrap();
    [
        DevFile {
            name: file,
            text: unit,
        },
        DevFile {
            name: map,
            text: json,
        },
    ]
}

/// The file of `namespace`'s unit: `<namespace>.js`, or `<namespace>$.js`
/// for a namespace named like a file of its own (`runtime`, `client`,
/// `index`), since no namespace name contains a `$`.
pub fn unit_file(namespace: &str) -> String {
    if matches!(namespace, "runtime" | "client" | "index") {
        format!("{namespace}$.js")
    } else {
        format!("{namespace}.js")
    }
}

impl DevManifest {
    /// `index.js`: loads the files of the build in order and sets
    /// `globalThis.__lightDevLoaded` to a promise of the entry's value.
    pub fn index(&self) -> String {
        let files = ["runtime.js", "client.js"]
            .into_iter()
            .chain(
                self.namespaces
                    .iter()
                    .map(|namespace| namespace.file.as_str()),
            )
            .collect::<Vec<_>>();
        let mut list = String::new();
        json_list(&mut list, &files);
        let mut entry = String::new();
        json_string(&mut entry, &self.entry);
        index_text(&list, &entry)
    }
}

/// The text of `index.js` for the JSON array `list` of its files and the
/// JSON string `entry`.
fn index_text(list: &str, entry: &str) -> String {
    format!(
        r#"// Loads a light-cljs dev build: the runtime, the dev client and one unit per
// namespace, in dependency order. globalThis.__lightDevLoaded is a promise of
// the entry namespace's value, the program's value. In a page each file is a
// script element after this one, and an error thrown while it runs rejects
// the promise before the next file loads; in Node (CommonJS) each is
// required, which `node --enable-source-maps` maps to the sources.
globalThis.__lightDevLoaded = (async () => {{
  const files = {list};
  if (typeof document === "object" && document !== null) {{
    const base = document.currentScript?.src ?? document.baseURI;
    for (const file of files) {{
      await new Promise((resolve, reject) => {{
        const script = document.createElement("script");
        // Files load one at a time, so an uncaught error until this one's
        // load event is its own.
        let failure = null;
        const failed = event => {{ failure ??= event.error ?? new Error(event.message); }};
        const settle = () => {{
          globalThis.removeEventListener("error", failed);
          if (failure === null) resolve(); else reject(failure);
        }};
        globalThis.addEventListener("error", failed);
        script.src = new URL(file, base).href;
        script.async = false;
        script.onload = settle;
        script.onerror = () => {{ failure ??= new Error("cannot load " + script.src); settle(); }};
        document.head.append(script);
      }});
    }}
  }} else {{
    for (const file of files) require("./" + file);
  }}
  return globalThis.__lightDev.values[{entry}];
}})();
"#
    )
}

/// Whether the metadata map `metadata` has the key `:dev/<name>` with a
/// value other than `nil` or `false`.
fn dev_flag(program: &Program, metadata: ExprId, name: &str) -> bool {
    let ExprKind::Map(entries) = &program.exprs[metadata.0].kind else {
        return false;
    };
    let text = |id: crate::analyzer::StringId| String::from_utf16_lossy(&program.strings[id.0]);
    entries.as_chunks::<2>().0.iter().any(|[key, value]| {
        let ExprKind::Keyword(literal) = program.exprs[key.0].kind else {
            return false;
        };
        let literal = &program.named_literals[literal.0];
        literal
            .namespace
            .is_some_and(|namespace| text(namespace) == "dev")
            && text(literal.name) == name
            && !matches!(
                program.exprs[value.0].kind,
                ExprKind::Nil | ExprKind::Bool(false)
            )
    })
}

/// Append `items` as a JSON array of strings.
fn json_list(out: &mut String, items: &[impl AsRef<str>]) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        json_string(out, item.as_ref());
    }
    out.push(']');
}
