//! `light-cljs.edn`, the configuration file: discovery, reading with the
//! compiler's own reader, `#light-cljs/env`, and the closed schema
//! (docs/build-configuration.md, "The file and where it is found", "Schema",
//! "#light-cljs/env" and "Errors").
//!
//! [`ConfigFile::read`] parses the file with [`crate::parser`], keeping each
//! form's span, resolves every `#light-cljs/env` against the lookup it is
//! given, and validates the whole file before it returns. Each setting a map
//! holds is stored in a slot of an array indexed by [`Setting`]: the top-level
//! settings in one layer, and each build's settings in three, the build's
//! own map and its `:dev` and `:release` maps. Builds are kept in a vector
//! indexed by [`BuildId`], in ascending id order. `LIGHT_CLJS`, read through
//! the same lookup, is deep-merged over the whole file as it is read.
//!
//! [`ConfigFile::settings`] merges one build's layers for a mode
//! ("Precedence", layers 1-5): the built-in defaults, the file's top level,
//! the build, its `:dev` or `:release` map and each [`ConfigMerge`], with
//! shadow-cljs's `deep-merge` ([`deep_merge`]), into [`BuildSettings`], a
//! slot per setting with its [`SettingOrigin`].
//!
//! Every error is located: `PATH:LINE:COLUMN: message`, where PATH is the
//! file's, `LIGHT_CLJS` or `--config-merge N`.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    ops::Range,
    path::{Component, Path, PathBuf},
};

mod print_config;

use crate::BuildConfiguration;
use crate::parser::{
    CollectionKind, LineIndex, Node, NodeId, NodeKind, NumberKind, Parser, PrefixKind,
    SymbolicValue,
};

/// The configuration file's name, after the tool, as `shadow-cljs.edn` is.
pub const FILE_NAME: &str = "light-cljs.edn";

/// The one tag the file reads, shadow-cljs's `#shadow/env` named after the
/// tool.
pub const ENVIRONMENT_TAG: &str = "light-cljs/env";

/// The environment variable whose EDN is deep-merged over the whole file,
/// as shadow-cljs's `SHADOW_CLJS` is.
pub const ENVIRONMENT_VARIABLE: &str = "LIGHT_CLJS";

/// Where a value was written: the file, `LIGHT_CLJS`, the Nth
/// `--config-merge` (from 1), nowhere (a built-in default), or a flag on
/// the command line, layer 6: the Nth (from 0) a build's
/// [`BuildSettings`] took ([`BuildSettings::set_by_flag`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ConfigSource {
    Default,
    File,
    LightCljs,
    ConfigMerge(usize),
    Flag(usize),
}

/// A build's dense id: its position among the file's builds, which are kept
/// in ascending order of their keyword ids.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BuildId(pub u32);

/// Shadow-cljs's modes: `compile` and `watch` run dev mode, `release`
/// release mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BuildMode {
    Dev,
    Release,
}

impl BuildMode {
    /// `dev` or `release`, the mode map's key without its colon.
    pub fn name(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Release => "release",
        }
    }

    fn layer(self) -> usize {
        match self {
            Self::Dev => 1,
            Self::Release => 2,
        }
    }
}

/// Every setting the file holds, one per key position of the schema's
/// tables. The discriminants are dense, so a layer of settings is an array
/// with one slot per setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u8)]
pub enum Setting {
    /// `:source-paths`, top level.
    SourcePaths,
    /// `:cache-root`, top level.
    CacheRoot,
    /// `:cache-blockers`, top level.
    CacheBlockers,
    /// `:compile-threads`, top level.
    CompileThreads,
    /// `:http :port`.
    HttpPort,
    /// `:http :host`.
    HttpHost,
    /// `:http :strict`.
    HttpStrict,
    /// `:nrepl`: `false` or `{:port N}`.
    Nrepl,
    /// `:fs-watch :impl`.
    FsWatchImpl,
    /// `:fs-watch :loop-wait`.
    FsWatchLoopWait,
    /// A build's `:target`.
    Target,
    /// A build's `:output-dir`.
    OutputDir,
    /// A build's `:modules`.
    Modules,
    /// A build's `:config`.
    Config,
    /// A build's own `:closure-defines`.
    ClosureDefines,
    /// `:compiler-options :optimizations`.
    Optimizations,
    /// `:compiler-options :pseudo-names`.
    PseudoNames,
    /// `:compiler-options :elide-asserts`.
    ElideAsserts,
    /// `:compiler-options :checked-arrays`.
    CheckedArrays,
    /// `:compiler-options :cljs-warnings`.
    CljsWarnings,
    /// `:compiler-options :closure-defines`, under the build's own.
    CompilerClosureDefines,
    /// `:compiler-options :macro-expansions`.
    MacroExpansions,
    /// `:compiler-options :macro-nodes`.
    MacroNodes,
    /// `:compiler-options :macro-timeout-ms`.
    MacroTimeoutMs,
    /// `:compiler-options :macro-memory-limit`.
    MacroMemoryLimit,
    /// `:compiler-options :macro-poll-budget`.
    MacroPollBudget,
    /// `:build-options :cache-level`.
    CacheLevel,
    /// `:build-options :cache-blockers`.
    BuildCacheBlockers,
    /// `:devtools :reload-strategy`.
    ReloadStrategy,
}

impl Setting {
    pub const COUNT: usize = Self::ALL.len();

    /// Every setting, in discriminant order.
    pub const ALL: [Setting; 29] = [
        Self::SourcePaths,
        Self::CacheRoot,
        Self::CacheBlockers,
        Self::CompileThreads,
        Self::HttpPort,
        Self::HttpHost,
        Self::HttpStrict,
        Self::Nrepl,
        Self::FsWatchImpl,
        Self::FsWatchLoopWait,
        Self::Target,
        Self::OutputDir,
        Self::Modules,
        Self::Config,
        Self::ClosureDefines,
        Self::Optimizations,
        Self::PseudoNames,
        Self::ElideAsserts,
        Self::CheckedArrays,
        Self::CljsWarnings,
        Self::CompilerClosureDefines,
        Self::MacroExpansions,
        Self::MacroNodes,
        Self::MacroTimeoutMs,
        Self::MacroMemoryLimit,
        Self::MacroPollBudget,
        Self::CacheLevel,
        Self::BuildCacheBlockers,
        Self::ReloadStrategy,
    ];

    /// The slot index.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The key, without its colon, in the map that holds it.
    pub fn key(self) -> &'static str {
        match self {
            Self::SourcePaths => "source-paths",
            Self::CacheRoot => "cache-root",
            Self::CacheBlockers | Self::BuildCacheBlockers => "cache-blockers",
            Self::CompileThreads => "compile-threads",
            Self::HttpPort => "port",
            Self::HttpHost => "host",
            Self::HttpStrict => "strict",
            Self::Nrepl => "nrepl",
            Self::FsWatchImpl => "impl",
            Self::FsWatchLoopWait => "loop-wait",
            Self::Target => "target",
            Self::OutputDir => "output-dir",
            Self::Modules => "modules",
            Self::Config => "config",
            Self::ClosureDefines | Self::CompilerClosureDefines => "closure-defines",
            Self::Optimizations => "optimizations",
            Self::PseudoNames => "pseudo-names",
            Self::ElideAsserts => "elide-asserts",
            Self::CheckedArrays => "checked-arrays",
            Self::CljsWarnings => "cljs-warnings",
            Self::MacroExpansions => "macro-expansions",
            Self::MacroNodes => "macro-nodes",
            Self::MacroTimeoutMs => "macro-timeout-ms",
            Self::MacroMemoryLimit => "macro-memory-limit",
            Self::MacroPollBudget => "macro-poll-budget",
            Self::CacheLevel => "cache-level",
            Self::ReloadStrategy => "reload-strategy",
        }
    }
}

/// A value of the file, as the reader read it, or as `#light-cljs/env`
/// resolved it: the value's kind is what a literal in its place would have,
/// and [`Value::environment`] says where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    pub kind: ValueKind,
    /// The form's bytes in the text of [`Value::source`]; for an environment
    /// value, the whole `#light-cljs/env` form's. Empty for a default.
    pub span: Range<usize>,
    /// The variable an environment value was read from.
    pub environment: Option<EnvironmentValue>,
    /// The text the value was written in. A merged collection is its
    /// highest layer's, and each element keeps its own.
    pub source: ConfigSource,
}

/// The EDN data the file holds.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueKind {
    Nil,
    Boolean(bool),
    /// A number as written: its text, and the reader's kind for it. An
    /// `:as :int` environment value is written in decimal.
    Number {
        kind: NumberKind,
        text: String,
    },
    /// `##Inf`, `##-Inf` or `##NaN`.
    Symbolic(SymbolicValue),
    String(String),
    /// A string holding an unpaired surrogate escape, which is no Rust
    /// `str`, as UTF-16 code units: `:config` takes it, as `--config` does;
    /// no path or host does.
    Utf16String(Vec<u16>),
    /// A keyword's name, without its colon.
    Keyword(String),
    Symbol(String),
    List(Vec<Value>),
    Vector(Vec<Value>),
    Set(Vec<Value>),
    /// Entries in the order written.
    Map(Vec<(Value, Value)>),
    /// A character or regular expression, as written, which no setting takes.
    Other(String),
}

/// Where an environment value came from: `#light-cljs/env` and its
/// variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentValue {
    /// The variable's name.
    pub variable: String,
    /// Whether the variable was set; when it was not, the value is the
    /// form's default, or nil without one.
    pub set: bool,
    /// Whether the form gives a default.
    pub default: bool,
}

impl Value {
    /// A built-in default.
    fn default_of(kind: ValueKind) -> Self {
        Self {
            kind,
            span: 0..0,
            environment: None,
            source: ConfigSource::Default,
        }
    }

    /// The value as EDN text: what a literal in its place would be written.
    pub fn to_edn(&self) -> String {
        let mut out = String::new();
        print(&self.kind, &mut out);
        out
    }

    /// The string, when the value is one.
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            ValueKind::String(text) => Some(text),
            _ => None,
        }
    }

    /// The keyword's name, when the value is a keyword.
    pub fn as_keyword(&self) -> Option<&str> {
        match &self.kind {
            ValueKind::Keyword(name) => Some(name),
            _ => None,
        }
    }

    /// The value of keyword key `name`, when the value is a map.
    pub fn get(&self, name: &str) -> Option<&Value> {
        match &self.kind {
            ValueKind::Map(entries) => entries
                .iter()
                .find(|(key, _)| key.as_keyword() == Some(name))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Whether this is the nil of an unset variable with no default, which
    /// validation keeps for the build and mode that run to check.
    pub fn is_unset_environment(&self) -> bool {
        matches!(self.kind, ValueKind::Nil)
            && self
                .environment
                .as_ref()
                .is_some_and(|origin| !origin.set && !origin.default)
    }

    /// The one EDN form of `text`, read as the file is, written nowhere: a
    /// flag's EDN, such as a `--config` value's canonical text, before
    /// [`BuildSettings::set_entry_by_flag`] places it.
    pub fn parse(text: &str) -> Result<Value, String> {
        let lines = LineIndex::new(text);
        let mut value = read_document(text, &lines, &|_| None, ConfigSource::Default)
            .map_err(|error| error.message)?
            .ok_or_else(|| format!("{text:?} holds no form"))?;
        value.place(ConfigSource::Default);
        Ok(value)
    }

    /// Give this value and everything in it `source`, written nowhere in it.
    fn place(&mut self, source: ConfigSource) {
        self.source = source;
        self.span = 0..0;
        match &mut self.kind {
            ValueKind::List(items) | ValueKind::Vector(items) | ValueKind::Set(items) => {
                items.iter_mut().for_each(|item| item.place(source));
            }
            ValueKind::Map(entries) => {
                for (key, value) in entries {
                    key.place(source);
                    value.place(source);
                }
            }
            _ => {}
        }
    }

    /// Whether this value is `target`, or holds it: the same text and span.
    fn holds(&self, target: &Value) -> bool {
        if self.source == target.source && self.span == target.span {
            return true;
        }
        match &self.kind {
            ValueKind::List(items) | ValueKind::Vector(items) | ValueKind::Set(items) => {
                items.iter().any(|item| item.holds(target))
            }
            ValueKind::Map(entries) => entries
                .iter()
                .any(|(key, value)| key.holds(target) || value.holds(target)),
            _ => false,
        }
    }
}

impl From<ValueKind> for Value {
    /// A value written nowhere, such as a flag's before
    /// [`BuildSettings::set_by_flag`] places it.
    fn from(kind: ValueKind) -> Self {
        Self::default_of(kind)
    }
}

fn print(kind: &ValueKind, out: &mut String) {
    let sequence = |out: &mut String, open: &str, items: &[Value]| {
        out.push_str(open);
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            print(&item.kind, out);
        }
    };
    match kind {
        ValueKind::Nil => out.push_str("nil"),
        ValueKind::Boolean(value) => out.push_str(if *value { "true" } else { "false" }),
        ValueKind::Number { text, .. } => out.push_str(text),
        ValueKind::Symbolic(value) => out.push_str(match value {
            SymbolicValue::Infinity => "##Inf",
            SymbolicValue::NegativeInfinity => "##-Inf",
            SymbolicValue::NaN => "##NaN",
        }),
        ValueKind::String(text) => print_string(text, out),
        ValueKind::Utf16String(units) => {
            out.push('"');
            for unit in char::decode_utf16(units.iter().copied()) {
                match unit {
                    Ok(c) => print_char(c, out),
                    Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
                }
            }
            out.push('"');
        }
        ValueKind::Keyword(name) => {
            out.push(':');
            out.push_str(name);
        }
        ValueKind::Symbol(name) | ValueKind::Other(name) => out.push_str(name),
        ValueKind::List(items) => {
            sequence(out, "(", items);
            out.push(')');
        }
        ValueKind::Vector(items) => {
            sequence(out, "[", items);
            out.push(']');
        }
        ValueKind::Set(items) => {
            sequence(out, "#{", items);
            out.push('}');
        }
        ValueKind::Map(entries) => {
            out.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                print(&key.kind, out);
                out.push(' ');
                print(&value.kind, out);
            }
            out.push('}');
        }
    }
}

/// `text` as an EDN string the compiler's reader reads back as `text`.
fn print_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        print_char(c, out);
    }
    out.push('"');
}

fn print_char(c: char, out: &mut String) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\t' => out.push_str("\\t"),
        '\r' => out.push_str("\\r"),
        c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push(c),
    }
}

/// A setting as written: its key's span and its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: Range<usize>,
    pub value: Value,
}

/// One map's settings, a slot per [`Setting`].
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    slots: [Option<Entry>; Setting::COUNT],
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
        }
    }
}

impl Layer {
    pub fn get(&self, setting: Setting) -> Option<&Entry> {
        self.slots[setting.index()].as_ref()
    }

    fn set(&mut self, setting: Setting, key: Range<usize>, value: &Value) {
        self.slots[setting.index()] = Some(Entry {
            key,
            value: value.clone(),
        });
    }
}

/// One build of the file: its keyword id, and the settings of its own map
/// and of its `:dev` and `:release` maps, unmerged.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildDefinition {
    name: String,
    span: Range<usize>,
    /// The text of the id key at `span`: the file, or `LIGHT_CLJS` for a
    /// build only it defines.
    source: ConfigSource,
    layers: [Layer; 3],
}

impl BuildDefinition {
    /// The build id, without its colon.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The span of the build's id key, in [`BuildDefinition::source`]'s text.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// The text that first defines the build.
    pub fn source(&self) -> ConfigSource {
        self.source
    }

    /// The build's own settings with `None`, or its `:dev` or `:release`
    /// map's.
    pub fn layer(&self, mode: Option<BuildMode>) -> &Layer {
        &self.layers[mode.map_or(0, BuildMode::layer)]
    }

    /// `setting` as the build's own map, or its mode map, writes it.
    pub fn setting(&self, mode: Option<BuildMode>, setting: Setting) -> Option<&Value> {
        self.layer(mode).get(setting).map(|entry| &entry.value)
    }

    /// The module id and entry namespace a layer's `:modules` names.
    pub fn entry(&self, mode: Option<BuildMode>) -> Option<(&str, &str)> {
        let ValueKind::Map(modules) = &self.setting(mode, Setting::Modules)?.kind else {
            return None;
        };
        let (id, module) = modules.first()?;
        let ValueKind::Vector(entries) = &module.get("entries")?.kind else {
            return None;
        };
        match (&id.kind, &entries.first()?.kind) {
            (ValueKind::Keyword(id), ValueKind::Symbol(namespace)) => Some((id, namespace)),
            _ => None,
        }
    }
}

/// A read and validated configuration file.
#[derive(Clone, Debug)]
pub struct ConfigFile {
    path: PathBuf,
    source: String,
    lines: LineIndex,
    document: Value,
    /// `LIGHT_CLJS`'s text and lines, when it was set and held a form.
    light: Option<(String, LineIndex)>,
    /// The file's directory, absolute, for comparing directories.
    project: PathBuf,
    top: Layer,
    builds: Vec<BuildDefinition>,
}

impl ConfigFile {
    /// The first `light-cljs.edn` in `start` or one of its parents, up to the
    /// file system root, as shadow-cljs's launcher finds `shadow-cljs.edn`.
    /// No boundary stops the walk. A relative `start` is taken from the
    /// current directory.
    pub fn discover(start: &Path) -> Option<PathBuf> {
        let start = std::path::absolute(start).ok()?;
        start
            .ancestors()
            .map(|directory| directory.join(FILE_NAME))
            .find(|candidate| candidate.is_file())
    }

    /// Read, resolve and validate the file at `path`. `environment` looks a
    /// variable up for `#light-cljs/env`: the CLI passes `std::env::var`,
    /// tests a map.
    pub fn read(
        path: &Path,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigFileError> {
        let source = std::fs::read_to_string(path).map_err(|error| ConfigFileError {
            path: path.to_owned(),
            location: None,
            build: None,
            mode: None,
            message: format!("cannot read the configuration file: {error}"),
        })?;
        Self::from_source(path, source, environment)
    }

    /// As [`Self::read`], for `source` read from `path`, which only names it.
    ///
    /// When `environment` gives `LIGHT_CLJS` a value with a form in it, that
    /// EDN map is validated as strictly as the file and deep-merged over the
    /// whole file, as shadow-cljs merges `SHADOW_CLJS`; the file may then
    /// leave out what `LIGHT_CLJS` supplies, and the merged whole is checked.
    pub fn from_source(
        path: &Path,
        source: String,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigFileError> {
        let lines = LineIndex::new(&source);
        let light_text = environment(ENVIRONMENT_VARIABLE).filter(|text| !text.is_empty());
        let light_lines = light_text.as_deref().map(LineIndex::new);
        let texts = Texts {
            path,
            lines: &lines,
            light: light_lines.as_ref(),
            merges: &[],
        };
        let file = |error: Located| texts.error(error, ConfigSource::File);
        let document = read_document(&source, &lines, environment, ConfigSource::File)
            .map_err(file)?
            .ok_or_else(|| {
                file(at(
                    &(0..0),
                    "the configuration file is empty; it holds one map, such as \
                     {:source-paths [\"src\"] :builds {...}}"
                        .into(),
                ))
            })?;
        let light = match (&light_text, &light_lines) {
            (Some(text), Some(light_lines)) => {
                read_document(text, light_lines, environment, ConfigSource::LightCljs)
                    .map_err(|error| texts.error(error, ConfigSource::LightCljs))?
            }
            _ => None,
        };
        let validator = Validator {
            overlay: light.is_some(),
            origin: ConfigSource::File,
        };
        let mut merged = validator.document(&document).map_err(file)?;
        if let Some(light) = &light {
            let overlay = Validator {
                overlay: true,
                origin: ConfigSource::LightCljs,
            }
            .document(light)
            .map_err(|error| texts.error(error, ConfigSource::LightCljs))?;
            merged.overlay(overlay);
            for build in &merged.builds {
                required(build).map_err(file)?;
            }
            complete(&merged, &document.span).map_err(file)?;
        }
        // Each build's own map with each mode map over it, after LIGHT_CLJS:
        // one module of one entry, whichever build and mode a command runs.
        for build in &merged.builds {
            for mode in [BuildMode::Dev, BuildMode::Release] {
                let mut modules = None;
                for (layer, from) in [
                    (build.layer(None), None),
                    (build.layer(Some(mode)), Some(mode)),
                ] {
                    if let Some(entry) = layer.get(Setting::Modules) {
                        merge_slot(&mut modules, &entry.value, from);
                    }
                }
                check_modules(modules.as_ref().map(|slot| &slot.value), &build.name, mode)
                    .map_err(file)?;
            }
        }
        let light = match (light, light_text, light_lines) {
            (Some(_), Some(text), Some(lines)) => Some((text, lines)),
            _ => None,
        };
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let project = std::path::absolute(parent).unwrap_or_else(|_| parent.to_owned());
        Ok(Self {
            path: path.to_owned(),
            project,
            source,
            lines,
            document,
            light,
            top: merged.top,
            builds: merged.builds,
        })
    }

    /// `LIGHT_CLJS`'s text, when it was merged over the file.
    pub fn light_cljs(&self) -> Option<&str> {
        self.light.as_ref().map(|(text, _)| text.as_str())
    }

    /// The texts of the file, `LIGHT_CLJS` and `merges`, for locating values.
    fn texts<'a>(&'a self, merges: &'a [ConfigMerge]) -> Texts<'a> {
        Texts {
            path: &self.path,
            lines: &self.lines,
            light: self.light.as_ref().map(|(_, lines)| lines),
            merges,
        }
    }

    /// Where `value`, a value of the file, of `LIGHT_CLJS` or of one of
    /// `merges`, was written. A merged map's entries each keep their own.
    pub fn origin(&self, value: &Value, merges: &[ConfigMerge]) -> SettingOrigin {
        self.texts(merges).origin(value, None)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The whole file as read, with every environment value resolved: the
    /// layers hold the settings, and this keeps what they do not, such as
    /// a map-valued key given by an unset variable.
    pub fn document(&self) -> &Value {
        &self.document
    }

    /// The 1-based line and byte column of `offset` in the file.
    pub fn line_column(&self, offset: usize) -> (usize, usize) {
        self.lines.line_column(offset).unwrap_or((1, 1))
    }

    /// The top-level settings.
    pub fn top(&self) -> &Layer {
        &self.top
    }

    /// A top-level setting's value.
    pub fn setting(&self, setting: Setting) -> Option<&Value> {
        self.top.get(setting).map(|entry| &entry.value)
    }

    /// Every build, indexed by [`BuildId`].
    pub fn builds(&self) -> &[BuildDefinition] {
        &self.builds
    }

    pub fn build(&self, id: BuildId) -> &BuildDefinition {
        &self.builds[id.0 as usize]
    }

    /// The build id closest to `name` by edit distance, for "did you mean".
    pub fn suggest_build(&self, name: &str) -> Option<&str> {
        suggestion(name, self.builds.iter().map(|build| build.name.as_str()))
    }

    /// The build with keyword id `name` (without its colon).
    pub fn find_build(&self, name: &str) -> Option<BuildId> {
        self.builds
            .binary_search_by(|build| build.name.as_str().cmp(name))
            .ok()
            .map(|index| BuildId(index as u32))
    }
}

/// An error about the configuration file: `PATH:LINE:COLUMN: message`, with
/// the build and mode it concerns, where it concerns one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigFileError {
    pub path: PathBuf,
    /// The 1-based line and byte column; `None` when the file was not read.
    pub location: Option<(usize, usize)>,
    pub build: Option<String>,
    pub mode: Option<BuildMode>,
    pub message: String,
}

impl fmt::Display for ConfigFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.location {
            Some((line, column)) => write!(
                f,
                "{}:{line}:{column}: {}",
                self.path.display(),
                self.message
            ),
            None => write!(f, "{}: {}", self.path.display(), self.message),
        }
    }
}

impl std::error::Error for ConfigFileError {}

/// An error at a span, before its path and line are known.
struct Located {
    span: Range<usize>,
    message: String,
    build: Option<String>,
    mode: Option<BuildMode>,
    /// The text the span is in, when it is not the one being read.
    source: Option<ConfigSource>,
}

fn at(span: &Range<usize>, message: String) -> Located {
    Located {
        span: span.clone(),
        message,
        build: None,
        mode: None,
        source: None,
    }
}

/// An error at `value`, in whichever text it was written.
fn at_value(value: &Value, message: String) -> Located {
    Located {
        source: Some(value.source),
        ..at(&value.span, message)
    }
}

// ---------------------------------------------------------------------------
// Reading

/// Read the one form of `source`, written in `origin`, with every
/// environment value resolved: `None` when it holds no form.
fn read_document(
    source: &str,
    lines: &LineIndex,
    environment: &dyn Fn(&str) -> Option<String>,
    origin: ConfigSource,
) -> Result<Option<Value>, Located> {
    let mut parser = Parser::new(source).retaining_discards();
    let mut roots = Vec::new();
    loop {
        match parser.read_form() {
            Ok(Some(root)) => roots.push(root),
            Ok(None) => break,
            Err(error) => return Err(at(&error.span, error.to_string())),
        }
    }
    let mut discards: Vec<(Range<usize>, NodeId)> = parser
        .discarded()
        .iter()
        .map(|&id| match parser.nodes()[id.0].kind {
            NodeKind::Discard { form } => (parser.nodes()[id.0].span.clone(), form),
            _ => unreachable!("the discard table holds discards"),
        })
        .collect();
    discards.sort_by_key(|(span, _)| span.start);
    let reader = Reader {
        source,
        lines,
        nodes: parser.nodes(),
        discards,
        environment,
        origin,
    };
    match roots[..] {
        [] => Ok(None),
        [root] => reader.value(root).map(Some),
        [_, second, ..] => Err(at(
            &reader.nodes[second.0].span,
            format!("{} holds one map; this is a second form", noun(origin)),
        )),
    }
}

/// What a message calls the text of `origin`.
fn noun(origin: ConfigSource) -> &'static str {
    match origin {
        ConfigSource::LightCljs => ENVIRONMENT_VARIABLE,
        ConfigSource::ConfigMerge(_) => "--config-merge",
        ConfigSource::File | ConfigSource::Default => "the configuration file",
        ConfigSource::Flag(_) => "the command line",
    }
}

struct Reader<'a> {
    source: &'a str,
    lines: &'a LineIndex,
    nodes: &'a [Node],
    /// The discarded forms, by span start: each discard's span and form.
    discards: Vec<(Range<usize>, NodeId)>,
    environment: &'a dyn Fn(&str) -> Option<String>,
    /// The text being read, which every value records.
    origin: ConfigSource,
}

impl Reader<'_> {
    fn spelled(&self, id: NodeId) -> &str {
        &self.source[self.nodes[id.0].span.clone()]
    }

    fn position(&self, span: &Range<usize>) -> String {
        let (line, column) = self.lines.line_column(span.start).unwrap_or((1, 1));
        format!("{line}:{column}")
    }

    fn value(&self, id: NodeId) -> Result<Value, Located> {
        let node = &self.nodes[id.0];
        let span = node.span.clone();
        let spelled = self.spelled(id);
        let kind = match &node.kind {
            NodeKind::Nil => ValueKind::Nil,
            NodeKind::Boolean(value) => ValueKind::Boolean(*value),
            NodeKind::Symbol => ValueKind::Symbol(spelled.to_owned()),
            NodeKind::Keyword {
                auto_resolve: false,
            } => ValueKind::Keyword(spelled[1..].to_owned()),
            NodeKind::Keyword { auto_resolve: true } => {
                return Err(at(
                    &span,
                    format!(
                        "{spelled}: EDN has no auto-resolved keywords; write the namespace in full"
                    ),
                ));
            }
            NodeKind::Number(kind) => ValueKind::Number {
                kind: *kind,
                text: spelled.to_owned(),
            },
            NodeKind::SymbolicValue(value) => ValueKind::Symbolic(*value),
            NodeKind::String(value) => {
                let units = value.to_utf16(self.source);
                match String::from_utf16(&units) {
                    Ok(text) => ValueKind::String(text),
                    Err(_) => ValueKind::Utf16String(units),
                }
            }
            NodeKind::Regex | NodeKind::Character(_) => ValueKind::Other(spelled.to_owned()),
            NodeKind::Collection {
                kind: CollectionKind::AnonymousFn,
                ..
            } => {
                return Err(at(
                    &span,
                    "#(...) is code; the configuration file holds EDN data".into(),
                ));
            }
            NodeKind::Collection { kind, children } => {
                let mut values = Vec::with_capacity(children.len());
                for &child in children {
                    values.push(self.value(child)?);
                }
                match kind {
                    CollectionKind::List => ValueKind::List(values),
                    CollectionKind::Vector => ValueKind::Vector(values),
                    CollectionKind::Set => {
                        self.distinct(&values, "element")?;
                        ValueKind::Set(values)
                    }
                    _ => {
                        let mut entries = Vec::with_capacity(values.len() / 2);
                        let mut values = values.into_iter();
                        while let (Some(key), Some(value)) = (values.next(), values.next()) {
                            entries.push((key, value));
                        }
                        let keys: Vec<Value> = entries.iter().map(|(k, _)| k.clone()).collect();
                        self.distinct(&keys, "key")?;
                        self.hidden_duplicates(&span, children, &keys)?;
                        ValueKind::Map(entries)
                    }
                }
            }
            NodeKind::Tagged { tag, form } => {
                let tag = self.spelled(*tag);
                return match tag {
                    ENVIRONMENT_TAG => self.environment_value(*form, span),
                    "shadow/env" | "env" => Err(at(
                        &span,
                        format!("#{tag}: write #light-cljs/env, which takes the same forms"),
                    )),
                    _ => Err(at(
                        &span,
                        format!(
                            "#{tag}: the configuration file reads no tagged literal but \
                             #light-cljs/env"
                        ),
                    )),
                };
            }
            NodeKind::Conditional { splicing, .. } => {
                let prefix = if *splicing { "#?@" } else { "#?" };
                return Err(at(
                    &span,
                    format!(
                        "{prefix}: the configuration file is EDN, which has no reader conditionals"
                    ),
                ));
            }
            NodeKind::NamespacedMap { .. } => {
                return Err(at(
                    &span,
                    "the configuration file reads no namespaced map syntax; write each key in full"
                        .into(),
                ));
            }
            NodeKind::Metadata { .. } => {
                return Err(at(
                    &span,
                    "^: the configuration file is EDN, which has no metadata".into(),
                ));
            }
            NodeKind::Prefix { kind, .. } => {
                let name = match kind {
                    PrefixKind::Quote => "quote",
                    PrefixKind::SyntaxQuote => "syntax quote",
                    PrefixKind::Unquote => "unquote",
                    PrefixKind::UnquoteSplicing => "unquote-splicing",
                    PrefixKind::Deref => "deref",
                    PrefixKind::VarQuote => "var",
                };
                return Err(at(
                    &span,
                    format!("the configuration file is EDN, which has no {name} form"),
                ));
            }
            NodeKind::Discard { .. } | NodeKind::Argument { .. } => {
                ValueKind::Other(spelled.to_owned())
            }
        };
        Ok(Value {
            kind,
            span,
            environment: None,
            source: self.origin,
        })
    }

    /// An error at the second of two equal keys or elements.
    fn distinct(&self, values: &[Value], what: &str) -> Result<(), Located> {
        let mut identities: Vec<(String, usize)> = values
            .iter()
            .enumerate()
            .map(|(index, value)| (identity(value), index))
            .collect();
        identities.sort();
        let repeated = identities
            .windows(2)
            .filter(|pair| pair[0].0 == pair[1].0)
            .min_by_key(|pair| values[pair[1].1].span.start);
        match repeated {
            Some(pair) => {
                let (first, second) = (&values[pair[0].1], &values[pair[1].1]);
                Err(at(
                    &second.span,
                    format!(
                        "duplicate {what} {}, first given at {}",
                        second.to_edn(),
                        self.position(&first.span)
                    ),
                ))
            }
            None => Ok(()),
        }
    }

    /// An error at a `#_` directly in the map at `span` that discards a
    /// value equal to one of the map's keys: a reader that kept the discarded
    /// entry would see a duplicate key, and the edit that removed one of them
    /// is easy to misread.
    fn hidden_duplicates(
        &self,
        span: &Range<usize>,
        children: &[NodeId],
        keys: &[Value],
    ) -> Result<(), Located> {
        let first = self
            .discards
            .partition_point(|(d, _)| d.start <= span.start);
        let last = self.discards.partition_point(|(d, _)| d.start < span.end);
        let inside = &self.discards[first..last];
        let within = |outer: &Range<usize>, inner: &Range<usize>| {
            outer.start <= inner.start && inner.end <= outer.end
        };
        for (discard, form) in inside {
            let in_child = children
                .iter()
                .any(|child| within(&self.nodes[child.0].span, discard));
            let in_discarded = inside
                .iter()
                .any(|(_, other)| within(&self.nodes[other.0].span, discard));
            if in_child || in_discarded {
                continue;
            }
            // A discarded form the configuration file could not read
            // anyway hides no key.
            let Ok(discarded) = self.value(*form) else {
                continue;
            };
            let hidden = identity(&discarded);
            if let Some(key) = keys.iter().find(|key| identity(key) == hidden) {
                return Err(at(
                    discard,
                    format!(
                        "#_ discards a second {}, beside the key at {}; delete one of them",
                        discarded.to_edn(),
                        self.position(&key.span)
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Resolve `#light-cljs/env FORM`, the tag at `span`.
    fn environment_value(&self, form: NodeId, span: Range<usize>) -> Result<Value, Located> {
        let malformed = || {
            at(
                &span,
                "#light-cljs/env takes \"NAME\", [\"NAME\"], [\"NAME\" DEFAULT] or \
                 [\"NAME\" :as TYPE :default DEFAULT]"
                    .into(),
            )
        };
        let name_of = |id: NodeId| match self.value(id) {
            Ok(Value {
                kind: ValueKind::String(name),
                ..
            }) if !name.is_empty() => Ok(name),
            _ => Err(malformed()),
        };
        let mut as_type: Option<String> = None;
        let mut default: Option<Value> = None;
        let name = match &self.nodes[form.0].kind {
            NodeKind::String(_) => name_of(form)?,
            NodeKind::Collection {
                kind: CollectionKind::Vector,
                children,
            } => {
                let (&first, rest) = children.split_first().ok_or_else(malformed)?;
                let name = name_of(first)?;
                match rest {
                    [] => {}
                    [value] => default = Some(self.value(*value)?),
                    _ if rest.len() % 2 == 0 => {
                        for pair in rest.chunks(2) {
                            let option = match &self.nodes[pair[0].0].kind {
                                NodeKind::Keyword {
                                    auto_resolve: false,
                                } => self.spelled(pair[0]),
                                _ => return Err(malformed()),
                            };
                            match option {
                                ":as" if as_type.is_none() => {
                                    let value = self.value(pair[1])?;
                                    match value.as_keyword() {
                                        Some(name @ ("int" | "bool" | "symbol" | "keyword")) => {
                                            as_type = Some(name.to_owned())
                                        }
                                        _ => {
                                            return Err(at(
                                                &value.span,
                                                format!(
                                                    "#light-cljs/env :as takes :int, :bool, \
                                                     :symbol or :keyword, got {}",
                                                    value.to_edn()
                                                ),
                                            ));
                                        }
                                    }
                                }
                                ":default" if default.is_none() => {
                                    default = Some(self.value(pair[1])?)
                                }
                                _ => return Err(malformed()),
                            }
                        }
                    }
                    _ => return Err(malformed()),
                }
                name
            }
            _ => return Err(malformed()),
        };
        let found = (self.environment)(&name);
        let environment = Some(EnvironmentValue {
            variable: name.clone(),
            set: found.is_some(),
            default: default.is_some(),
        });
        let Some(text) = found else {
            let kind = default.map_or(ValueKind::Nil, |value| value.kind);
            return Ok(Value {
                kind,
                span,
                environment,
                source: self.origin,
            });
        };
        let refused = |what: &str, as_name: &str| {
            let mut quoted = String::new();
            print_string(&text, &mut quoted);
            at(
                &span,
                format!("{name} is {quoted}, not {what} (:as :{as_name})"),
            )
        };
        let kind = match as_type.as_deref() {
            None => ValueKind::String(text.clone()),
            Some("int") => {
                let digits = text.strip_prefix(['+', '-']).unwrap_or(&text);
                let number = (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| text.parse::<i64>().ok())
                    .flatten()
                    .ok_or_else(|| refused("a decimal integer", "int"))?;
                ValueKind::Number {
                    kind: NumberKind::Integer,
                    text: number.to_string(),
                }
            }
            Some("bool") => ValueKind::Boolean(text == "true"),
            Some("symbol") => match single_node(&text) {
                Some(NodeKind::Symbol) => ValueKind::Symbol(text.clone()),
                _ => return Err(refused("a symbol", "symbol")),
            },
            Some(_) => match single_node(&format!(":{text}")) {
                Some(NodeKind::Keyword {
                    auto_resolve: false,
                }) => ValueKind::Keyword(text.clone()),
                _ => return Err(refused("a keyword name", "keyword")),
            },
        };
        Ok(Value {
            kind,
            span,
            environment,
            source: self.origin,
        })
    }
}

/// The value's identity as the EDN reader compares values, as
/// `BuildConfiguration`'s canonical text does: decimal integers and floats by
/// their number (`1` and `1.0` are equal, and `0` and `-0.0`), lists and
/// vectors alike, and maps and sets in sorted order.
fn identity(value: &Value) -> String {
    let sorted = |mut items: Vec<String>| {
        items.sort();
        items.join(" ")
    };
    match &value.kind {
        ValueKind::Number {
            kind: NumberKind::Integer | NumberKind::Float,
            text,
        } => match text.parse::<f64>() {
            // Matches negative zero too: float patterns compare with `==`.
            Ok(0.0) => "0".into(),
            Ok(number) if number.is_finite() && number.fract() == 0.0 && number.abs() < 1e21 => {
                format!("{number:.0}")
            }
            Ok(number) if number.is_finite() => format!("{number}"),
            _ => text.clone(),
        },
        ValueKind::List(items) | ValueKind::Vector(items) => {
            let items: Vec<String> = items.iter().map(identity).collect();
            format!("[{}]", items.join(" "))
        }
        ValueKind::Set(items) => format!("#{{{}}}", sorted(items.iter().map(identity).collect())),
        ValueKind::Map(entries) => format!(
            "{{{}}}",
            sorted(
                entries
                    .iter()
                    .map(|(key, value)| format!("{} {}", identity(key), identity(value)))
                    .collect()
            )
        ),
        _ => value.to_edn(),
    }
}

/// The kind of the one form that spans all of `text`.
fn single_node(text: &str) -> Option<NodeKind> {
    let tree = crate::parser::parse(text).ok()?;
    match tree.roots[..] {
        [root] if tree.nodes[root.0].span == (0..text.len()) => {
            Some(tree.nodes[root.0].kind.clone())
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The schema

/// The maps of the schema whose key sets are closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MapKind {
    Top,
    Http,
    FsWatch,
    Nrepl,
    Build,
    CompilerOptions,
    BuildOptions,
    Devtools,
    Module,
}

/// What a key of a closed map is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Key {
    Setting(Setting),
    Map(MapKind),
    Builds,
    Mode(BuildMode),
    /// A key its map's own validation reads (`:nrepl :port`, a module's
    /// `:entries`).
    Field,
}

type Table = &'static [(&'static str, Key)];

// Each table is sorted by key, for binary search.
const TOP: Table = &[
    ("builds", Key::Builds),
    ("cache-blockers", Key::Setting(Setting::CacheBlockers)),
    ("cache-root", Key::Setting(Setting::CacheRoot)),
    ("compile-threads", Key::Setting(Setting::CompileThreads)),
    ("fs-watch", Key::Map(MapKind::FsWatch)),
    ("http", Key::Map(MapKind::Http)),
    ("nrepl", Key::Setting(Setting::Nrepl)),
    ("source-paths", Key::Setting(Setting::SourcePaths)),
];
const HTTP: Table = &[
    ("host", Key::Setting(Setting::HttpHost)),
    ("port", Key::Setting(Setting::HttpPort)),
    ("strict", Key::Setting(Setting::HttpStrict)),
];
const FS_WATCH: Table = &[
    ("impl", Key::Setting(Setting::FsWatchImpl)),
    ("loop-wait", Key::Setting(Setting::FsWatchLoopWait)),
];
const NREPL: Table = &[("port", Key::Field)];
const BUILD: Table = &[
    ("build-options", Key::Map(MapKind::BuildOptions)),
    ("closure-defines", Key::Setting(Setting::ClosureDefines)),
    ("compiler-options", Key::Map(MapKind::CompilerOptions)),
    ("config", Key::Setting(Setting::Config)),
    ("dev", Key::Mode(BuildMode::Dev)),
    ("devtools", Key::Map(MapKind::Devtools)),
    ("modules", Key::Setting(Setting::Modules)),
    ("output-dir", Key::Setting(Setting::OutputDir)),
    ("release", Key::Mode(BuildMode::Release)),
    ("target", Key::Setting(Setting::Target)),
];
const COMPILER_OPTIONS: Table = &[
    ("checked-arrays", Key::Setting(Setting::CheckedArrays)),
    ("cljs-warnings", Key::Setting(Setting::CljsWarnings)),
    (
        "closure-defines",
        Key::Setting(Setting::CompilerClosureDefines),
    ),
    ("elide-asserts", Key::Setting(Setting::ElideAsserts)),
    ("macro-expansions", Key::Setting(Setting::MacroExpansions)),
    (
        "macro-memory-limit",
        Key::Setting(Setting::MacroMemoryLimit),
    ),
    ("macro-nodes", Key::Setting(Setting::MacroNodes)),
    ("macro-poll-budget", Key::Setting(Setting::MacroPollBudget)),
    ("macro-timeout-ms", Key::Setting(Setting::MacroTimeoutMs)),
    ("optimizations", Key::Setting(Setting::Optimizations)),
    ("pseudo-names", Key::Setting(Setting::PseudoNames)),
];
const BUILD_OPTIONS: Table = &[
    ("cache-blockers", Key::Setting(Setting::BuildCacheBlockers)),
    ("cache-level", Key::Setting(Setting::CacheLevel)),
];
const DEVTOOLS: Table = &[("reload-strategy", Key::Setting(Setting::ReloadStrategy))];
const MODULE: Table = &[("entries", Key::Field)];

impl MapKind {
    fn table(self) -> Table {
        match self {
            Self::Top => TOP,
            Self::Http => HTTP,
            Self::FsWatch => FS_WATCH,
            Self::Nrepl => NREPL,
            Self::Build => BUILD,
            Self::CompilerOptions => COMPILER_OPTIONS,
            Self::BuildOptions => BUILD_OPTIONS,
            Self::Devtools => DEVTOOLS,
            Self::Module => MODULE,
        }
    }

    fn lookup(self, name: &str) -> Option<Key> {
        let table = self.table();
        table
            .binary_search_by(|(key, _)| (*key).cmp(name))
            .ok()
            .map(|index| table[index].1)
    }

    /// The key that holds this map, for messages: `:http`.
    fn key(self) -> &'static str {
        match self {
            Self::Top | Self::Build => "",
            Self::Http => "http",
            Self::FsWatch => "fs-watch",
            Self::Nrepl => "nrepl",
            Self::CompilerOptions => "compiler-options",
            Self::BuildOptions => "build-options",
            Self::Devtools => "devtools",
            Self::Module => "modules",
        }
    }
}

/// Shadow-cljs keys with no light-cljs equivalent, sorted, with the reason
/// and what to use instead ("Deviations from shadow-cljs").
const NO_EQUIVALENT: &[(&str, &str)] = &[
    (
        "after-load",
        "mark the function ^:dev/after-load instead, metadata shadow-cljs also reads",
    ),
    (
        "asset-path",
        "chunks import one another by relative URL, and the :http dev server serves each \
         build's :output-dir; delete it",
    ),
    (
        "before-load",
        "mark the function ^:dev/before-load instead, metadata shadow-cljs also reads",
    ),
    (
        "before-load-async",
        "mark the function ^:dev/before-load instead, metadata shadow-cljs also reads",
    ),
    (
        "build-hooks",
        "no user code runs outside macros, whose inputs the cache contract governs; reload \
         hooks are ^:dev/before-load and ^:dev/after-load metadata",
    ),
    (
        "dependencies",
        "light-cljs resolves no dependencies; put library sources in :source-paths, and let \
         the host or a bundler supply JavaScript packages",
    ),
    (
        "depends-on",
        "a build has one module; chunks are inferred from light.lazy/loadable sites \
         (docs/code-splitting.md), and named chunks arrive with its Phase 4",
    ),
    (
        "deps",
        "light-cljs reads no deps.edn and resolves no dependencies; put library sources in \
         :source-paths, and let the host or a bundler supply JavaScript packages",
    ),
    (
        "dev-http",
        "the :http dev server serves each build's :output-dir; use :http {:port N}",
    ),
    (
        "exports",
        "the startup chunk's default export is the entry's value; there are no named exports",
    ),
    (
        "externs",
        "light-cljs runs no Closure Compiler, so there are no externs",
    ),
    (
        "fn-invoke-direct",
        "light-cljs's optimizer chooses direct calls itself; use :optimizations",
    ),
    (
        "infer-externs",
        "light-cljs runs no Closure Compiler, so there are no externs to infer",
    ),
    (
        "init-fn",
        "the entry's value is the startup chunk's default export, and the entry namespace \
         runs when it loads; call the function from there",
    ),
    (
        "js-options",
        "light-cljs resolves no JavaScript packages; let the host or a bundler supply them",
    ),
    ("jvm-opts", "light-cljs runs no JVM"),
    (
        "output-feature-set",
        "light-cljs writes one kind of output, ES modules, so there is no feature set to choose",
    ),
    (
        "output-wrapper",
        "ES modules have their own scope, so there is no wrapper",
    ),
    (
        "prepend",
        "light-cljs writes the startup chunk as compiled; put other scripts in the page",
    ),
    ("source-map", "chunked output has no source maps yet"),
    (
        "warnings",
        "light-cljs has no per-warning switches; :cljs-warnings true makes undeclared Vars \
         and conflicting arities warnings",
    ),
    (
        "warnings-as-errors",
        "undeclared Vars and conflicting arities are errors unless :cljs-warnings is true, \
         and there are no other warnings to promote",
    ),
];

/// Where a map sits, for messages.
#[derive(Clone, Debug)]
struct Place {
    map: MapKind,
    build: Option<String>,
    mode: Option<BuildMode>,
    module: Option<String>,
    /// In a `--config-merge`, whose build is not known when it is read.
    merge: bool,
}

impl Place {
    fn top(map: MapKind) -> Self {
        Self {
            map,
            build: None,
            mode: None,
            module: None,
            merge: false,
        }
    }

    /// A `--config-merge`'s map of build keys.
    fn merge() -> Self {
        Self {
            merge: true,
            ..Self::top(MapKind::Build)
        }
    }

    fn within(&self, map: MapKind) -> Self {
        Self {
            map,
            ..self.clone()
        }
    }

    /// `build :app`, or `build :app (dev)` in a mode map; `the merged build`
    /// in a `--config-merge`.
    fn build_phrase(&self) -> String {
        if self.merge {
            return "the merged build".into();
        }
        let build = self.build.as_deref().unwrap_or_default();
        match self.mode {
            Some(mode) => format!("build :{build} ({})", mode.name()),
            None => format!("build :{build}"),
        }
    }

    /// Where a key of this map is: `in build :app`.
    fn in_phrase(&self) -> String {
        match self.map {
            MapKind::Top => "at the top level".into(),
            MapKind::Http | MapKind::FsWatch | MapKind::Nrepl => {
                format!("in :{}", self.map.key())
            }
            MapKind::Build => format!("in {}", self.build_phrase()),
            MapKind::Module => format!(
                "in module :{} of {}",
                self.module.as_deref().unwrap_or_default(),
                self.build_phrase()
            ),
            _ => format!("in :{} of {}", self.map.key(), self.build_phrase()),
        }
    }

    /// Whose a value is: ` of build :app`, nothing at the top level.
    fn owner_phrase(&self) -> String {
        match self.map {
            MapKind::Top => String::new(),
            MapKind::Http | MapKind::FsWatch | MapKind::Nrepl => {
                format!(" of :{}", self.map.key())
            }
            MapKind::Module => format!(
                " of module :{} of {}",
                self.module.as_deref().unwrap_or_default(),
                self.build_phrase()
            ),
            _ => format!(" of {}", self.build_phrase()),
        }
    }

    fn error(&self, span: &Range<usize>, message: String) -> Located {
        Located {
            span: span.clone(),
            message,
            build: self.build.clone(),
            mode: self.mode,
            source: None,
        }
    }

    fn bad_value(&self, key: &str, value: &Value, expected: &str) -> Located {
        self.error(
            &value.span,
            format!(
                ":{key}{} must be {expected}, got {}",
                self.owner_phrase(),
                value.to_edn()
            ),
        )
    }
}

/// The message for key `name`, which `place`'s map does not have.
fn unknown_key(name: &str, place: &Place) -> String {
    let in_phrase = place.in_phrase();
    if let Ok(index) = NO_EQUIVALENT.binary_search_by(|(key, _)| (*key).cmp(name)) {
        let reason = NO_EQUIVALENT[index].1;
        return match place.map {
            MapKind::Top => format!(":{name}: {reason}"),
            _ => format!(":{name} {in_phrase}: {reason}"),
        };
    }
    let in_build = place.build.is_some() || place.merge;
    let order: &[MapKind] = if in_build {
        &[
            MapKind::Build,
            MapKind::CompilerOptions,
            MapKind::BuildOptions,
            MapKind::Devtools,
            MapKind::Module,
            MapKind::Top,
            MapKind::Http,
            MapKind::FsWatch,
        ]
    } else {
        &[
            MapKind::Top,
            MapKind::Http,
            MapKind::FsWatch,
            MapKind::Build,
            MapKind::CompilerOptions,
            MapKind::BuildOptions,
            MapKind::Devtools,
            MapKind::Module,
        ]
    };
    if let Some(home) = order
        .iter()
        .find(|&&kind| kind != place.map && kind.lookup(name).is_some())
    {
        let belongs = match home {
            MapKind::Top => "is a top-level key".to_owned(),
            MapKind::Build => "is a build key".to_owned(),
            MapKind::Http | MapKind::FsWatch | MapKind::Nrepl => {
                format!("belongs under :{}", home.key())
            }
            MapKind::Module => "belongs in a module under :modules".to_owned(),
            _ if in_build => format!("belongs under :{}", home.key()),
            _ => format!("belongs under a build's :{}", home.key()),
        };
        return format!(":{name} {in_phrase} {belongs}, as in shadow-cljs");
    }
    let candidates = place.map.table().iter().map(|(key, _)| *key);
    match suggestion(name, candidates.clone()) {
        Some(close) => format!("unknown key :{name} {in_phrase}; did you mean :{close}?"),
        None => {
            let keys: Vec<String> = candidates.map(|key| format!(":{key}")).collect();
            format!(
                "unknown key :{name} {in_phrase}; the keys here are {}",
                keys.join(", ")
            )
        }
    }
}

/// The candidate closest to `name` by edit distance, after spelling both in
/// kebab case (`outputDir` and `output_dir` as `output-dir`), when no more
/// than half of the longer one differs.
fn suggestion<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let kebab = |text: &str| {
        let mut out = String::with_capacity(text.len() + 4);
        for (index, c) in text.chars().enumerate() {
            match c {
                '_' => out.push('-'),
                c if c.is_ascii_uppercase() => {
                    if index > 0 && !out.ends_with('-') {
                        out.push('-');
                    }
                    out.push(c.to_ascii_lowercase());
                }
                c => out.push(c),
            }
        }
        out
    };
    let name = kebab(name);
    candidates
        .map(|candidate| (edit_distance(&name, candidate), candidate))
        .filter(|&(distance, candidate)| distance * 2 <= name.len().max(candidate.len()))
        .min_by_key(|&(distance, _)| distance)
        .map(|(_, candidate)| candidate)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, &x) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, &y) in b.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(x != y))
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// A build id: a letter, then letters, digits and hyphens.
fn build_id(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A count in decimal digits, as the flags read one.
fn digits(value: &Value) -> Option<&str> {
    match &value.kind {
        ValueKind::Number {
            kind: NumberKind::Integer,
            text,
        } if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => Some(text),
        _ => None,
    }
}

fn positive_integer(value: &Value) -> Option<u64> {
    digits(value)?.parse().ok().filter(|&n| n > 0)
}

fn port(value: &Value) -> Option<u16> {
    digits(value)?.parse().ok()
}

fn nonempty_string(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.is_empty())
}

fn namespace_symbol(value: &Value) -> bool {
    matches!(&value.kind, ValueKind::Symbol(name) if !name.contains('/'))
}

fn count_word(count: usize) -> String {
    match count {
        2 => "two".into(),
        3 => "three".into(),
        n => n.to_string(),
    }
}

const CODE_SPLITTING: &str = "chunks are inferred from light.lazy/loadable sites \
                              (docs/code-splitting.md), and named chunks arrive with its Phase 4";

/// A checked key of a closed map: its span, its name, what it is, and its
/// value.
type KeyedValue<'v> = (&'v Range<usize>, &'v str, Key, &'v Value);

/// Validates one text. An overlay (`LIGHT_CLJS`, or the file under it) may
/// leave out what the merged whole must hold: `:source-paths`, `:builds` and
/// a build's required keys, which [`complete`] checks after merging.
struct Validator {
    overlay: bool,
    origin: ConfigSource,
}

/// The settings a document holds, before the checks of [`complete`].
struct Document {
    top: Layer,
    builds: Vec<BuildDefinition>,
    /// The `:builds` value's span, when the document has one.
    builds_span: Option<Range<usize>>,
}

impl Validator {
    fn document(&self, document: &Value) -> Result<Document, Located> {
        let place = Place::top(MapKind::Top);
        let ValueKind::Map(_) = &document.kind else {
            let message = match self.origin {
                ConfigSource::LightCljs => format!(
                    "{ENVIRONMENT_VARIABLE} holds one map of the configuration file's shape, \
                     such as {{:builds {{:app {{:output-dir \"out\"}}}}}}, got {}",
                    document.to_edn()
                ),
                _ => format!(
                    "the configuration file holds one map of :source-paths and :builds, got {}",
                    document.to_edn()
                ),
            };
            return Err(at(&document.span, message));
        };
        let mut top = Layer::default();
        let mut builds = Vec::new();
        let mut builds_span = None;
        for (key, name, kind, value) in self.entries(document, &place)? {
            match kind {
                Key::Builds => {
                    builds_span = Some(value.span.clone());
                    builds = self.builds(value)?;
                }
                _ => self.key(kind, key, name, value, &place, &mut top)?,
            }
        }
        let document_value = Document {
            top,
            builds,
            builds_span,
        };
        if !self.overlay {
            complete(&document_value, &document.span)?;
        }
        Ok(document_value)
    }

    /// A `--config-merge`'s map of build keys, into one layer.
    fn merge(&self, value: &Value) -> Result<Layer, Located> {
        let place = Place::merge();
        let ValueKind::Map(_) = &value.kind else {
            return Err(at(
                &value.span,
                format!(
                    "--config-merge must be a map of build keys, such as \
                     {{:output-dir \"out/x\"}}, got {}",
                    value.to_edn()
                ),
            ));
        };
        let mut layer = Layer::default();
        for (key, name, kind, value) in self.entries(value, &place)? {
            if let Key::Mode(_) = kind {
                return Err(at(
                    key,
                    format!(
                        ":{name} in --config-merge: the command chooses the mode (compile and \
                         watch run :dev, release runs :release), so --config-merge holds the \
                         build keys themselves"
                    ),
                ));
            }
            self.key(kind, key, name, value, &place, &mut layer)?;
        }
        Ok(layer)
    }

    /// `map`'s entries, each key checked against `place`'s closed key set:
    /// its span, its name, what it is, and its value.
    fn entries<'v>(&self, map: &'v Value, place: &Place) -> Result<Vec<KeyedValue<'v>>, Located> {
        let ValueKind::Map(entries) = &map.kind else {
            unreachable!("callers check for a map")
        };
        let mut out = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let Some(name) = key.as_keyword() else {
                return Err(place.error(
                    &key.span,
                    format!(
                        "the key {} {} must be a keyword",
                        key.to_edn(),
                        place.in_phrase()
                    ),
                ));
            };
            let Some(kind) = place.map.lookup(name) else {
                return Err(place.error(&key.span, unknown_key(name, place)));
            };
            out.push((&key.span, name, kind, value));
        }
        Ok(out)
    }

    /// Check one key of a closed map and store what it sets in `layer`.
    fn key(
        &self,
        kind: Key,
        key: &Range<usize>,
        name: &str,
        value: &Value,
        place: &Place,
        layer: &mut Layer,
    ) -> Result<(), Located> {
        match kind {
            Key::Setting(setting) => {
                self.setting(setting, key, value, place)?;
                layer.set(setting, key.clone(), value);
            }
            Key::Map(map) => {
                if value.is_unset_environment() {
                    return Ok(());
                }
                if !matches!(value.kind, ValueKind::Map(_)) {
                    return Err(place.bad_value(name, value, "a map"));
                }
                let inner = place.within(map);
                for (key, name, kind, value) in self.entries(value, &inner)? {
                    self.key(kind, key, name, value, &inner, layer)?;
                }
            }
            Key::Mode(_) | Key::Builds | Key::Field => {
                unreachable!("handled by the map that holds the key")
            }
        }
        Ok(())
    }

    fn builds(&self, value: &Value) -> Result<Vec<BuildDefinition>, Located> {
        let place = Place::top(MapKind::Top);
        let ValueKind::Map(entries) = &value.kind else {
            return Err(place.bad_value(
                "builds",
                value,
                "a map of build id to build configuration",
            ));
        };
        if entries.is_empty() && !self.overlay {
            return Err(no_builds(&value.span));
        }
        let mut builds = Vec::with_capacity(entries.len());
        for (key, build) in entries {
            let name = match key.as_keyword() {
                Some(name) if build_id(name) => name,
                _ => {
                    return Err(at(
                        &key.span,
                        format!(
                            "build id {} must be a keyword of a letter followed by letters, \
                             digits and hyphens, such as :app",
                            key.to_edn()
                        ),
                    ));
                }
            };
            builds.push(self.build(name, &key.span, build)?);
        }
        builds.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(builds)
    }

    fn build(
        &self,
        name: &str,
        span: &Range<usize>,
        value: &Value,
    ) -> Result<BuildDefinition, Located> {
        let place = Place {
            build: Some(name.to_owned()),
            ..Place::top(MapKind::Build)
        };
        if !matches!(value.kind, ValueKind::Map(_)) {
            return Err(place.error(
                &value.span,
                format!(
                    "build :{name} must be a map of build keys, got {}",
                    value.to_edn()
                ),
            ));
        }
        let mut layers: [Layer; 3] = Default::default();
        for (key, key_name, kind, value) in self.entries(value, &place)? {
            let Key::Mode(mode) = kind else {
                self.key(kind, key, key_name, value, &place, &mut layers[0])?;
                continue;
            };
            let mode_place = Place {
                mode: Some(mode),
                ..place.clone()
            };
            if !matches!(value.kind, ValueKind::Map(_)) {
                return Err(mode_place.error(
                    &value.span,
                    format!(
                        ":{} of build :{name} must be a map of build keys, got {}",
                        mode.name(),
                        value.to_edn()
                    ),
                ));
            }
            for (key, key_name, kind, value) in self.entries(value, &mode_place)? {
                if let Key::Mode(_) = kind {
                    return Err(mode_place.error(
                        key,
                        format!(
                            ":{key_name} {}: a mode map holds build keys; :dev and :release \
                             sit in the build itself",
                            mode_place.in_phrase()
                        ),
                    ));
                }
                self.key(
                    kind,
                    key,
                    key_name,
                    value,
                    &mode_place,
                    &mut layers[mode.layer()],
                )?;
            }
        }
        let build = BuildDefinition {
            name: name.to_owned(),
            span: span.clone(),
            source: self.origin,
            layers,
        };
        if !self.overlay {
            required(&build)?;
        }
        Ok(build)
    }

    /// Check `setting`'s value.
    fn setting(
        &self,
        setting: Setting,
        key: &Range<usize>,
        value: &Value,
        place: &Place,
    ) -> Result<(), Located> {
        // An unset variable's nil is checked where it is required, for the
        // build and mode that run.
        if value.is_unset_environment() {
            return Ok(());
        }
        let name = setting.key();
        let expect = |valid: bool, expected: &str| {
            if valid {
                Ok(())
            } else {
                Err(place.bad_value(name, value, expected))
            }
        };
        let keyword_in = |names: &[&str]| value.as_keyword().is_some_and(|k| names.contains(&k));
        match setting {
            Setting::SourcePaths => {
                let ValueKind::Vector(paths) = &value.kind else {
                    return expect(false, "a vector of directory strings");
                };
                if paths.is_empty() {
                    return expect(false, "a vector of directory strings");
                }
                if let Some(path) = paths
                    .iter()
                    .find(|path| !path.is_unset_environment() && !nonempty_string(path))
                {
                    return Err(place.error(
                        &path.span,
                        format!(
                            "each of :source-paths must be a directory string, got {}",
                            path.to_edn()
                        ),
                    ));
                }
                Ok(())
            }
            Setting::CacheRoot | Setting::OutputDir => {
                expect(nonempty_string(value), "a directory string")
            }
            Setting::CacheBlockers | Setting::BuildCacheBlockers => {
                let ValueKind::Set(blockers) = &value.kind else {
                    return expect(false, "a set of namespace symbols");
                };
                if let Some(blocker) = blockers
                    .iter()
                    .find(|blocker| !blocker.is_unset_environment() && !namespace_symbol(blocker))
                {
                    return Err(place.error(
                        &blocker.span,
                        format!(
                            "each of :cache-blockers{} must be a namespace symbol, got {}",
                            place.owner_phrase(),
                            blocker.to_edn()
                        ),
                    ));
                }
                Ok(())
            }
            Setting::CompileThreads
            | Setting::FsWatchLoopWait
            | Setting::MacroExpansions
            | Setting::MacroNodes
            | Setting::MacroTimeoutMs
            | Setting::MacroMemoryLimit
            | Setting::MacroPollBudget => {
                expect(positive_integer(value).is_some(), "a positive integer")
            }
            Setting::HttpPort => expect(port(value).is_some(), "a port number, 0 to 65535"),
            Setting::HttpHost => expect(nonempty_string(value), "a host name string"),
            Setting::HttpStrict
            | Setting::PseudoNames
            | Setting::ElideAsserts
            | Setting::CljsWarnings => {
                expect(matches!(value.kind, ValueKind::Boolean(_)), "true or false")
            }
            Setting::Nrepl => self.nrepl(value, place),
            Setting::FsWatchImpl => expect(
                keyword_in(&["polling"]),
                ":polling, the one watcher light-cljs has",
            ),
            Setting::Target => {
                if keyword_in(&["esm"]) {
                    return Ok(());
                }
                Err(place.error(
                    &value.span,
                    format!(
                        ":target{} must be :esm: light-cljs writes ES modules, which run in \
                         browsers, Node and bundlers",
                        place.owner_phrase()
                    ),
                ))
            }
            Setting::Modules => self.modules(value, place),
            Setting::Config => self.config(value, place),
            Setting::ClosureDefines | Setting::CompilerClosureDefines => {
                self.closure_defines(value, place)?;
                self.release_only(name, "takes no defines", key, place)
            }
            Setting::CheckedArrays => {
                expect(keyword_in(&["warn", "error"]), ":warn or :error")?;
                self.release_only(name, "checks no array accesses", key, place)
            }
            Setting::Optimizations => expect(
                keyword_in(&["none", "prepare", "advanced"]),
                ":none, :prepare or :advanced",
            ),
            Setting::CacheLevel => {
                if value.as_keyword() == Some("jars") {
                    return Err(place.error(
                        &value.span,
                        format!(
                            ":cache-level{} must be :all or :off; :jars caches only the \
                             namespaces of jars, and light-cljs reads no jars",
                            place.owner_phrase()
                        ),
                    ));
                }
                expect(keyword_in(&["all", "off"]), ":all or :off")
            }
            Setting::ReloadStrategy => expect(keyword_in(&["full"]), ":full"),
        }
    }

    /// The static mode rule: `key` may not reach dev mode, whose dev build
    /// `reason`. The build's own map and its `:dev` map reach dev mode.
    fn release_only(
        &self,
        name: &str,
        reason: &str,
        key: &Range<usize>,
        place: &Place,
    ) -> Result<(), Located> {
        if place.build.is_none() || place.mode == Some(BuildMode::Release) {
            return Ok(());
        }
        Err(Located {
            mode: Some(BuildMode::Dev),
            ..place.error(
                key,
                format!(
                    ":{name} of {} applies in dev mode, whose dev build {reason}; write it \
                     under :release",
                    place.build_phrase()
                ),
            )
        })
    }

    fn nrepl(&self, value: &Value, place: &Place) -> Result<(), Located> {
        match &value.kind {
            ValueKind::Boolean(false) => Ok(()),
            ValueKind::Map(_) => {
                let inner = place.within(MapKind::Nrepl);
                for (_, name, _, port_value) in self.entries(value, &inner)? {
                    if !port_value.is_unset_environment() && port(port_value).is_none() {
                        return Err(inner.bad_value(name, port_value, "a port number, 0 to 65535"));
                    }
                }
                Ok(())
            }
            _ => Err(place.bad_value("nrepl", value, "false or a map such as {:port 0}")),
        }
    }

    fn modules(&self, value: &Value, place: &Place) -> Result<(), Located> {
        let expected = "a map of one module, such as {:main {:entries [app.main]}}";
        let ValueKind::Map(modules) = &value.kind else {
            return Err(place.bad_value("modules", value, expected));
        };
        let build = place.build_phrase();
        match &modules[..] {
            [] => return Err(place.bad_value("modules", value, expected)),
            [_] => {}
            [_, (second, _), ..] => {
                return Err(place.error(
                    &second.span,
                    format!(
                        "{build} has {} modules; {CODE_SPLITTING}",
                        count_word(modules.len())
                    ),
                ));
            }
        }
        let (id, module) = &modules[0];
        let Some(id_name) = id.as_keyword().filter(|name| build_id(name)) else {
            return Err(place.error(
                &id.span,
                format!(
                    "the module id {} of {build} must be a keyword, such as :main",
                    id.to_edn()
                ),
            ));
        };
        let inner = Place {
            module: Some(id_name.to_owned()),
            ..place.within(MapKind::Module)
        };
        if module.is_unset_environment() {
            return Ok(());
        }
        if !matches!(module.kind, ValueKind::Map(_)) {
            return Err(place.error(
                &module.span,
                format!(
                    "module :{id_name} of {build} must be a map such as {{:entries [app.main]}}, \
                     got {}",
                    module.to_edn()
                ),
            ));
        }
        let mut entries_value = None;
        for (_, _, _, value) in self.entries(module, &inner)? {
            entries_value = Some(value);
        }
        let Some(entries) = entries_value else {
            return Err(place.error(
                &module.span,
                format!(
                    "module :{id_name} of {build} has no :entries; give its namespace, such as \
                     {{:entries [app.main]}}"
                ),
            ));
        };
        let expected = "a vector of one namespace symbol, such as [app.main]";
        if entries.is_unset_environment() {
            return Ok(());
        }
        let ValueKind::Vector(namespaces) = &entries.kind else {
            return Err(inner.bad_value("entries", entries, expected));
        };
        match &namespaces[..] {
            [] => Err(inner.bad_value("entries", entries, expected)),
            [namespace] if namespace.is_unset_environment() || namespace_symbol(namespace) => {
                Ok(())
            }
            [namespace] => Err(inner.bad_value("entries", namespace, "a namespace symbol")),
            [_, second, ..] => Err(place.error(
                &second.span,
                format!(
                    "module :{id_name} of {build} has {} entries; give one namespace: \
                     {CODE_SPLITTING}",
                    count_word(namespaces.len())
                ),
            )),
        }
    }

    /// `:config` follows `--config`: each value's EDN text is handed to
    /// [`BuildConfiguration::insert`], whose messages these are.
    fn config(&self, value: &Value, place: &Place) -> Result<(), Located> {
        let ValueKind::Map(entries) = &value.kind else {
            return Err(place.bad_value("config", value, "a map of keyword to EDN value"));
        };
        let mut configuration = BuildConfiguration::new();
        for (key, entry) in entries {
            let Some(name) = key.as_keyword() else {
                return Err(place.error(
                    &key.span,
                    format!(
                        "each key of :config{} must be a keyword, such as :app/flavor, got {}",
                        place.owner_phrase(),
                        key.to_edn()
                    ),
                ));
            };
            configuration
                .insert(name, &entry.to_edn())
                .map_err(|message| {
                    place.error(
                        &entry.span,
                        format!(":config :{name}{}: {message}", place.owner_phrase()),
                    )
                })?;
        }
        Ok(())
    }

    fn closure_defines(&self, value: &Value, place: &Place) -> Result<(), Located> {
        let ValueKind::Map(entries) = &value.kind else {
            return Err(place.bad_value(
                "closure-defines",
                value,
                "a map of qualified symbol to boolean, number or string",
            ));
        };
        for (key, define) in entries {
            let qualified = matches!(&key.kind, ValueKind::Symbol(name)
                if name.split_once('/').is_some_and(|(ns, local)| !ns.is_empty() && !local.is_empty()));
            if !qualified {
                return Err(place.error(
                    &key.span,
                    format!(
                        "each key of :closure-defines{} must be a qualified symbol such as \
                         app.config/DEBUG, got {}",
                        place.owner_phrase(),
                        key.to_edn()
                    ),
                ));
            }
            let valid = define.is_unset_environment()
                || matches!(
                    define.kind,
                    ValueKind::Boolean(_)
                        | ValueKind::String(_)
                        | ValueKind::Symbolic(_)
                        | ValueKind::Number {
                            kind: NumberKind::Integer | NumberKind::Float,
                            ..
                        }
                );
            if !valid {
                return Err(place.error(
                    &define.span,
                    format!(
                        ":closure-defines {}{} must be a boolean, number or string, got {}",
                        key.to_edn(),
                        place.owner_phrase(),
                        define.to_edn()
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn no_builds(span: &Range<usize>) -> Located {
    at(
        span,
        ":builds must hold at least one build, such as {:app {:target :esm \
         :output-dir \"out\" :modules {:main {:entries [app.main]}}}}"
            .into(),
    )
}

/// What a whole configuration must hold, which an overlay may leave to the
/// text under or over it: `:source-paths` and a build. `span` is the file's
/// map.
fn complete(document: &Document, span: &Range<usize>) -> Result<(), Located> {
    if document.top.get(Setting::SourcePaths).is_none() {
        return Err(at(
            span,
            "the configuration file has no :source-paths, the directories its builds \
             read namespaces from; add one, such as :source-paths [\"src\"]"
                .into(),
        ));
    }
    if document.builds.is_empty() {
        return Err(match &document.builds_span {
            Some(builds) => no_builds(builds),
            None => at(
                span,
                "the configuration file has no :builds; add one, such as :builds {:app \
                 {:target :esm :output-dir \"out\" :modules {:main {:entries [app.main]}}}}"
                    .into(),
            ),
        });
    }
    Ok(())
}

/// The keys every build needs in each mode, in the build or in its mode map.
fn required(build: &BuildDefinition) -> Result<(), Located> {
    let name = &build.name;
    let error = |message: String| Located {
        build: Some(name.clone()),
        source: Some(build.source),
        ..at(&build.span, message)
    };
    for setting in [Setting::Target, Setting::OutputDir, Setting::Modules] {
        let missing: Vec<BuildMode> = [BuildMode::Dev, BuildMode::Release]
            .into_iter()
            .filter(|mode| {
                build.layers[0].get(setting).is_none()
                    && build.layers[mode.layer()].get(setting).is_none()
            })
            .collect();
        let key = setting.key();
        match missing[..] {
            [] => {}
            [mode] => {
                return Err(error(format!(
                    "build :{name} has no :{key} in {} mode, which every build needs; \
                     give it in the build or under :{}",
                    mode.name(),
                    mode.name()
                )));
            }
            _ => {
                return Err(error(format!(
                    "build :{name} has no :{key}, which every build needs"
                )));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Merging

/// Shadow-cljs's `deep-merge` (`shadow/build/api.clj`), `upper` over
/// `lower`: a nil `lower` gives `upper`, and a nil `upper` keeps `lower`;
/// maps merge by key, each key's values merged again; vectors are
/// concatenated keeping each distinct element once, in order; sets are
/// unioned; anything else is replaced by `upper`. Keys and elements are
/// equal as [`BuildConfiguration`] finds them (`1` and `1.0`, a list and a
/// vector). The result is `upper`'s span and source, and each entry and
/// element keeps its own.
pub fn deep_merge(lower: Value, upper: Value) -> Value {
    if matches!(lower.kind, ValueKind::Nil) {
        return upper;
    }
    if matches!(upper.kind, ValueKind::Nil) {
        return lower;
    }
    let Value {
        kind,
        span,
        environment,
        source,
    } = upper;
    let kind = match (lower.kind, kind) {
        (ValueKind::Map(mut entries), ValueKind::Map(upper_entries)) => {
            let mut index: HashMap<String, usize> = entries
                .iter()
                .enumerate()
                .map(|(position, (key, _))| (identity(key), position))
                .collect();
            for (key, value) in upper_entries {
                let key_identity = identity(&key);
                match index.get(&key_identity) {
                    Some(&position) => {
                        let slot = &mut entries[position].1;
                        let below = std::mem::replace(slot, Value::default_of(ValueKind::Nil));
                        *slot = deep_merge(below, value);
                    }
                    None => {
                        index.insert(key_identity, entries.len());
                        entries.push((key, value));
                    }
                }
            }
            ValueKind::Map(entries)
        }
        (ValueKind::Vector(items), ValueKind::Vector(upper_items)) => {
            ValueKind::Vector(distinct(items, upper_items))
        }
        (ValueKind::Set(items), ValueKind::Set(upper_items)) => {
            ValueKind::Set(distinct(items, upper_items))
        }
        (_, kind) => kind,
    };
    Value {
        kind,
        span,
        environment,
        source,
    }
}

/// `lower` then `upper`, each element once, at its first place.
fn distinct(lower: Vec<Value>, upper: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    lower
        .into_iter()
        .chain(upper)
        .filter(|item| seen.insert(identity(item)))
        .collect()
}

impl Layer {
    /// Deep-merge `upper`'s settings over these, slot by slot: a setting is
    /// one key path, so this is `deep_merge` of the maps that hold them.
    fn overlay(&mut self, upper: &Layer) {
        for (slot, entry) in self.slots.iter_mut().zip(&upper.slots) {
            let Some(entry) = entry else { continue };
            *slot = Some(match slot.take() {
                Some(lower)
                    if matches!(entry.value.kind, ValueKind::Nil)
                        && !matches!(lower.value.kind, ValueKind::Nil) =>
                {
                    lower
                }
                Some(lower) => Entry {
                    key: entry.key.clone(),
                    value: deep_merge(lower.value, entry.value.clone()),
                },
                None => entry.clone(),
            });
        }
    }
}

impl Document {
    /// `LIGHT_CLJS` over the file: the top level, and each build by id, its
    /// own map and its mode maps; a build only `upper` has is added.
    fn overlay(&mut self, upper: Document) {
        self.top.overlay(&upper.top);
        for build in upper.builds {
            match self
                .builds
                .binary_search_by(|lower| lower.name.as_str().cmp(&build.name))
            {
                Ok(position) => {
                    let lower = &mut self.builds[position];
                    for (lower, upper) in lower.layers.iter_mut().zip(&build.layers) {
                        lower.overlay(upper);
                    }
                }
                Err(position) => self.builds.insert(position, build),
            }
        }
    }
}

impl BuildDefinition {
    /// The output directory the file gives the build in `mode`.
    fn output_dir(&self, mode: BuildMode) -> Option<&str> {
        self.output_dir_value(mode)?.as_str()
    }

    /// The value of the output directory the file gives the build in `mode`:
    /// its mode map's, else its own.
    pub fn output_dir_value(&self, mode: BuildMode) -> Option<&Value> {
        match self.setting(Some(mode), Setting::OutputDir) {
            Some(value) if !matches!(value.kind, ValueKind::Nil) => Some(value),
            _ => self.setting(None, Setting::OutputDir),
        }
    }
}

/// The texts values are written in, to locate them.
struct Texts<'a> {
    path: &'a Path,
    lines: &'a LineIndex,
    light: Option<&'a LineIndex>,
    merges: &'a [ConfigMerge],
}

impl Texts<'_> {
    fn lines(&self, source: ConfigSource) -> Option<&LineIndex> {
        match source {
            ConfigSource::File => Some(self.lines),
            ConfigSource::LightCljs => self.light,
            ConfigSource::ConfigMerge(index) => self
                .merges
                .iter()
                .find(|merge| merge.index == index)
                .map(|merge| &merge.lines),
            ConfigSource::Default | ConfigSource::Flag(_) => None,
        }
    }

    fn name(&self, source: ConfigSource) -> PathBuf {
        match source {
            ConfigSource::LightCljs => PathBuf::from(ENVIRONMENT_VARIABLE),
            ConfigSource::ConfigMerge(index) => merge_name(index),
            ConfigSource::File | ConfigSource::Default | ConfigSource::Flag(_) => {
                self.path.to_owned()
            }
        }
    }

    /// `error`, in its own text or else in `default`'s.
    fn error(&self, error: Located, default: ConfigSource) -> ConfigFileError {
        let source = error.source.unwrap_or(default);
        let location = self
            .lines(source)
            .and_then(|lines| lines.line_column(error.span.start))
            .unwrap_or((1, 1));
        ConfigFileError {
            path: self.name(source),
            location: Some(location),
            build: error.build,
            mode: error.mode,
            message: error.message,
        }
    }

    fn origin(&self, value: &Value, mode: Option<BuildMode>) -> SettingOrigin {
        let location = match value.source {
            ConfigSource::Default | ConfigSource::Flag(_) => None,
            source => Some(
                self.lines(source)
                    .and_then(|lines| lines.line_column(value.span.start))
                    .unwrap_or((1, 1)),
            ),
        };
        SettingOrigin {
            source: value.source,
            span: value.span.clone(),
            location,
            mode,
            environment: value.environment.clone(),
            flag: None,
        }
    }
}

fn merge_name(index: usize) -> PathBuf {
    PathBuf::from(format!("--config-merge {index}"))
}

/// `located`, about build `build` in `mode`.
fn scoped(located: Located, build: &str, mode: BuildMode) -> Located {
    Located {
        build: Some(build.to_owned()),
        mode: Some(mode),
        ..located
    }
}

/// Where a merged setting's value was written.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingOrigin {
    pub source: ConfigSource,
    /// The value's bytes in its source's text; empty for a default.
    pub span: Range<usize>,
    /// The 1-based line and byte column of the span; `None` for a default.
    pub location: Option<(usize, usize)>,
    /// The mode map that gave the value, when one did.
    pub mode: Option<BuildMode>,
    /// The variable, for a `#light-cljs/env` value.
    pub environment: Option<EnvironmentValue>,
    /// For a flag's value, the flag as typed and what it replaced:
    /// `--optimize none, over the release default :advanced`.
    pub flag: Option<String>,
}

impl fmt::Display for SettingOrigin {
    /// `light-cljs.edn:14:49 (:release), from APP_VERSION`,
    /// `LIGHT_CLJS:1:14`, `--config-merge 2:1:30`, `default`, or a flag's
    /// text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(flag) = &self.flag {
            return f.write_str(flag);
        }
        let Some((line, column)) = self.location else {
            return f.write_str("default");
        };
        match self.source {
            ConfigSource::Default | ConfigSource::Flag(_) => return f.write_str("default"),
            ConfigSource::File => write!(f, "{FILE_NAME}:{line}:{column}")?,
            ConfigSource::LightCljs => write!(f, "{ENVIRONMENT_VARIABLE}:{line}:{column}")?,
            ConfigSource::ConfigMerge(index) => {
                write!(f, "--config-merge {index}:{line}:{column}")?
            }
        }
        if let Some(mode) = self.mode {
            write!(f, " (:{})", mode.name())?;
        }
        match &self.environment {
            Some(environment) if environment.set => write!(f, ", from {}", environment.variable),
            Some(environment) => write!(f, ", {} unset", environment.variable),
            None => Ok(()),
        }
    }
}

/// A merged setting: its value and where it was written.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingValue {
    pub value: Value,
    pub origin: SettingOrigin,
}

/// One build's settings for one mode, layers 1-5 of "Precedence" merged: a
/// slot per [`Setting`], empty where no layer gives the setting and it has
/// no default. `:closure-defines` under `:compiler-options` is merged under
/// the build's own, into [`Setting::ClosureDefines`], leaving
/// [`Setting::CompilerClosureDefines`] empty; the top-level
/// `:cache-blockers` are unioned into [`Setting::BuildCacheBlockers`].
///
/// The command's flags, layer 6, write over them through
/// [`BuildSettings::set_by_flag`] and its siblings, for `--print-config`.
#[derive(Clone, Debug)]
pub struct BuildSettings {
    build: BuildId,
    name: String,
    mode: BuildMode,
    slots: [Option<SettingValue>; Setting::COUNT],
    /// The origin text of each [`ConfigSource::Flag`], by its index.
    flags: Vec<String>,
}

impl BuildSettings {
    pub fn build(&self) -> BuildId {
        self.build
    }

    /// The build id, without its colon.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mode(&self) -> BuildMode {
        self.mode
    }

    pub fn slot(&self, setting: Setting) -> Option<&SettingValue> {
        self.slots[setting.index()].as_ref()
    }

    pub fn get(&self, setting: Setting) -> Option<&Value> {
        self.slot(setting).map(|slot| &slot.value)
    }

    pub fn origin(&self, setting: Setting) -> Option<&SettingOrigin> {
        self.slot(setting).map(|slot| &slot.origin)
    }

    /// Layer 6: `flag`, as typed (`--optimize none`), gives `setting` the
    /// value `kind`, replacing what layers 1-5 gave it. Its origin prints as
    /// the flag and what it replaced: `--optimize none, over the release
    /// default :advanced`, `--out-dir out/x, over "out/app" at
    /// light-cljs.edn:7:21`.
    pub fn set_by_flag(&mut self, setting: Setting, kind: ValueKind, flag: &str) {
        let text = match self.slot(setting) {
            Some(lower) => format!("{flag}, {}", self.over(setting, lower)),
            None => flag.to_owned(),
        };
        let source = self.flag_source(text);
        let mut value = Value::from(kind);
        value.place(source);
        self.slots[setting.index()] = Some(SettingValue {
            origin: self.flag_origin_of(source),
            value,
        });
    }

    /// Layer 6 for one entry of a map setting, `:config` or
    /// `:closure-defines`: `flag` (`--config app/flavor=:dev`) gives key
    /// `key` the value `value`, replacing an equal key's entry, whose origin
    /// `file` and `merges` locate (`--define app.x/Y=1, over 2 at
    /// light-cljs.edn:9:30`), or adding one at the end.
    pub fn set_entry_by_flag(
        &mut self,
        file: &ConfigFile,
        merges: &[ConfigMerge],
        setting: Setting,
        key: Value,
        value: Value,
        flag: &str,
    ) {
        let below = self.get(setting).and_then(|map| match &map.kind {
            ValueKind::Map(entries) => entries
                .iter()
                .position(|(other, _)| identity(other) == identity(&key))
                .map(|position| (position, &entries[position].1)),
            _ => None,
        });
        let text = match below {
            Some((_, lower)) => format!(
                "{flag}, over {} at {}",
                lower.to_edn(),
                file.value_origin(self, lower, merges)
            ),
            None => flag.to_owned(),
        };
        let position = below.map(|(position, _)| position);
        let source = self.flag_source(text);
        let (mut key, mut value) = (key, value);
        key.place(source);
        value.place(source);
        let origin = self.flag_origin_of(source);
        match &mut self.slots[setting.index()] {
            Some(SettingValue {
                value:
                    Value {
                        kind: ValueKind::Map(entries),
                        ..
                    },
                ..
            }) => match position {
                // Where the entry it replaces was, so the map keeps its order.
                Some(position) => entries[position] = (key, value),
                None => entries.push((key, value)),
            },
            slot => {
                let mut map = Value::from(ValueKind::Map(vec![(key, value)]));
                map.source = source;
                *slot = Some(SettingValue { origin, value: map });
            }
        }
    }

    /// Layer 6 for a set setting, the build's `:cache-blockers`, which
    /// accumulate: `flag` (`--cache-blocker`), once per element, adds each
    /// of `elements` the set does not hold. Its origin then prints as the
    /// flags that added one and what they added to: `--cache-blocker app.b,
    /// added to #{app.a} at light-cljs.edn:2:18`.
    pub fn add_by_flag(&mut self, setting: Setting, flag: &str, elements: Vec<Value>) {
        let held: Vec<Value> = match self.get(setting).map(|set| &set.kind) {
            Some(ValueKind::Set(items)) => items.clone(),
            _ => Vec::new(),
        };
        let mut added: Vec<Value> = Vec::new();
        for element in elements {
            if !held
                .iter()
                .chain(&added)
                .any(|other| identity(other) == identity(&element))
            {
                added.push(element);
            }
        }
        if added.is_empty() {
            return;
        }
        let flag = added
            .iter()
            .map(|element| format!("{flag} {}", element.to_edn()))
            .collect::<Vec<_>>()
            .join(" ");
        let text = match self.slot(setting) {
            Some(lower) if lower.origin.source != ConfigSource::Default || !held.is_empty() => {
                format!(
                    "{flag}, added to {} at {}",
                    lower.value.to_edn(),
                    lower.origin
                )
            }
            _ => flag,
        };
        let source = self.flag_source(text);
        for element in &mut added {
            element.place(source);
        }
        let mut set = Value::from(ValueKind::Set(held.into_iter().chain(added).collect()));
        set.source = source;
        self.slots[setting.index()] = Some(SettingValue {
            origin: self.flag_origin_of(source),
            value: set,
        });
    }

    /// What a flag replacing `lower`, `setting`'s value, writes over: `over
    /// the default 1`, `over the release default :advanced` for a default
    /// that depends on the mode, `over "out/app" at light-cljs.edn:7:21`.
    fn over(&self, setting: Setting, lower: &SettingValue) -> String {
        if lower.origin.source == ConfigSource::Default {
            let mode = match setting {
                Setting::Optimizations | Setting::CljsWarnings => {
                    format!("{} ", self.mode.name())
                }
                _ => String::new(),
            };
            format!("over the {mode}default {}", lower.value.to_edn())
        } else {
            format!("over {} at {}", lower.value.to_edn(), lower.origin)
        }
    }

    /// A new flag source, whose origin prints as `text`.
    fn flag_source(&mut self, text: String) -> ConfigSource {
        self.flags.push(text);
        ConfigSource::Flag(self.flags.len() - 1)
    }

    fn flag_origin_of(&self, source: ConfigSource) -> SettingOrigin {
        let flag = match source {
            ConfigSource::Flag(index) => self.flags.get(index).cloned(),
            _ => None,
        };
        SettingOrigin {
            source,
            span: 0..0,
            location: None,
            mode: None,
            environment: None,
            flag,
        }
    }
}

/// A setting as merged so far, and the mode map that last gave it.
#[derive(Clone, Debug)]
struct Slot {
    value: Value,
    mode: Option<BuildMode>,
}

type Slots = [Option<Slot>; Setting::COUNT];

/// Deep-merge `value`, from a layer that is `mode`'s map or not, over `slot`.
fn merge_slot(slot: &mut Option<Slot>, value: &Value, mode: Option<BuildMode>) {
    *slot = Some(match slot.take() {
        Some(lower)
            if matches!(value.kind, ValueKind::Nil)
                && !matches!(lower.value.kind, ValueKind::Nil) =>
        {
            lower
        }
        Some(lower) => Slot {
            value: deep_merge(lower.value, value.clone()),
            mode,
        },
        None => Slot {
            value: value.clone(),
            mode,
        },
    });
}

fn merge_layer(slots: &mut Slots, layer: &Layer, mode: Option<BuildMode>) {
    for (slot, entry) in slots.iter_mut().zip(&layer.slots) {
        if let Some(entry) = entry {
            merge_slot(slot, &entry.value, mode);
        }
    }
}

/// `upper` deep-merged over `lower`, where a default `upper` adds nothing.
fn combine(lower: Option<Slot>, upper: Option<Slot>) -> Option<Slot> {
    match (lower, upper) {
        (lower, None) => lower,
        (None, upper) => upper,
        (Some(lower), Some(upper)) if upper.value.source == ConfigSource::Default => Some(lower),
        (lower, Some(upper)) => {
            let mut slot = lower;
            merge_slot(&mut slot, &upper.value, upper.mode);
            slot
        }
    }
}

/// Layer 1, the built-in defaults: today's flag defaults, with release's
/// `:optimizations :advanced` and the watch's `:http`, `:fs-watch` and cache
/// root ("Schema"). Dev builds run at none semantics and with ClojureScript's
/// warnings, so dev's `:optimizations` is `:none` and `:cljs-warnings` true.
fn defaults(mode: BuildMode) -> Slots {
    let limits = crate::MacroLimits::default();
    let evaluator = crate::evaluator::EvaluatorOptions::default();
    let cache_root = Path::new(crate::compile_cache::DEFAULT_DIR)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or(".light-cljs");
    let integer = |number: u128| ValueKind::Number {
        kind: NumberKind::Integer,
        text: number.to_string(),
    };
    let keyword = |name: &str| ValueKind::Keyword(name.to_owned());
    let release = mode == BuildMode::Release;
    let values = [
        (Setting::CacheRoot, ValueKind::String(cache_root.to_owned())),
        (Setting::CacheBlockers, ValueKind::Set(Vec::new())),
        (Setting::CompileThreads, integer(1)),
        (Setting::HttpPort, integer(9630)),
        (Setting::HttpHost, ValueKind::String("127.0.0.1".into())),
        (Setting::FsWatchLoopWait, integer(100)),
        (
            Setting::Optimizations,
            keyword(if release { "advanced" } else { "none" }),
        ),
        (Setting::PseudoNames, ValueKind::Boolean(false)),
        (Setting::ElideAsserts, ValueKind::Boolean(false)),
        (Setting::CljsWarnings, ValueKind::Boolean(!release)),
        (Setting::MacroExpansions, integer(limits.expansions as u128)),
        (Setting::MacroNodes, integer(limits.generated_nodes as u128)),
        (
            Setting::MacroTimeoutMs,
            integer(evaluator.timeout.as_millis()),
        ),
        (
            Setting::MacroMemoryLimit,
            integer(evaluator.memory_limit as u128),
        ),
        (
            Setting::MacroPollBudget,
            integer(u128::from(evaluator.poll_budget)),
        ),
        (Setting::CacheLevel, keyword("all")),
        (Setting::BuildCacheBlockers, ValueKind::Set(Vec::new())),
    ];
    let mut slots: Slots = std::array::from_fn(|_| None);
    for (setting, kind) in values {
        slots[setting.index()] = Some(Slot {
            value: Value::default_of(kind),
            mode: None,
        });
    }
    slots
}

/// Whether `mode` reads `setting`: the dev server, nREPL, the poller and
/// reloading belong to `watch`, a dev mode command.
fn applies(setting: Setting, mode: BuildMode) -> bool {
    match setting {
        Setting::HttpPort
        | Setting::HttpHost
        | Setting::HttpStrict
        | Setting::Nrepl
        | Setting::FsWatchImpl
        | Setting::FsWatchLoopWait
        | Setting::ReloadStrategy => mode == BuildMode::Dev,
        _ => true,
    }
}

/// `path` under `base`, with `.` and `..` resolved by name.
fn lexical(base: &Path, path: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for component in base.join(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

impl ConfigFile {
    /// Build `build`'s settings for `mode`: the built-in defaults, the file's
    /// top level (with `LIGHT_CLJS` merged over the whole file as it was
    /// read), the build, its `:dev` or `:release` map, then each of `merges`
    /// in order, deep-merged ([`deep_merge`]) slot by slot, each slot with
    /// its origin.
    ///
    /// The result is validated again: a merge that adds a second module or
    /// `:entries` namespace, or `:closure-defines` or `:checked-arrays` in
    /// dev mode; a nil from an unset variable without a default where a
    /// value is required, in the settings `mode` reads; and an output
    /// directory that another build writes in `mode` (as the file defines
    /// that build, without `merges`), that is the cache store or that is a
    /// source path.
    pub fn settings(
        &self,
        build: BuildId,
        mode: BuildMode,
        merges: &[ConfigMerge],
    ) -> Result<BuildSettings, ConfigFileError> {
        self.merged_settings(build, mode, merges, true)
    }

    /// As [`Self::settings`], without comparing the build's output directory
    /// with the other builds', the cache store and the source paths: for a
    /// command whose flags replace one of them, which compares the
    /// directories the build will use once the flags are applied.
    pub fn settings_before_flags(
        &self,
        build: BuildId,
        mode: BuildMode,
        merges: &[ConfigMerge],
    ) -> Result<BuildSettings, ConfigFileError> {
        self.merged_settings(build, mode, merges, false)
    }

    fn merged_settings(
        &self,
        build: BuildId,
        mode: BuildMode,
        merges: &[ConfigMerge],
        outputs: bool,
    ) -> Result<BuildSettings, ConfigFileError> {
        let texts = self.texts(merges);
        let fail = |error: Located| texts.error(error, ConfigSource::File);
        let definition = self.build(build);
        let name = definition.name();
        let mut slots = defaults(mode);
        merge_layer(&mut slots, &self.top, None);
        merge_layer(&mut slots, definition.layer(None), None);
        merge_layer(&mut slots, definition.layer(Some(mode)), Some(mode));
        for merge in merges {
            if mode == BuildMode::Dev {
                merge.dev_rule(name).map_err(fail)?;
            }
            merge_layer(&mut slots, &merge.layer, None);
        }
        let compiler = slots[Setting::CompilerClosureDefines.index()].take();
        let own = slots[Setting::ClosureDefines.index()].take();
        slots[Setting::ClosureDefines.index()] = combine(compiler, own);
        let top_blockers = slots[Setting::CacheBlockers.index()].clone();
        let own = slots[Setting::BuildCacheBlockers.index()].take();
        slots[Setting::BuildCacheBlockers.index()] = combine(top_blockers, own);

        check_modules(
            slots[Setting::Modules.index()]
                .as_ref()
                .map(|slot| &slot.value),
            name,
            mode,
        )
        .map_err(fail)?;
        check_unset(&mut slots, name, mode).map_err(fail)?;
        if outputs {
            self.check_output(&slots, build, mode).map_err(fail)?;
        }

        let slots = slots.map(|slot| {
            slot.map(|Slot { value, mode }| SettingValue {
                origin: texts.origin(&value, mode),
                value,
            })
        });
        Ok(BuildSettings {
            build,
            name: name.to_owned(),
            mode,
            slots,
            flags: Vec::new(),
        })
    }

    /// Several builds run by one command, each with `merges` applied, must
    /// not write one directory: `settings` checks each against the others
    /// as the file defines them, and this checks them against each other.
    pub fn check_outputs(
        &self,
        settings: &[BuildSettings],
        merges: &[ConfigMerge],
    ) -> Result<(), ConfigFileError> {
        let project = self.project_dir();
        let mut seen: HashMap<PathBuf, &BuildSettings> = HashMap::new();
        for build in settings {
            let Some(value) = build.get(Setting::OutputDir) else {
                continue;
            };
            let Some(text) = value.as_str() else { continue };
            if let Some(other) = seen.insert(lexical(project, text), build) {
                let (a, b) = ordered(other.name(), build.name());
                return Err(self.texts(merges).error(
                    scoped(
                        at_value(
                            value,
                            format!(
                                "builds :{a} and :{b} both write {text} in {} mode",
                                build.mode().name()
                            ),
                        ),
                        build.name(),
                        build.mode(),
                    ),
                    ConfigSource::File,
                ));
            }
        }
        Ok(())
    }

    /// The project directory, which relative paths resolve against: the
    /// file's, absolute.
    pub fn project_dir(&self) -> &Path {
        &self.project
    }

    /// `path` as the commands use it: joined to the project directory, with
    /// `.` and `..` resolved by name, so a command run from a subdirectory
    /// spells a source path, and so its cache keys, as one run from the
    /// project directory does ("Cache keys and fingerprints").
    pub fn resolve(&self, path: &Path) -> PathBuf {
        match path.to_str() {
            Some(text) => lexical(&self.project, text),
            None => self.project.join(path),
        }
    }

    fn check_output(&self, slots: &Slots, build: BuildId, mode: BuildMode) -> Result<(), Located> {
        let Some(out) = slots[Setting::OutputDir.index()].as_ref() else {
            return Ok(());
        };
        let Some(text) = out.value.as_str() else {
            return Ok(());
        };
        let project = self.project_dir();
        let target = lexical(project, text);
        let name = self.build(build).name();
        let mode_name = mode.name();
        let error = |message: String| scoped(at_value(&out.value, message), name, mode);
        for (index, other) in self.builds.iter().enumerate() {
            if index == build.0 as usize {
                continue;
            }
            if other
                .output_dir(mode)
                .is_some_and(|other_text| lexical(project, other_text) == target)
            {
                let (a, b) = ordered(name, other.name());
                return Err(error(format!(
                    "builds :{a} and :{b} both write {text} in {mode_name} mode"
                )));
            }
        }
        let root = slots[Setting::CacheRoot.index()].as_ref();
        if let Some(root) = root.and_then(|root| root.value.as_str())
            && lexical(project, root).join("cache") == target
        {
            let mut quoted = String::new();
            print_string(root, &mut quoted);
            return Err(error(format!(
                "build :{name} writes {text} in {mode_name} mode, the cache store of \
                 :cache-root {quoted}; give it a directory of its own"
            )));
        }
        if let Some(Slot {
            value:
                Value {
                    kind: ValueKind::Vector(paths),
                    ..
                },
            ..
        }) = &slots[Setting::SourcePaths.index()]
        {
            for path in paths {
                if path
                    .as_str()
                    .is_some_and(|path| lexical(project, path) == target)
                {
                    return Err(error(format!(
                        "build :{name} writes {text} in {mode_name} mode, the source path {}; \
                         give it a directory of its own",
                        path.to_edn()
                    )));
                }
            }
        }
        Ok(())
    }
}

fn ordered<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Merged `:modules` with a second module or a second `:entries` namespace.
fn check_modules(modules: Option<&Value>, build: &str, mode: BuildMode) -> Result<(), Located> {
    let Some(Value {
        kind: ValueKind::Map(modules),
        ..
    }) = modules
    else {
        return Ok(());
    };
    let phrase = format!("build :{build} ({})", mode.name());
    if let [_, (second, _), ..] = &modules[..] {
        return Err(scoped(
            at_value(
                second,
                format!(
                    "{phrase} has {} modules; {CODE_SPLITTING}",
                    count_word(modules.len())
                ),
            ),
            build,
            mode,
        ));
    }
    let Some((id, module)) = modules.first() else {
        return Ok(());
    };
    // An unset variable's nil is no namespace: check_unset reports it.
    let entries: Vec<&Value> = match module.get("entries").map(|entries| &entries.kind) {
        Some(ValueKind::Vector(entries)) => entries
            .iter()
            .filter(|entry| !entry.is_unset_environment())
            .collect(),
        _ => Vec::new(),
    };
    if let [_, second, ..] = &entries[..] {
        return Err(scoped(
            at_value(
                second,
                format!(
                    "module {} of {phrase} has {} entries; give one namespace: {CODE_SPLITTING}",
                    id.to_edn(),
                    count_word(entries.len())
                ),
            ),
            build,
            mode,
        ));
    }
    Ok(())
}

/// The nils of unset variables without a default ("Errors"), checked for
/// one build and mode, in the settings the mode reads: an error where a
/// value is required, and otherwise the setting is not given.
fn check_unset(slots: &mut Slots, build: &str, mode: BuildMode) -> Result<(), Located> {
    let unset = |value: &Value, subject: String| {
        let variable = value
            .environment
            .as_ref()
            .map_or("", |environment| environment.variable.as_str());
        scoped(
            at_value(
                value,
                format!(
                    "{subject} of build :{build} ({}) is nil: {variable} is unset and the form \
                     gives no default",
                    mode.name()
                ),
            ),
            build,
            mode,
        )
    };
    fn first_unset(items: &[Value]) -> Option<&Value> {
        items.iter().find(|item| item.is_unset_environment())
    }
    for setting in Setting::ALL {
        let Some(slot) = &slots[setting.index()] else {
            continue;
        };
        if !applies(setting, mode) {
            continue;
        }
        let value = &slot.value;
        if value.is_unset_environment() {
            if matches!(
                setting,
                Setting::SourcePaths | Setting::Target | Setting::OutputDir | Setting::Modules
            ) {
                return Err(unset(value, format!(":{}", setting.key())));
            }
            continue;
        }
        match (setting, &value.kind) {
            (Setting::SourcePaths, ValueKind::Vector(paths)) => {
                if let Some(path) = first_unset(paths) {
                    return Err(unset(path, "a source path".into()));
                }
            }
            (Setting::CacheBlockers | Setting::BuildCacheBlockers, ValueKind::Set(blockers)) => {
                if let Some(blocker) = first_unset(blockers) {
                    return Err(unset(blocker, "a cache blocker".into()));
                }
            }
            (Setting::Modules, ValueKind::Map(modules)) => {
                for (id, module) in modules {
                    let id = id.to_edn();
                    if module.is_unset_environment() {
                        return Err(unset(module, format!("module {id}")));
                    }
                    let Some(entries) = module.get("entries") else {
                        continue;
                    };
                    if entries.is_unset_environment() {
                        return Err(unset(entries, format!(":entries of module {id}")));
                    }
                    if let ValueKind::Vector(namespaces) = &entries.kind
                        && let Some(namespace) = first_unset(namespaces)
                    {
                        return Err(unset(namespace, format!("the entry of module {id}")));
                    }
                }
            }
            (Setting::ClosureDefines, ValueKind::Map(defines)) => {
                for (key, define) in defines {
                    if define.is_unset_environment() {
                        return Err(unset(define, format!(":closure-defines {}", key.to_edn())));
                    }
                }
            }
            (Setting::Nrepl, ValueKind::Map(_)) => {
                if let Some(port) = value.get("port")
                    && port.is_unset_environment()
                {
                    return Err(unset(port, ":nrepl :port".into()));
                }
            }
            _ => {}
        }
    }
    for slot in slots.iter_mut() {
        if slot
            .as_ref()
            .is_some_and(|slot| slot.value.is_unset_environment())
        {
            *slot = None;
        }
    }
    Ok(())
}

/// One `--config-merge DATA`, read and validated: a map of build keys
/// deep-merged over each build the command names, after its mode map.
#[derive(Clone, Debug)]
pub struct ConfigMerge {
    index: usize,
    text: String,
    lines: LineIndex,
    layer: Layer,
}

impl ConfigMerge {
    /// Read the `index`th `--config-merge` (from 1), as shadow-cljs's
    /// `parse-merge-data`: DATA starting with `{` is EDN text, and anything
    /// else a file path relative to `project_dir`; `classpath:` is an
    /// error, since light-cljs has no classpath. The text is read with the
    /// file's reader, its `#light-cljs/env` resolved against `environment`,
    /// and validated as strictly as a build's map, with `--config-merge N`
    /// as its path in messages. `:dev` and `:release` are errors in it: the
    /// command has chosen the mode.
    pub fn parse(
        index: usize,
        data: &str,
        project_dir: &Path,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigFileError> {
        let unlocated = |message: String| ConfigFileError {
            path: merge_name(index),
            location: None,
            build: None,
            mode: None,
            message,
        };
        if data.starts_with("classpath:") {
            return Err(unlocated(format!(
                "{data}: light-cljs has no classpath; give EDN text starting with {{ or a \
                 file path relative to the project directory"
            )));
        }
        let text = if data.starts_with('{') {
            data.to_owned()
        } else {
            let path = project_dir.join(data);
            std::fs::read_to_string(&path)
                .map_err(|error| unlocated(format!("cannot read {}: {error}", path.display())))?
        };
        let lines = LineIndex::new(&text);
        let located = |error: Located| ConfigFileError {
            path: merge_name(index),
            location: Some(lines.line_column(error.span.start).unwrap_or((1, 1))),
            build: error.build,
            mode: error.mode,
            message: error.message,
        };
        let origin = ConfigSource::ConfigMerge(index);
        let Some(value) = read_document(&text, &lines, environment, origin).map_err(located)?
        else {
            return Err(located(at(
                &(0..0),
                "--config-merge must be a map of build keys, such as {:output-dir \"out/x\"}, \
                 got nothing"
                    .into(),
            )));
        };
        let layer = Validator {
            overlay: true,
            origin,
        }
        .merge(&value)
        .map_err(located)?;
        Ok(Self {
            index,
            text,
            lines,
            layer,
        })
    }

    /// Which `--config-merge` this is, from 1.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The EDN text, as given or as read from the file.
    pub fn source(&self) -> &str {
        &self.text
    }

    /// The settings it merges.
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// The mode rule for a merge, which the command applies in dev mode:
    /// `:closure-defines` and `:checked-arrays` reach no dev build.
    fn dev_rule(&self, build: &str) -> Result<(), Located> {
        let refused = [
            (Setting::ClosureDefines, "takes no defines"),
            (Setting::CompilerClosureDefines, "takes no defines"),
            (Setting::CheckedArrays, "checks no array accesses"),
        ];
        let first = refused
            .iter()
            .filter_map(|&(setting, reason)| {
                let entry = self.layer.get(setting)?;
                (!entry.value.is_unset_environment()).then_some((setting, reason, entry))
            })
            .min_by_key(|(_, _, entry)| entry.key.start);
        match first {
            None => Ok(()),
            Some((setting, reason, entry)) => Err(scoped(
                Located {
                    source: Some(entry.value.source),
                    ..at(
                        &entry.key,
                        format!(
                            ":{} of build :{build} applies in dev mode, whose dev build \
                             {reason}; merge it into release builds only",
                            setting.key()
                        ),
                    )
                },
                build,
                BuildMode::Dev,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_tables_are_sorted_for_binary_search() {
        let tables = [
            TOP,
            HTTP,
            FS_WATCH,
            NREPL,
            BUILD,
            COMPILER_OPTIONS,
            BUILD_OPTIONS,
            DEVTOOLS,
            MODULE,
        ];
        for table in tables {
            assert!(
                table.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "{table:?}"
            );
        }
        assert!(NO_EQUIVALENT.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (index, setting) in Setting::ALL.iter().enumerate() {
            assert_eq!(setting.index(), index);
        }
    }

    #[test]
    fn suggestions_spell_keys_in_kebab_case() {
        let keys = BUILD.iter().map(|(key, _)| *key);
        assert_eq!(suggestion("output-path", keys.clone()), Some("output-dir"));
        assert_eq!(suggestion("output_dir", keys.clone()), Some("output-dir"));
        assert_eq!(suggestion("outputDir", keys.clone()), Some("output-dir"));
        assert_eq!(suggestion("zzz", keys), None);
    }
}
