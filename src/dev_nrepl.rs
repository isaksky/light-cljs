//! The nREPL server of `--watch --serve` (docs/dev-workflow.md, "The
//! connected REPL"; docs/evaluation.md, "nREPL"): the editor's path to the
//! connected REPL. It speaks bencode over TCP, on the server's own port (a
//! connection whose first byte opens a dictionary) and on the port of
//! `--nrepl PORT`, with sessions and the operations of [`OPS`].
//!
//! An nREPL session is a session of the connected REPL
//! ([`crate::dev_server`]): its forms compile with the watch's session in
//! its namespace and run in its runtime, which prints their values, as for
//! `--repl --connect`. `completions` and `lookup` are answered from the
//! retained program's definitions and their metadata, on the compiler's
//! thread, without a runtime round trip. `interrupt` stops the wait for the
//! runtime's reply; the runtime finishes what it was sent.

use crate::compiler_data::{DatumKind, Form};
use crate::dev_repl::ReplRequest;
use crate::dev_server::{Evaluation, FormError, FormResult, Interrupt, Shared, evaluate, placed};
use crate::dev_session::DevSession;
use crate::eval_session::PrintOptions;
use crate::source_provider::SourceInput;
use crate::{CollectionKind, ExprKind, Program};
use std::collections::{BTreeMap, HashMap};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The operations the server answers, as `describe` lists them.
pub const OPS: [&str; 9] = [
    "clone",
    "close",
    "completions",
    "describe",
    "eval",
    "interrupt",
    "load-file",
    "lookup",
    "ls-sessions",
];

/// The most bytes one message may take, as for a request of the dev server.
const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// How deeply lists and dictionaries may nest in a message.
const MAX_DEPTH: usize = 64;
/// How long a write to a client that stopped reading may take.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// A bencoded value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bencode {
    Integer(i64),
    Bytes(Vec<u8>),
    List(Vec<Bencode>),
    /// Entries in their order in the text.
    Dict(Vec<(Vec<u8>, Bencode)>),
}

impl Bencode {
    /// The text of a byte string.
    pub fn text(text: &str) -> Self {
        Bencode::Bytes(text.as_bytes().to_vec())
    }

    /// A list of byte strings.
    pub fn texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        Bencode::List(texts.into_iter().map(Bencode::text).collect())
    }

    /// A dictionary of `entries`.
    pub fn dict<'a>(entries: impl IntoIterator<Item = (&'a str, Bencode)>) -> Self {
        Bencode::Dict(
            entries
                .into_iter()
                .map(|(key, value)| (key.as_bytes().to_vec(), value))
                .collect(),
        )
    }

    /// The entry `key` of a dictionary.
    pub fn get(&self, key: &str) -> Option<&Bencode> {
        match self {
            Bencode::Dict(entries) => entries
                .iter()
                .find(|(name, _)| name == key.as_bytes())
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// A byte string's text, when it is UTF-8.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Bencode::Bytes(bytes) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    /// Append the value's bencoding.
    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Bencode::Integer(n) => {
                out.push(b'i');
                out.extend_from_slice(n.to_string().as_bytes());
                out.push(b'e');
            }
            Bencode::Bytes(bytes) => {
                out.extend_from_slice(bytes.len().to_string().as_bytes());
                out.push(b':');
                out.extend_from_slice(bytes);
            }
            Bencode::List(items) => {
                out.push(b'l');
                for item in items {
                    item.write(out);
                }
                out.push(b'e');
            }
            Bencode::Dict(entries) => {
                out.push(b'd');
                for (key, value) in entries {
                    Bencode::Bytes(key.clone()).write(out);
                    value.write(out);
                }
                out.push(b'e');
            }
        }
    }

    /// Read one value from `reader`: None at the end of the input before
    /// its first byte. A value over [`MAX_MESSAGE_BYTES`] or nested deeper
    /// than [`MAX_DEPTH`] is an error, as is a dictionary key that is not a
    /// byte string.
    pub fn read(reader: &mut impl BufRead) -> io::Result<Option<Bencode>> {
        let mut budget = MAX_MESSAGE_BYTES;
        let Some(first) = byte(reader, &mut budget, true)? else {
            return Ok(None);
        };
        value(reader, first, 0, &mut budget).map(Some)
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// The next byte, None at the end of the input when `end_ok`.
fn byte(reader: &mut impl BufRead, budget: &mut usize, end_ok: bool) -> io::Result<Option<u8>> {
    let mut one = [0; 1];
    loop {
        match reader.read(&mut one) {
            Ok(0) if end_ok => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| invalid("an nREPL message is over 16 MiB"))?;
    Ok(Some(one[0]))
}

fn next(reader: &mut impl BufRead, budget: &mut usize) -> io::Result<u8> {
    Ok(byte(reader, budget, false)?.expect("a byte or an error"))
}

/// The digits after `first` up to `end`, as a number.
fn number(reader: &mut impl BufRead, first: u8, end: u8, budget: &mut usize) -> io::Result<i64> {
    let mut text = vec![first];
    loop {
        let byte = next(reader, budget)?;
        if byte == end {
            break;
        }
        if text.len() > 19 {
            return Err(invalid("a bencoded number is too long"));
        }
        text.push(byte);
    }
    let text = std::str::from_utf8(&text).map_err(|_| invalid("a malformed bencoded number"))?;
    let valid = text.strip_prefix('-').unwrap_or(text);
    if valid.is_empty()
        || !valid.bytes().all(|b| b.is_ascii_digit())
        || (valid.len() > 1 && valid.starts_with('0'))
        || text == "-0"
    {
        return Err(invalid("a malformed bencoded number"));
    }
    text.parse()
        .map_err(|_| invalid("a malformed bencoded number"))
}

fn value(
    reader: &mut impl BufRead,
    first: u8,
    depth: usize,
    budget: &mut usize,
) -> io::Result<Bencode> {
    if depth > MAX_DEPTH {
        return Err(invalid("an nREPL message nests too deeply"));
    }
    match first {
        b'i' => {
            let first = next(reader, budget)?;
            number(reader, first, b'e', budget).map(Bencode::Integer)
        }
        b'0'..=b'9' => {
            let length = usize::try_from(number(reader, first, b':', budget)?)
                .map_err(|_| invalid("a malformed bencoded length"))?;
            if length > *budget {
                return Err(invalid("an nREPL message is over 16 MiB"));
            }
            *budget -= length;
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes)?;
            Ok(Bencode::Bytes(bytes))
        }
        b'l' => {
            let mut items = Vec::new();
            loop {
                let byte = next(reader, budget)?;
                if byte == b'e' {
                    return Ok(Bencode::List(items));
                }
                items.push(value(reader, byte, depth + 1, budget)?);
            }
        }
        b'd' => {
            let mut entries = Vec::new();
            loop {
                let byte = next(reader, budget)?;
                if byte == b'e' {
                    return Ok(Bencode::Dict(entries));
                }
                let Bencode::Bytes(key) = value(reader, byte, depth + 1, budget)? else {
                    return Err(invalid("a bencoded dictionary key is not a byte string"));
                };
                let byte = next(reader, budget)?;
                entries.push((key, value(reader, byte, depth + 1, budget)?));
            }
        }
        _ => Err(invalid("not bencode")),
    }
}

/// A name that completes a prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// The text that replaces the prefix.
    pub candidate: String,
    /// The namespace that defines it, if it names a definition.
    pub namespace: Option<String>,
    /// `function`, `macro`, `var` or `namespace`.
    pub kind: &'static str,
}

/// What a symbol names, as `lookup` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub namespace: String,
    pub name: String,
    /// `function`, `macro` or `var`.
    pub kind: &'static str,
    /// The arglists, printed: `([x] [x y])`.
    pub arglists: Option<String>,
    pub doc: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub column: Option<i64>,
}

/// Whether `namespace` is ClojureScript's core.
fn core_namespace(namespace: &str) -> bool {
    matches!(namespace, "cljs.core" | "clojure.core")
}

/// The public names of cljs.core the compiler provides, with their kinds:
/// `macro`, `function` for a Var whose pinned metadata gives arglists, and
/// `var`.
fn core_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    static NAMES: std::sync::OnceLock<Vec<(&'static str, &'static str)>> =
        std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            let functions = include_str!("core_var_metadata.tsv")
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .filter(|(_, metadata)| metadata.contains(":arglists (["))
                .map(|(name, _)| name)
                .collect::<std::collections::HashSet<_>>();
            include_str!("core_public_bindings.tsv")
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .filter(|(name, _)| {
                    crate::CoreFnId::resolve(name).is_some()
                        || crate::compiler_names::core_macro(name)
                })
                .map(|(name, kind)| {
                    let kind = match kind {
                        "Macro" => "macro",
                        _ if functions.contains(name) => "function",
                        _ => "var",
                    };
                    (name, kind)
                })
                .collect()
        })
        .iter()
        .copied()
}

/// The retained program's view of the session's namespaces, for completions
/// and lookups.
struct Names<'a> {
    session: &'a DevSession,
    program: &'a Program,
    globals: &'a crate::emitter::NamespaceGlobals,
    /// The macro session, which keeps the source macros' metadata.
    host: &'a crate::MacroHost,
}

/// A definition the retained program has: its name, whether it is private
/// and its metadata.
struct Definition<'a> {
    name: &'a str,
    private: bool,
    metadata: Option<crate::ExprId>,
}

impl<'a> Names<'a> {
    fn context(&self, namespace: &str) -> Option<&'a crate::analyzer::RuntimeReaderNamespace> {
        self.program
            .reader_contexts
            .get(namespace)
            .map(|context| &**context)
    }

    /// The namespace `qualifier` names in `namespace`: an alias's, or the
    /// namespace itself.
    fn qualified(&self, namespace: &str, qualifier: &str) -> String {
        self.context(namespace)
            .and_then(|context| context.aliases.iter().find(|(alias, _)| alias == qualifier))
            .map_or_else(|| qualifier.to_owned(), |(_, target)| target.clone())
    }

    /// The definitions of `namespace`, in definition order.
    fn definitions(&self, namespace: &str) -> impl Iterator<Item = Definition<'a>> + 'a {
        let program = self.program;
        self.globals
            .get(namespace)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(move |id| {
                let global = &program.globals[id.0];
                global.declared.then(|| Definition {
                    name: &global.name,
                    private: global
                        .metadata
                        .and_then(|metadata| entry(program, metadata, "private"))
                        .is_some_and(|value| {
                            matches!(program.exprs[value.0].kind, ExprKind::Bool(true))
                        }),
                    metadata: global.metadata,
                })
            })
    }

    /// The macros `namespace` declares.
    fn macros(&self, namespace: &str) -> impl Iterator<Item = &'a str> + 'a {
        self.context(namespace)
            .into_iter()
            .flat_map(|context| context.macros.iter().map(String::as_str))
    }

    /// The names `namespace` defines that other namespaces may name, with
    /// their kinds, or with `private` all of them.
    fn public(&self, namespace: &str, private: bool) -> Vec<(String, &'static str)> {
        if core_namespace(namespace) {
            return core_names()
                .map(|(name, kind)| (name.to_owned(), kind))
                .collect();
        }
        let mut names = self
            .definitions(namespace)
            .filter(|definition| private || !definition.private)
            .map(|definition| (definition.name.to_owned(), self.kind(definition.metadata)))
            .collect::<Vec<_>>();
        names.extend(
            self.macros(namespace)
                .filter(|name| private || !self.host.source_binding_private(namespace, name))
                .map(|name| (name.to_owned(), "macro")),
        );
        names
    }

    /// The kind of a definition with `metadata`: a function has arglists.
    fn kind(&self, metadata: Option<crate::ExprId>) -> &'static str {
        match metadata.and_then(|metadata| entry(self.program, metadata, "arglists")) {
            Some(arglists) => match &self.program.exprs[arglists.0].kind {
                ExprKind::List(items) | ExprKind::Vector(items) if !items.is_empty() => "function",
                _ => "var",
            },
            None => "var",
        }
    }

    /// The namespace and name `symbol` names in `namespace`.
    fn resolve(&self, namespace: &str, symbol: &str) -> Option<(String, String)> {
        if let Some((qualifier, name)) = symbol.split_once('/')
            && !qualifier.is_empty()
            && !name.is_empty()
        {
            return Some((self.qualified(namespace, qualifier), name.to_owned()));
        }
        if self.definitions(namespace).any(|d| d.name == symbol)
            || self.macros(namespace).any(|name| name == symbol)
        {
            return Some((namespace.to_owned(), symbol.to_owned()));
        }
        let context = self.context(namespace);
        if let Some((_, target, name)) = context
            .into_iter()
            .flat_map(|context| context.refers.iter())
            .find(|(local, _, _)| local == symbol)
        {
            return Some((target.clone(), name.clone()));
        }
        let excluded = context.is_some_and(|context| context.excludes.iter().any(|n| n == symbol));
        (!excluded && core_names().any(|(name, _)| name == symbol))
            .then(|| ("cljs.core".to_owned(), symbol.to_owned()))
    }

    /// What `namespace/name` is.
    fn info(&self, namespace: &str, name: &str) -> Option<Info> {
        if core_namespace(namespace) {
            let (_, kind) = core_names().find(|(core, _)| *core == name)?;
            let mut info = Info {
                namespace: "cljs.core".into(),
                name: name.into(),
                kind,
                ..Info::default()
            };
            if let Some(metadata) = crate::compiler_reader::core_metadata(name) {
                let source = SourceInput::new("<core metadata>", metadata);
                if let Some(map) = self
                    .session
                    .read_forms(&source)
                    .ok()
                    .and_then(|forms| forms.first().map(Form::data))
                {
                    datum_info(&map, &mut info);
                }
            }
            return Some(info);
        }
        if let Some(definition) = self.definitions(namespace).find(|d| d.name == name) {
            let program = self.program;
            let text = |key: &str| {
                let value = entry(program, definition.metadata?, key)?;
                match &program.exprs[value.0].kind {
                    ExprKind::String(id) => Some(String::from_utf16_lossy(&program.strings[id.0])),
                    _ => None,
                }
            };
            let number = |key: &str| {
                let value = entry(program, definition.metadata?, key)?;
                match program.exprs[value.0].kind {
                    ExprKind::Number(n) if n.fract() == 0.0 => Some(n as i64),
                    _ => None,
                }
            };
            let arglists = definition
                .metadata
                .and_then(|metadata| entry(program, metadata, "arglists"))
                .map(|arglists| {
                    let mut out = String::new();
                    expr_edn(program, arglists, &mut out);
                    out
                });
            let macro_ = self.macros(namespace).any(|macro_| macro_ == name);
            return Some(Info {
                namespace: namespace.into(),
                name: name.into(),
                kind: if macro_ {
                    "macro"
                } else {
                    self.kind(definition.metadata)
                },
                arglists,
                doc: text("doc"),
                file: text("file"),
                line: number("line"),
                column: number("column"),
            });
        }
        if !self.macros(namespace).any(|macro_| macro_ == name) {
            return None;
        }
        let mut info = Info {
            namespace: namespace.into(),
            name: name.into(),
            kind: "macro",
            ..Info::default()
        };
        if let Some(metadata) = self
            .host
            .tables
            .javascript_macros
            .get(&format!("{namespace}/{name}"))
            .and_then(|declaration| declaration.metadata.as_ref())
        {
            datum_info(metadata, &mut info);
        }
        Some(info)
    }
}

/// Fill `info` from `metadata`, a metadata map as data: a core
/// definition's pinned metadata or a source macro's.
fn datum_info(metadata: &crate::compiler_data::Datum, info: &mut Info) {
    let DatumKind::Collection {
        kind: CollectionKind::Map,
        children,
    } = metadata.kind()
    else {
        return;
    };
    for pair in children.as_chunks::<2>().0 {
        let DatumKind::Keyword(key) = pair[0].kind().clone() else {
            continue;
        };
        let value = &pair[1];
        match (String::from_utf16_lossy(key.name()).as_str(), value.kind()) {
            ("doc", DatumKind::String(text)) => info.doc = Some(String::from_utf16_lossy(text)),
            ("file", DatumKind::String(text)) => {
                info.file = Some(String::from_utf16_lossy(text));
            }
            ("line", DatumKind::Integer(n)) => info.line = Some(*n),
            ("column", DatumKind::Integer(n)) => info.column = Some(*n),
            ("arglists", kind) => {
                // A macro's arglists come quoted.
                let value = match kind {
                    DatumKind::Collection {
                        kind: CollectionKind::List,
                        children,
                    } if children.len() == 2
                        && matches!(children[0].kind(), DatumKind::Symbol(name)
                                if name.namespace().is_none()
                                    && String::from_utf16_lossy(name.name()) == "quote") =>
                    {
                        &children[1]
                    }
                    _ => value,
                };
                let mut out = String::new();
                datum_edn(value, &mut out);
                info.arglists = Some(out);
            }
            _ => {}
        }
    }
}

/// The value of the keyword entry `key` of the metadata map `metadata`.
fn entry(program: &Program, metadata: crate::ExprId, key: &str) -> Option<crate::ExprId> {
    let (ExprKind::Map(entries) | ExprKind::LiteralMap(entries)) = &program.exprs[metadata.0].kind
    else {
        return None;
    };
    entries.as_chunks::<2>().0.iter().find_map(|pair| {
        let ExprKind::Keyword(name) = program.exprs[pair[0].0].kind else {
            return None;
        };
        let literal = &program.named_literals[name.0];
        (literal.namespace.is_none()
            && program.strings[literal.name.0]
                .iter()
                .copied()
                .eq(key.encode_utf16()))
        .then_some(pair[1])
    })
}

/// Append the data expression `id` as EDN; what is not data prints as `_`.
fn expr_edn(program: &Program, id: crate::ExprId, out: &mut String) {
    let named = |name: crate::NamedLiteralId, out: &mut String| {
        let literal = &program.named_literals[name.0];
        if let Some(namespace) = literal.namespace {
            out.push_str(&String::from_utf16_lossy(&program.strings[namespace.0]));
            out.push('/');
        }
        out.push_str(&String::from_utf16_lossy(&program.strings[literal.name.0]));
    };
    let items = |open: &str, items: &[crate::ExprId], close: char, out: &mut String| {
        out.push_str(open);
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            expr_edn(program, *item, out);
        }
        out.push(close);
    };
    match &program.exprs[id.0].kind {
        ExprKind::Nil => out.push_str("nil"),
        ExprKind::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        ExprKind::Number(n) if n.fract() == 0.0 && n.abs() < 9e15 => {
            out.push_str(&(*n as i64).to_string());
        }
        ExprKind::Number(n) => out.push_str(&n.to_string()),
        ExprKind::String(text) => {
            crate::dev_repl::edn_string(out, &String::from_utf16_lossy(&program.strings[text.0]))
        }
        ExprKind::Keyword(name) => {
            out.push(':');
            named(*name, out);
        }
        ExprKind::Symbol(name) => named(*name, out),
        ExprKind::List(list) => items("(", list, ')', out),
        ExprKind::Vector(vector) => items("[", vector, ']', out),
        ExprKind::Map(map) | ExprKind::LiteralMap(map) => items("{", map, '}', out),
        ExprKind::Set(set) | ExprKind::LiteralSet(set) => items("#{", set, '}', out),
        ExprKind::WithMeta { value, .. } => expr_edn(program, *value, out),
        _ => out.push('_'),
    }
}

/// Append `data` as EDN.
fn datum_edn(data: &crate::compiler_data::Datum, out: &mut String) {
    let name = |name: &crate::compiler_data::Name, out: &mut String| {
        if let Some(namespace) = name.namespace() {
            out.push_str(&String::from_utf16_lossy(namespace));
            out.push('/');
        }
        out.push_str(&String::from_utf16_lossy(name.name()));
    };
    match data.kind() {
        DatumKind::Nil => out.push_str("nil"),
        DatumKind::Boolean(value) => out.push_str(if *value { "true" } else { "false" }),
        DatumKind::Integer(n) => out.push_str(&n.to_string()),
        DatumKind::Double(n) => out.push_str(&n.to_string()),
        DatumKind::String(text) => {
            crate::dev_repl::edn_string(out, &String::from_utf16_lossy(text));
        }
        DatumKind::Symbol(symbol) => name(symbol, out),
        DatumKind::Keyword(keyword) => {
            out.push(':');
            name(keyword, out);
        }
        DatumKind::Collection { kind, children } => {
            let (open, close) = match kind {
                CollectionKind::List => ("(", ')'),
                CollectionKind::AnonymousFn => ("#(", ')'),
                CollectionKind::Vector => ("[", ']'),
                CollectionKind::Map => ("{", '}'),
                CollectionKind::Set => ("#{", '}'),
            };
            out.push_str(open);
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                datum_edn(child, out);
            }
            out.push(close);
        }
        _ => out.push('_'),
    }
}

/// The names that complete `prefix` in `namespace`, sorted: with a
/// qualifier (`str/jo`), the public names of the namespace it names (an
/// alias's or its own name); without one, the namespace's own definitions
/// and macros, those it refers, cljs.core's that it does not exclude, its
/// aliases and the session's namespaces.
pub fn completions(session: &DevSession, namespace: &str, prefix: &str) -> Vec<Completion> {
    let compiler = session.compiler();
    let Some(program) = compiler.program() else {
        return Vec::new();
    };
    let names = Names {
        session,
        program,
        globals: compiler.namespace_globals(),
        host: &compiler.host,
    };
    let mut found = BTreeMap::new();
    let mut add = |candidate: String, namespace: Option<&str>, kind: &'static str| {
        found.entry(candidate.clone()).or_insert(Completion {
            candidate,
            namespace: namespace.map(str::to_owned),
            kind,
        });
    };
    if let Some((qualifier, rest)) = prefix.split_once('/')
        && !qualifier.is_empty()
    {
        let target = names.qualified(namespace, qualifier);
        for (name, kind) in names.public(&target, target == namespace) {
            if name.starts_with(rest) {
                add(format!("{qualifier}/{name}"), Some(&target), kind);
            }
        }
        return found.into_values().collect();
    }
    for (name, kind) in names.public(namespace, true) {
        if name.starts_with(prefix) {
            add(name, Some(namespace), kind);
        }
    }
    let context = names.context(namespace);
    for (local, target, name) in context.into_iter().flat_map(|c| c.refers.iter()) {
        if local.starts_with(prefix) {
            let kind = names.info(target, name).map_or("var", |info| info.kind);
            add(local.clone(), Some(target), kind);
        }
    }
    for (name, kind) in core_names() {
        if name.starts_with(prefix)
            && !context.is_some_and(|context| context.excludes.iter().any(|n| n == name))
        {
            add(name.to_owned(), Some("cljs.core"), kind);
        }
    }
    for (alias, _) in context.into_iter().flat_map(|c| c.aliases.iter()) {
        if alias.starts_with(prefix) {
            add(alias.clone(), None, "namespace");
        }
    }
    for known in program.namespaces.iter() {
        if known.starts_with(prefix) {
            add(known.clone(), None, "namespace");
        }
    }
    found.into_values().collect()
}

/// What `symbol` names in `namespace`, if the retained program knows it.
pub fn lookup(session: &DevSession, namespace: &str, symbol: &str) -> Option<Info> {
    let compiler = session.compiler();
    let program = compiler.program()?;
    let names = Names {
        session,
        program,
        globals: compiler.namespace_globals(),
        host: &compiler.host,
    };
    let (target, name) = names.resolve(namespace, symbol)?;
    names.info(&target, &name)
}

/// The `.nrepl-port` file of a running nREPL server, which editors read to
/// find it: removed when dropped, and on unix also when the process ends on
/// SIGINT, SIGTERM or SIGHUP.
pub struct PortFile {
    path: PathBuf,
    text: String,
}

impl PortFile {
    /// Write `port` into `.nrepl-port` in `directory`.
    pub fn create(directory: &Path, port: u16) -> io::Result<Self> {
        let path = std::path::absolute(directory.join(".nrepl-port"))?;
        let text = port.to_string();
        std::fs::write(&path, &text)?;
        #[cfg(unix)]
        signals::remove_on_exit(&path, &text);
        Ok(Self { path, text })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PortFile {
    fn drop(&mut self) {
        #[cfg(unix)]
        signals::forget();
        // Another server may have written the file since.
        if std::fs::read_to_string(&self.path).is_ok_and(|text| text == self.text) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
mod signals {
    //! Removing the port file when a signal ends the process: the handler
    //! reads the file, unlinks it when it still holds this server's port,
    //! and exits, all with async-signal-safe calls on data prepared before.
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::sync::atomic::{AtomicPtr, Ordering};

    /// The port file's path and the text this server wrote into it.
    struct Owned {
        path: CString,
        text: Vec<u8>,
    }

    static OWNED: AtomicPtr<Owned> = AtomicPtr::new(std::ptr::null_mut());

    unsafe extern "C" {
        fn signal(signum: c_int, handler: usize) -> usize;
        fn open(path: *const c_char, flags: c_int, ...) -> c_int;
        fn read(fd: c_int, buffer: *mut c_void, count: usize) -> isize;
        fn close(fd: c_int) -> c_int;
        fn unlink(path: *const c_char) -> c_int;
        fn _exit(status: c_int) -> !;
    }

    /// SIGHUP, SIGINT and SIGTERM, which are 1, 2 and 15 on every unix.
    const SIGNALS: [c_int; 3] = [1, 2, 15];
    /// O_RDONLY, 0 on every unix.
    const READ_ONLY: c_int = 0;

    extern "C" fn handle(signum: c_int) {
        let owned = OWNED.load(Ordering::SeqCst);
        // SAFETY: `owned` is null or an `Owned` leaked by `remove_on_exit`,
        // never freed; open, read, close, unlink and _exit are
        // async-signal-safe, and the buffer is on the stack.
        unsafe {
            if let Some(owned) = owned.as_ref() {
                let mut buffer = [0u8; 16];
                let fd = open(owned.path.as_ptr(), READ_ONLY);
                if fd >= 0 {
                    let count = read(fd, buffer.as_mut_ptr().cast(), buffer.len());
                    close(fd);
                    // Another server may have written the file since.
                    if usize::try_from(count).is_ok_and(|count| buffer[..count] == owned.text[..]) {
                        unlink(owned.path.as_ptr());
                    }
                }
            }
            _exit(128 + signum)
        }
    }

    pub(super) fn remove_on_exit(path: &std::path::Path, text: &str) {
        let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        let owned = Box::new(Owned {
            path,
            text: text.as_bytes().to_vec(),
        });
        // Leaked: a handler running on another thread may still read an
        // earlier one.
        OWNED.store(Box::into_raw(owned), Ordering::SeqCst);
        for signum in SIGNALS {
            // SAFETY: `handle` is an `extern "C" fn(c_int)`, a valid
            // sighandler_t.
            unsafe {
                signal(signum, handle as extern "C" fn(c_int) as usize);
            }
        }
    }

    pub(super) fn forget() {
        OWNED.store(std::ptr::null_mut(), Ordering::SeqCst);
    }
}

/// An `eval` or `load-file` queued for a session's worker.
struct Job {
    message: Bencode,
    session: u64,
    session_id: String,
    code: String,
    file: String,
    line: Option<usize>,
    column: Option<usize>,
    namespace: Option<String>,
    /// Whether it is a `load-file`, which answers with the value of the
    /// last form alone and leaves the session's namespace as it was.
    last_only: bool,
}

/// One client's connection.
struct Connection {
    shared: Arc<Shared>,
    writer: Arc<Mutex<TcpStream>>,
    /// The eval queue of each session this connection evaluated in.
    workers: HashMap<u64, Sender<Job>>,
    /// The sessions this connection opened, which it closes when it ends.
    opened: Vec<u64>,
    /// The session a message without one uses, opened on first use: its
    /// number and ID.
    own: Option<(u64, String)>,
}

/// Send `response` to the client, with the `id` and `session` of
/// `message`.
fn send(writer: &Mutex<TcpStream>, message: &Bencode, session: Option<&str>, response: Bencode) {
    let Bencode::Dict(mut entries) = response else {
        return;
    };
    if let Some(id) = message.get("id") {
        entries.push((b"id".to_vec(), id.clone()));
    }
    if let Some(session) = session {
        entries.push((b"session".to_vec(), Bencode::text(session)));
    }
    let mut bytes = Vec::new();
    Bencode::Dict(entries).write(&mut bytes);
    let mut stream = writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = stream.write_all(&bytes).and_then(|()| stream.flush());
}

fn status(statuses: &[&str]) -> Bencode {
    Bencode::dict([("status", Bencode::texts(statuses.iter().copied()))])
}

/// Serve one nREPL client until it closes the connection.
pub(crate) fn connection(stream: TcpStream, shared: &Arc<Shared>) -> io::Result<()> {
    // An editor stays connected while idle.
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut connection = Connection {
        shared: shared.clone(),
        writer: Arc::new(Mutex::new(stream)),
        workers: HashMap::new(),
        opened: Vec::new(),
        own: None,
    };
    let result = loop {
        match Bencode::read(&mut reader) {
            Ok(None) => break Ok(()),
            Ok(Some(message @ Bencode::Dict(_))) => connection.message(message),
            Ok(Some(_)) => break Err(invalid("an nREPL message is a dictionary")),
            Err(error) => break Err(error),
        }
    };
    // The client's sessions end with it: their evaluations stop waiting.
    connection.workers.clear();
    for session in connection.opened {
        shared.interrupt(session, None);
        shared.close_session(session);
    }
    let _ = connection
        .writer
        .lock()
        .map(|stream| stream.shutdown(std::net::Shutdown::Both));
    result
}

impl Connection {
    fn reply(&self, message: &Bencode, session: Option<&str>, response: Bencode) {
        send(&self.writer, message, session, response);
    }

    /// The session `message` names, its number and ID, or the connection's
    /// own when it names none; an error was sent when there is none.
    fn session(&mut self, message: &Bencode) -> Option<(u64, String)> {
        match message.get("session").and_then(Bencode::as_str) {
            Some(id) => match self.shared.session(id) {
                Some(number) => Some((number, id.to_owned())),
                None => {
                    self.reply(
                        message,
                        Some(id),
                        status(&["error", "unknown-session", "done"]),
                    );
                    None
                }
            },
            None => {
                if self.own.is_none() {
                    let Some((number, id, _)) = self.shared.open_session(None) else {
                        self.reply(message, None, status(&["error", "no-compiler", "done"]));
                        return None;
                    };
                    self.opened.push(number);
                    self.own = Some((number, id));
                }
                self.own.clone()
            }
        }
    }

    /// The namespace `message` names, or its session's.
    fn namespace(&self, message: &Bencode) -> Option<String> {
        if let Some(namespace) = message.get("ns").and_then(Bencode::as_str)
            && !namespace.is_empty()
        {
            return Some(namespace.to_owned());
        }
        let session = message
            .get("session")
            .and_then(Bencode::as_str)
            .and_then(|id| self.shared.session(id));
        match session.or(self.own.as_ref().map(|(number, _)| *number)) {
            Some(session) => self.shared.session_namespace(session),
            None => self.shared.start_namespace(),
        }
    }

    fn message(&mut self, message: Bencode) {
        let op = message
            .get("op")
            .and_then(Bencode::as_str)
            .unwrap_or("")
            .to_owned();
        match op.as_str() {
            "describe" => {
                let ops = OPS.iter().map(|op| (*op, Bencode::Dict(Vec::new())));
                let versions = Bencode::dict([
                    (
                        "nrepl",
                        Bencode::dict([
                            ("major", Bencode::Integer(1)),
                            ("minor", Bencode::Integer(0)),
                            ("incremental", Bencode::Integer(0)),
                            ("version-string", Bencode::text("1.0.0")),
                        ]),
                    ),
                    (
                        "light-cljs",
                        Bencode::dict([(
                            "version-string",
                            Bencode::text(env!("CARGO_PKG_VERSION")),
                        )]),
                    ),
                ]);
                let mut aux = Vec::new();
                if let Some(namespace) = self.namespace(&message) {
                    aux.push(("current-ns", Bencode::text(&namespace)));
                }
                let session = message.get("session").and_then(Bencode::as_str);
                self.reply(
                    &message,
                    session,
                    Bencode::dict([
                        ("ops", Bencode::dict(ops)),
                        ("versions", versions),
                        ("aux", Bencode::dict(aux)),
                        ("status", Bencode::texts(["done"])),
                    ]),
                );
            }
            "clone" => {
                let from = message
                    .get("session")
                    .and_then(Bencode::as_str)
                    .and_then(|id| self.shared.session(id));
                match self.shared.open_session(from) {
                    Some((number, id, _)) => {
                        self.opened.push(number);
                        let from = message.get("session").and_then(Bencode::as_str);
                        self.reply(
                            &message,
                            from,
                            Bencode::dict([
                                ("new-session", Bencode::text(&id)),
                                ("status", Bencode::texts(["done"])),
                            ]),
                        );
                    }
                    None => self.reply(&message, None, status(&["error", "no-compiler", "done"])),
                }
            }
            "close" => {
                let Some((number, id)) = self.session(&message) else {
                    return;
                };
                self.workers.remove(&number);
                self.shared.interrupt(number, None);
                self.shared.close_session(number);
                self.opened.retain(|opened| *opened != number);
                if self.own.as_ref().is_some_and(|(own, _)| *own == number) {
                    self.own = None;
                }
                self.reply(&message, Some(&id), status(&["session-closed", "done"]));
            }
            "ls-sessions" => {
                let ids = self.shared.session_ids();
                let session = message.get("session").and_then(Bencode::as_str);
                self.reply(
                    &message,
                    session,
                    Bencode::dict([
                        ("sessions", Bencode::texts(ids.iter().map(String::as_str))),
                        ("status", Bencode::texts(["done"])),
                    ]),
                );
            }
            "eval" | "load-file" => self.evaluate(message),
            "interrupt" => {
                let Some((number, id)) = self.session(&message) else {
                    return;
                };
                let asked = message.get("interrupt-id").and_then(Bencode::as_str);
                let statuses: &[&str] = match self.shared.interrupt(number, asked) {
                    Interrupt::Interrupted => &["done"],
                    Interrupt::Idle => &["session-idle", "done"],
                    Interrupt::Mismatch => &["error", "interrupt-id-mismatch", "done"],
                };
                self.reply(&message, Some(&id), status(statuses));
            }
            "completions" | "lookup" => self.ask_compiler(&op, message),
            _ => {
                let session = message.get("session").and_then(Bencode::as_str);
                let mut response = status(&["error", "unknown-op", "done"]);
                if let Bencode::Dict(entries) = &mut response {
                    entries.push((b"op".to_vec(), Bencode::text(&op)));
                }
                self.reply(&message, session, response);
            }
        }
    }

    /// Queue an `eval` or a `load-file` on its session's worker.
    fn evaluate(&mut self, message: Bencode) {
        let load = message.get("op").and_then(Bencode::as_str) == Some("load-file");
        let Some((session, session_id)) = self.session(&message) else {
            return;
        };
        let code = message
            .get(if load { "file" } else { "code" })
            .and_then(Bencode::as_str);
        let Some(code) = code.map(str::to_owned) else {
            let what = if load { "no-file" } else { "no-code" };
            self.reply(
                &message,
                Some(&session_id),
                status(&["error", what, "done"]),
            );
            return;
        };
        let text = |key: &str| {
            message
                .get(key)
                .and_then(Bencode::as_str)
                .map(str::to_owned)
        };
        let position = |key: &str| match message.get(key) {
            Some(Bencode::Integer(n)) => usize::try_from(*n).ok(),
            Some(bytes) => bytes.as_str().and_then(|text| text.parse().ok()),
            None => None,
        };
        let file = if load {
            text("file-path").or_else(|| text("file-name"))
        } else {
            text("file")
        }
        .unwrap_or_else(|| "<nrepl>".to_owned());
        let job = Job {
            session,
            session_id,
            code,
            file,
            line: if load { None } else { position("line") },
            column: if load { None } else { position("column") },
            namespace: text("ns").filter(|namespace| !namespace.is_empty()),
            last_only: load,
            message,
        };
        let worker = self.workers.entry(session).or_insert_with(|| {
            let (sender, jobs) = channel::<Job>();
            let (shared, writer) = (self.shared.clone(), self.writer.clone());
            let _ = std::thread::Builder::new()
                .name("light-cljs nREPL eval".into())
                .spawn(move || {
                    for job in jobs {
                        run(&shared, &writer, job);
                    }
                });
            sender
        });
        let _ = worker.send(job);
    }

    /// Answer `completions` or `lookup` with the compiler's help.
    fn ask_compiler(&self, op: &str, message: Bencode) {
        let session = message
            .get("session")
            .and_then(Bencode::as_str)
            .map(str::to_owned);
        let session = session.as_deref();
        let Some(compiler) = self.shared.compiler() else {
            self.reply(&message, session, status(&["error", "no-compiler", "done"]));
            return;
        };
        let namespace = self.namespace(&message).unwrap_or_default();
        let text = |key: &str| {
            message
                .get(key)
                .and_then(Bencode::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        if op == "completions" {
            let (respond, answer) = channel();
            let request = ReplRequest::Complete {
                namespace,
                prefix: text("prefix"),
                respond,
            };
            let Some(found) = compiler
                .send(request)
                .ok()
                .and_then(|()| answer.recv().ok())
            else {
                self.reply(&message, session, status(&["error", "no-compiler", "done"]));
                return;
            };
            let completions = found.into_iter().map(|completion| {
                let mut entries = vec![
                    ("candidate", Bencode::text(&completion.candidate)),
                    ("type", Bencode::text(completion.kind)),
                ];
                if let Some(namespace) = &completion.namespace {
                    entries.push(("ns", Bencode::text(namespace)));
                }
                Bencode::dict(entries)
            });
            self.reply(
                &message,
                session,
                Bencode::dict([
                    ("completions", Bencode::List(completions.collect())),
                    ("status", Bencode::texts(["done"])),
                ]),
            );
            return;
        }
        let symbol = match text("sym") {
            symbol if symbol.is_empty() => text("symbol"),
            symbol => symbol,
        };
        let (respond, answer) = channel();
        let request = ReplRequest::Lookup {
            namespace,
            symbol,
            respond,
        };
        let Ok(found) = compiler
            .send(request)
            .map_err(drop)
            .and_then(|()| answer.recv().map_err(drop))
        else {
            self.reply(&message, session, status(&["error", "no-compiler", "done"]));
            return;
        };
        let Some(info) = found else {
            self.reply(
                &message,
                session,
                Bencode::dict([
                    ("info", Bencode::Dict(Vec::new())),
                    ("status", Bencode::texts(["done", "no-info"])),
                ]),
            );
            return;
        };
        let mut entries = vec![
            ("ns", Bencode::text(&info.namespace)),
            ("name", Bencode::text(&info.name)),
            ("type", Bencode::text(info.kind)),
        ];
        if info.kind == "macro" {
            entries.push(("macro", Bencode::text("true")));
        }
        for (key, value) in [
            ("arglists-str", &info.arglists),
            ("doc", &info.doc),
            ("file", &info.file),
        ] {
            if let Some(value) = value {
                entries.push((key, Bencode::text(value)));
            }
        }
        for (key, value) in [("line", info.line), ("column", info.column)] {
            if let Some(value) = value {
                entries.push((key, Bencode::Integer(value)));
            }
        }
        self.reply(
            &message,
            session,
            Bencode::dict([
                ("info", Bencode::dict(entries)),
                ("status", Bencode::texts(["done"])),
            ]),
        );
    }
}

/// Run an evaluation on its session's worker and send its responses: a
/// `value` with the `ns` after it per form (the last alone for
/// `load-file`), an `err` and an `ex` for a form that failed and `err` for
/// warnings, then `done`, after `interrupted` when an interrupt ended it.
fn run(shared: &Shared, writer: &Mutex<TcpStream>, job: Job) {
    let session = Some(job.session_id.as_str());
    let message = &job.message;
    let text = placed(&job.code, job.line, job.column);
    let mut last = None;
    let each = &mut |result: &FormResult, namespace: &str| {
        for warning in &result.warnings {
            let text = format!("WARNING: {}\n", located(warning));
            send(
                writer,
                message,
                session,
                Bencode::dict([("err", Bencode::text(&text))]),
            );
        }
        match &result.outcome {
            Ok((value, _)) if job.last_only => last = Some((value.clone(), namespace.to_owned())),
            Ok((value, _)) => send(
                writer,
                message,
                session,
                Bencode::dict([
                    ("value", Bencode::text(value)),
                    ("ns", Bencode::text(namespace)),
                ]),
            ),
            Err(error) => {
                last = None;
                let text = format!("{}\n", error_text(error));
                send(
                    writer,
                    message,
                    session,
                    Bencode::dict([("err", Bencode::text(&text))]),
                );
                let class = format!("light-cljs/{}-error", error.phase);
                send(
                    writer,
                    message,
                    session,
                    Bencode::dict([
                        ("ex", Bencode::text(&class)),
                        ("root-ex", Bencode::text(&class)),
                        ("status", Bencode::texts(["eval-error"])),
                    ]),
                );
            }
        }
    };
    let print = PrintOptions::default();
    // A file loads in its own namespace and leaves the session's as it was.
    let evaluation = Evaluation {
        file: &job.file,
        text: &text,
        print: &print,
        namespace: job.namespace.as_deref(),
        restore: job.last_only,
        message: message.get("id").and_then(Bencode::as_str),
    };
    let outcome = evaluate(shared, job.session, &evaluation, each);
    match outcome {
        Ok(evaluated) => {
            if let Some((value, _)) = last {
                let namespace = &evaluated.namespace;
                send(
                    writer,
                    message,
                    session,
                    Bencode::dict([
                        ("value", Bencode::text(&value)),
                        ("ns", Bencode::text(namespace)),
                    ]),
                );
            }
            if evaluated.interrupted {
                send(writer, message, session, status(&["interrupted"]));
            }
            send(writer, message, session, status(&["done"]));
        }
        Err(error) => {
            let text = format!("{error}\n");
            send(
                writer,
                message,
                session,
                Bencode::dict([("err", Bencode::text(&text))]),
            );
            send(writer, message, session, status(&["error", "done"]));
        }
    }
}

/// A diagnostic as `FILE:LINE:COLUMN: MESSAGE`, as `--repl --connect`
/// prints it.
fn located(diagnostic: &crate::dev_server::Diagnostic) -> String {
    let mut parts = Vec::new();
    if let Some(file) = &diagnostic.file {
        parts.push(file.clone());
    }
    parts.extend(diagnostic.line.map(|n| n.to_string()));
    parts.extend(diagnostic.column.map(|n| n.to_string()));
    if parts.is_empty() {
        diagnostic.message.clone()
    } else {
        format!("{}: {}", parts.join(":"), diagnostic.message)
    }
}

/// A form's error as `--repl --connect` prints it: located, with the stack
/// in place of the message when it names it, and the data.
fn error_text(error: &FormError) -> String {
    let mut text = located(&error.diagnostic);
    let message = &error.diagnostic.message;
    if let Some(stack) = &error.stack
        && stack.contains(message.as_str())
    {
        text.truncate(text.len() - message.len());
        text.push_str(stack);
    }
    if let Some(data) = &error.data {
        text.push_str("\n  data: ");
        text.push_str(data);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8]) -> io::Result<Option<Bencode>> {
        Bencode::read(&mut io::BufReader::new(bytes))
    }

    #[test]
    fn bencode_round_trips_and_refuses_what_is_not_bencode() {
        let message = Bencode::dict([
            ("op", Bencode::text("eval")),
            ("code", Bencode::text("(+ 1 2) \"é\"")),
            ("line", Bencode::Integer(-12)),
            (
                "list",
                Bencode::List(vec![Bencode::Integer(0), Bencode::Dict(Vec::new())]),
            ),
        ]);
        let mut bytes = Vec::new();
        message.write(&mut bytes);
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            "d2:op4:eval4:code12:(+ 1 2) \"é\"4:linei-12e4:listli0edeee"
        );
        let mut reader = io::BufReader::new(&bytes[..]);
        assert_eq!(Bencode::read(&mut reader).unwrap(), Some(message.clone()));
        assert_eq!(Bencode::read(&mut reader).unwrap(), None);
        assert_eq!(
            message.get("code").and_then(Bencode::as_str),
            Some("(+ 1 2) \"é\"")
        );
        for bad in [
            &b"x"[..],
            b"i01e",
            b"i-0e",
            b"ie",
            b"d1:a",
            b"di1ei2ee",
            b"5:abc",
            b"99999999999999999999:a",
            b"20000000:a",
        ] {
            assert!(read(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        let deep = format!("{}{}", "l".repeat(100), "e".repeat(100));
        assert!(read(deep.as_bytes()).is_err());
    }
}
