//! `--print-config`: a build's merged settings, flags included, printed as
//! EDN a `light-cljs.edn` could hold, with each value's origin in a comment
//! (docs/build-configuration.md, "Commands", the `--print-config` bullet).
//!
//! The settings print in the order of the schema's tables ("Top-level
//! keys", "Build keys", "Compiler options"), with `:builds` last, each on a
//! line of its own; `:config` and `:closure-defines` print one entry per
//! line, in key order, each with its own origin. In release mode
//! `:closure-defines` and `:checked-arrays`, errors in dev mode, print in the
//! build's `:release` map, so the printed EDN reads back as a valid file.
//! Comments start at one column, the code padded to [`CODE_WIDTH`] (a longer
//! line takes one space), so the output is the same for the same settings.

use super::{
    BuildConfiguration, BuildMode, BuildSettings, ConfigFile, ConfigMerge, ConfigSource, Setting,
    SettingOrigin, Value, ValueKind, identity,
};

/// The width code is padded to before its ` ; ` comment.
const CODE_WIDTH: usize = 52;

/// A map entry as printed.
enum Item {
    /// `KEY VALUE`, with its origin.
    Leaf {
        key: String,
        value: String,
        origin: String,
    },
    /// `KEY {...}`, a map of the schema, its entries aligned after the `{`.
    Map { key: String, items: Vec<Item> },
    /// `:builds`, then `{:ID` and the build's map on lines of their own.
    Builds { id: String, items: Vec<Item> },
}

/// Lines of code, each with its comment.
#[derive(Default)]
struct Printer {
    lines: Vec<(String, Option<String>)>,
    line: String,
    comment: Option<String>,
}

impl Printer {
    fn column(&self) -> usize {
        self.line.chars().count()
    }

    fn newline(&mut self, indent: usize) {
        let line = std::mem::replace(&mut self.line, " ".repeat(indent));
        self.lines.push((line, self.comment.take()));
    }

    fn map(&mut self, items: &[Item]) {
        self.line.push('{');
        let column = self.column();
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.newline(column);
            }
            self.item(item);
        }
        self.line.push('}');
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Leaf { key, value, origin } => {
                self.line.push_str(key);
                self.line.push(' ');
                self.line.push_str(value);
                self.comment = Some(origin.clone());
            }
            Item::Map { key, items } => {
                self.line.push_str(key);
                self.line.push(' ');
                self.map(items);
            }
            Item::Builds { id, items } => {
                let column = self.column();
                self.line.push_str(":builds");
                self.newline(column);
                self.line.push_str("{:");
                self.line.push_str(id);
                self.newline(column + 1);
                self.map(items);
                self.line.push('}');
            }
        }
    }

    fn finish(mut self, out: &mut String) {
        self.newline(0);
        for (code, comment) in self.lines {
            match comment {
                Some(comment) => {
                    out.push_str(&format!("{code:<CODE_WIDTH$} ; {}", one_line(&comment)));
                }
                None => out.push_str(&code),
            }
            out.push('\n');
        }
    }
}

/// `text` on one line of a comment: a line break, which a flag's value or a
/// directory's name can hold, as `\n` or `\r`, so the text after it stays
/// in the comment.
fn one_line(text: &str) -> String {
    text.replace('\n', "\\n").replace('\r', "\\r")
}

/// `value` in canonical EDN: a set's elements in sorted order, everything
/// else as [`Value::to_edn`] writes it.
fn canonical(value: &Value) -> String {
    match &value.kind {
        ValueKind::Set(items) => {
            let mut texts: Vec<String> = items.iter().map(canonical).collect();
            texts.sort();
            format!("#{{{}}}", texts.join(" "))
        }
        _ => value.to_edn(),
    }
}

impl ConfigFile {
    /// Where `value`, a value of `settings` or inside one, was written: a
    /// flag, or a text `merges` and the file locate, with the mode map it is
    /// in when the build's `:dev` or `:release` map holds it.
    pub fn value_origin(
        &self,
        settings: &BuildSettings,
        value: &Value,
        merges: &[ConfigMerge],
    ) -> SettingOrigin {
        if let ConfigSource::Flag(_) = value.source {
            return settings.flag_origin_of(value.source);
        }
        let mode = settings.mode();
        let in_mode = matches!(value.source, ConfigSource::File | ConfigSource::LightCljs)
            && self
                .build(settings.build())
                .layer(Some(mode))
                .slots
                .iter()
                .flatten()
                .any(|entry| entry.value.holds(value));
        self.texts(merges).origin(value, in_mode.then_some(mode))
    }

    /// `settings`, one build's for its mode with the command's flags applied,
    /// as `--print-config` prints them: a `;; build :ID, MODE mode, from
    /// PATH` line, then the top-level settings the command reads and the
    /// build, merged, as one map, each value with its origin.
    ///
    /// A setting with a default prints at its default, but for the compiler
    /// options, which print when a layer or a flag gives them, and
    /// `:optimizations`, which always prints in release mode. In release
    /// mode `:closure-defines` and `:checked-arrays` print under `:release`,
    /// since either reaching dev mode is an error and the printed EDN reads
    /// back as a valid file. The settings a
    /// mode ignores do not print: in dev mode `:optimizations`,
    /// `:pseudo-names` and `:cljs-warnings`, and the dev server, nREPL,
    /// poller and reloading settings unless `watch` is set. The build's
    /// `:cache-blockers` prints only those the top level's do not hold, which
    /// are unioned into it.
    pub fn print_config(
        &self,
        settings: &BuildSettings,
        merges: &[ConfigMerge],
        watch: bool,
    ) -> String {
        let leaf = |setting: Setting| {
            settings.slot(setting).map(|slot| Item::Leaf {
                key: format!(":{}", setting.key()),
                value: canonical(&slot.value),
                origin: slot.origin.to_string(),
            })
        };
        let given = |setting: Setting| {
            settings
                .slot(setting)
                .filter(|slot| slot.origin.source != ConfigSource::Default)
                .and_then(|_| leaf(setting))
        };
        let map = |key: &str, items: Vec<Item>| {
            (!items.is_empty()).then(|| Item::Map {
                key: format!(":{key}"),
                items,
            })
        };
        let release = settings.mode() == BuildMode::Release;

        let mut top: Vec<Item> = [
            Setting::SourcePaths,
            Setting::CacheRoot,
            Setting::CacheBlockers,
            Setting::CompileThreads,
        ]
        .into_iter()
        .filter_map(leaf)
        .collect();
        if watch {
            top.extend(map(
                "http",
                [Setting::HttpPort, Setting::HttpHost, Setting::HttpStrict]
                    .into_iter()
                    .filter_map(leaf)
                    .collect(),
            ));
            top.extend(leaf(Setting::Nrepl));
            top.extend(map(
                "fs-watch",
                [Setting::FsWatchLoopWait, Setting::FsWatchImpl]
                    .into_iter()
                    .filter_map(leaf)
                    .collect(),
            ));
        }

        let mut build: Vec<Item> = [Setting::Target, Setting::OutputDir, Setting::Modules]
            .into_iter()
            .filter_map(leaf)
            .collect();
        build.extend(self.config_item(settings, merges));
        let mut options: Vec<Item> = Vec::new();
        if release {
            options.extend(leaf(Setting::Optimizations));
            options.extend(
                [
                    Setting::PseudoNames,
                    Setting::ElideAsserts,
                    Setting::CljsWarnings,
                ]
                .into_iter()
                .filter_map(given),
            );
        } else {
            options.extend(given(Setting::ElideAsserts));
        }
        options.extend(
            [
                Setting::MacroExpansions,
                Setting::MacroNodes,
                Setting::MacroTimeoutMs,
                Setting::MacroMemoryLimit,
                Setting::MacroPollBudget,
            ]
            .into_iter()
            .filter_map(given),
        );
        build.extend(map("compiler-options", options));
        let mut build_options: Vec<Item> = leaf(Setting::CacheLevel).into_iter().collect();
        build_options.extend(own_blockers(settings));
        build.extend(map("build-options", build_options));
        if watch {
            build.extend(map(
                "devtools",
                given(Setting::ReloadStrategy).into_iter().collect(),
            ));
        }
        // The keys that are an error in dev mode print under `:release`, so
        // the printed file is valid in both modes.
        if release {
            let mut only: Vec<Item> = Vec::new();
            only.extend(map(
                "compiler-options",
                given(Setting::CheckedArrays).into_iter().collect(),
            ));
            only.extend(self.defines_item(settings, merges));
            build.extend(map("release", only));
        }
        top.push(Item::Builds {
            id: settings.name().to_owned(),
            items: build,
        });

        let mut out = format!(
            ";; build :{}, {} mode, from {}\n",
            settings.name(),
            settings.mode().name(),
            one_line(&self.path().display().to_string())
        );
        let mut printer = Printer::default();
        printer.map(&top);
        printer.finish(&mut out);
        out
    }

    /// `:config`, an entry per line in key order, each value in the
    /// canonical text `--config` gives it.
    fn config_item(&self, settings: &BuildSettings, merges: &[ConfigMerge]) -> Option<Item> {
        let slot = settings.slot(Setting::Config)?;
        let ValueKind::Map(entries) = &slot.value.kind else {
            return None;
        };
        let mut canonical = BuildConfiguration::new();
        for (key, value) in entries {
            if let Some(key) = key.as_keyword() {
                canonical.insert(key, &value.to_edn()).ok()?;
            }
        }
        let items: Vec<Item> = canonical
            .entries()
            .filter_map(|(key, text)| {
                let (_, value) = entries
                    .iter()
                    .find(|(other, _)| other.as_keyword() == Some(key))?;
                Some(Item::Leaf {
                    key: format!(":{key}"),
                    value: text.to_owned(),
                    origin: self.value_origin(settings, value, merges).to_string(),
                })
            })
            .collect();
        Some(entry_map("config", items, slot.origin.to_string()))
    }

    /// `:closure-defines`, an entry per line in name order.
    fn defines_item(&self, settings: &BuildSettings, merges: &[ConfigMerge]) -> Option<Item> {
        let slot = settings.slot(Setting::ClosureDefines)?;
        let ValueKind::Map(entries) = &slot.value.kind else {
            return None;
        };
        let mut sorted: Vec<&(Value, Value)> = entries.iter().collect();
        sorted.sort_by_key(|(name, _)| name.to_edn());
        let items = sorted
            .into_iter()
            .map(|(name, value)| Item::Leaf {
                key: name.to_edn(),
                value: canonical(value),
                origin: self.value_origin(settings, value, merges).to_string(),
            })
            .collect();
        Some(entry_map("closure-defines", items, slot.origin.to_string()))
    }
}

/// A map of user keys: its entries, or `{}` with the map's origin.
fn entry_map(key: &str, items: Vec<Item>, origin: String) -> Item {
    if items.is_empty() {
        Item::Leaf {
            key: format!(":{key}"),
            value: "{}".into(),
            origin,
        }
    } else {
        Item::Map {
            key: format!(":{key}"),
            items,
        }
    }
}

/// The build's own `:cache-blockers`: those the top level's, unioned into
/// them, do not hold.
fn own_blockers(settings: &BuildSettings) -> Option<Item> {
    let slot = settings.slot(Setting::BuildCacheBlockers)?;
    let ValueKind::Set(items) = &slot.value.kind else {
        return None;
    };
    let top: Vec<String> = match settings.get(Setting::CacheBlockers).map(|set| &set.kind) {
        Some(ValueKind::Set(top)) => top.iter().map(identity).collect(),
        _ => Vec::new(),
    };
    let own: Vec<Value> = items
        .iter()
        .filter(|item| !top.contains(&identity(item)))
        .cloned()
        .collect();
    (!own.is_empty()).then(|| Item::Leaf {
        key: ":cache-blockers".into(),
        value: canonical(&Value::from(ValueKind::Set(own))),
        origin: slot.origin.to_string(),
    })
}
