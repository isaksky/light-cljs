//! The dev build (docs/dev-workflow.md, "The dev build"): a namespace program
//! emitted as a directory whose namespaces a running application can replace
//! one at a time.
//!
//! The directory holds
//!
//! - `runtime.js`, the whole runtime once:
//!   `globalThis.__lightRuntime = (factory)();`;
//! - `client.js`, a stub of the dev client that defines
//!   `globalThis.__lightDev.define`, which records each namespace's unit and
//!   runs it against the runtime at once, keeping its value;
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

/// The stub dev client. Phase 3's client replaces it (docs/dev-workflow.md,
/// "The server and the client").
const CLIENT: &str = r#"// The dev client stub of a light-cljs dev build: `define` records each
// namespace's unit and runs it against the runtime at once. A unit evaluated
// again (a reload) replaces the record and runs again.
globalThis.__lightDev = (() => {
  const units = Object.create(null);
  const values = Object.create(null);
  return {
    units,
    values,
    define(namespace, requires, unit) {
      units[namespace] = {requires, unit};
      values[namespace] = unit(globalThis.__lightRuntime);
      return values[namespace];
    },
  };
})();
"#;

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
    let mut manifest = String::from("{\n  \"entry\": ");
    json_string(&mut manifest, entry_namespace(program));
    manifest.push_str(",\n  \"namespaces\": [");
    for (index, namespace) in program.namespaces.iter().enumerate() {
        let requires = program
            .namespace_requires
            .get(index)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|&required| program.namespaces[required].as_str())
            .collect::<Vec<_>>();
        let file = unit_file(namespace);
        let map = format!("{file}.map");
        let mut define = String::from("globalThis.__lightDev.define(");
        json_string(&mut define, namespace);
        define.push_str(", ");
        json_list(&mut define, &requires);
        define.push_str(", ");
        let (mut unit, mapping) = crate::emitter::emit_namespace_unit(
            program,
            entries[namespace.as_str()],
            index == 0,
            [&define, ");\n"],
        );
        writeln!(unit, "//# sourceMappingURL={map}").unwrap();
        files.push(DevFile {
            name: file.clone(),
            text: unit,
        });
        files.push(DevFile {
            name: map,
            text: mapping.to_json(Some(&file), sources, false),
        });
        manifest.push_str(if index == 0 { "\n" } else { ",\n" });
        manifest.push_str("    {\"name\": ");
        json_string(&mut manifest, namespace);
        manifest.push_str(", \"file\": ");
        json_string(&mut manifest, &file);
        manifest.push_str(", \"requires\": ");
        json_list(&mut manifest, &requires);
        let macros = program.declares_macros(namespace);
        write!(manifest, ", \"macros\": {macros}}}").unwrap();
    }
    manifest.push_str("\n  ]");
    // Hooks in definition order, which is the order of their definitions'
    // metadata maps, analyzed with the definitions; a declaration may have
    // allocated the global earlier. A client runs those before a load in
    // reverse.
    let mut definitions = program
        .globals
        .iter()
        .filter_map(|global| Some((global.metadata?, global)))
        .collect::<Vec<_>>();
    definitions.sort_by_key(|(metadata, _)| metadata.0);
    for phase in ["before-load", "after-load"] {
        write!(manifest, ",\n  \"{phase}\": [").unwrap();
        let asynchronous = format!("{phase}-async");
        let mut first = true;
        for &(metadata, global) in &definitions {
            let asynchronous = if dev_flag(program, metadata, &asynchronous) {
                true
            } else if dev_flag(program, metadata, phase) {
                false
            } else {
                continue;
            };
            manifest.push_str(if first { "\n    " } else { ",\n    " });
            first = false;
            manifest.push_str("{\"name\": ");
            json_string(
                &mut manifest,
                &format!("{}/{}", global.namespace, global.name),
            );
            write!(manifest, ", \"async\": {asynchronous}}}").unwrap();
        }
        manifest.push_str(if first { "]" } else { "\n  ]" });
    }
    manifest.push_str("\n}\n");
    files.push(DevFile {
        name: "manifest.json".into(),
        text: manifest,
    });
    files.push(DevFile {
        name: "index.js".into(),
        text: index(program),
    });
    files
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

/// `index.js`: loads the files of the build in order and sets
/// `globalThis.__lightDevLoaded` to a promise of the entry's value.
fn index(program: &Program) -> String {
    let files = ["runtime.js".to_owned(), "client.js".to_owned()]
        .into_iter()
        .chain(
            program
                .namespaces
                .iter()
                .map(|namespace| unit_file(namespace)),
        )
        .collect::<Vec<_>>();
    let mut list = String::new();
    json_list(
        &mut list,
        &files.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let mut entry = String::new();
    json_string(&mut entry, entry_namespace(program));
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
fn json_list(out: &mut String, items: &[&str]) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        json_string(out, item);
    }
    out.push(']');
}
