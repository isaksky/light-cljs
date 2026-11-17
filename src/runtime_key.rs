//! The per-build runtime key (docs/value-layout.md, section 1, "Derivation").
//!
//! Every protocol slot, marker, mask, kind and class field name ends in `$`
//! and the build's key, eight characters of `[0-9a-z]`. The runtime text
//! (`src/js_runtime.js`) and emission write the placeholder
//! [`KEY_PLACEHOLDER`] for it; the compile-time runtime keeps the
//! placeholder as its key, so compiled namespace artifacts, which hold
//! compile-time JavaScript and keyed names in their IR, never depend on a
//! build's key. The output writer substitutes the key for the placeholder
//! where output leaves the compiler for a page:
//!
//! - release output ([`crate::PreparedApplication`]'s `emit_*`): the key is
//!   derived from the output's text with the placeholders in it and the
//!   optimization level ([`RuntimeKey::release`]), so the same program at
//!   the same level gets the same key on every run and machine, and two
//!   outputs that differ in anything get different keys;
//! - dev builds, their hot reloads and the forms the REPL sends to a page
//!   ([`RuntimeKey::dev`]): the key is derived from the whole runtime
//!   source and the build's identity, and the dev session keeps it, so
//!   every unit written against a running page has the key the page loaded
//!   with, and two dev builds on one page have two keys.
//!
//! The runtime's `runtimeKey` unit registers `key -> hash` in the page-wide
//! registry `globalThis[Symbol.for("light-cljs.runtime-keys")]` as it loads,
//! and throws when the key is registered with another hash: the
//! [`HASH_PLACEHOLDER`] it holds is written as the hash of the output (of
//! the runtime and the build's identity for a dev build).
//!
//! A string literal the emitter writes never holds a placeholder: it spells
//! the `$` of one as `\u0024` (`crate::emitter`), so the substitution never
//! changes a string of the program. A placeholder is substituted only at
//! the end of a name: followed by a character that cannot continue a
//! JavaScript identifier, as `src/runtime_manifest.rs` requires of every
//! placeholder in the runtime text.

use crate::application::OptimizationLevel;
use crate::sha256::Fingerprint;
use crate::source_map::SourceMapping;

/// The suffix of every keyed name in the runtime text and in emission,
/// which the output writer replaces with `$` and the build's key.
pub const KEY_PLACEHOLDER: &str = "$LKEY";

/// The hash the runtime registers with its key, as the runtime text holds
/// it (a whole string literal of the `runtimeKey` unit).
pub const HASH_PLACEHOLDER: &str = "$LHASH";

/// The codes of the protocol names `protocolSlot` can name, as the runtime
/// text holds them (a whole string literal of its `protocolSlotCodes`
/// unit): the output writer writes the build's table for it where it gives
/// keyed names short names ([`RuntimeKey::shortened`]) and leaves it
/// elsewhere, where `protocolSlot` names every slot in full.
pub const NAMES_PLACEHOLDER: &str = "$LNAMES";

/// The characters of a short name's code, after `$` and the key, in the
/// order the most frequent names take them.
const CODE_DIGITS: &[u8; 52] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The characters of a key.
pub const KEY_LENGTH: usize = 8;

/// The short names of one `Advanced` output's keyed names
/// (docs/value-layout.md, section 1, "Short names at `:advanced`"): each
/// keyed name whose stem is a class field (`cnt`), a kind, cell or record
/// name (`cljs$lang$kind$`), a mask, a marker or a protocol slot is written
/// `$`, the key and a code of one or two letters, the most frequent first.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ShortNames {
    /// The code of each stem that takes one.
    codes: std::collections::HashMap<String, String>,
    /// What the runtime's `protocolSlot` reads for [`NAMES_PLACEHOLDER`]:
    /// ` stem code stem code `: the stems it can name and their codes, each
    /// between spaces.
    table: String,
}

impl ShortNames {
    /// The short names of the keyed names in `texts`, which hold the key
    /// placeholder: by use, most first, ties in the stems' order.
    fn of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut uses = std::collections::HashMap::<&str, usize>::new();
        for text in texts {
            for (at, _) in text.match_indices(KEY_PLACEHOLDER) {
                if !name_end(&text[at + KEY_PLACEHOLDER.len()..]) {
                    continue;
                }
                let stem = &text[stem_start(text, at)..at];
                if shortened_stem(stem) {
                    *uses.entry(stem).or_default() += 1;
                }
            }
        }
        let mut ranked = uses.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|(a, m), (b, n)| n.cmp(m).then(a.cmp(b)));
        let mut codes = std::collections::HashMap::with_capacity(ranked.len());
        let mut table = String::from(" ");
        for (index, (stem, _)) in ranked.into_iter().enumerate() {
            let code = short_code(index);
            if slot_stem(stem) {
                table.push_str(stem);
                table.push(' ');
                table.push_str(&code);
                table.push(' ');
            }
            codes.insert(stem.to_owned(), code);
        }
        Self { codes, table }
    }
}

/// Whether the keyed name of `stem` takes a short name: a field of the
/// runtime's classes (a stem without `$`), or the shape of a protocol slot,
/// marker or mask, which the kind, cell and record names share
/// (`crate::analyzer::protocol_member`). The key's own spelling (`"$LKEY"`,
/// no stem), the page globals of dev output (`__lightDev`) and a stem a
/// program builds otherwise keep their keyed names.
fn shortened_stem(stem: &str) -> bool {
    if stem.contains('$') {
        return crate::analyzer::protocol_member(stem);
    }
    stem.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && !stem.starts_with("__light")
}

/// Whether `protocolSlot` can name the keyed name of `stem`: a protocol
/// slot or marker, which it names from a protocol's key; the masks and the
/// kind, cell and record names (`cljs$lang$`) it never names.
fn slot_stem(stem: &str) -> bool {
    stem.contains('$') && !stem.starts_with("cljs$lang$") && crate::analyzer::protocol_member(stem)
}

/// The code of the short name ranked `index`: one of [`CODE_DIGITS`], then
/// two, bijectively.
fn short_code(index: usize) -> String {
    let base = CODE_DIGITS.len();
    let mut code = Vec::new();
    let mut n = index + 1;
    while n > 0 {
        n -= 1;
        code.push(CODE_DIGITS[n % base]);
        n /= base;
    }
    code.reverse();
    String::from_utf8(code).unwrap()
}

/// The byte offset in `text` where the name that the placeholder at `at`
/// ends starts.
fn stem_start(text: &str, at: usize) -> usize {
    text[..at]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| c.is_alphanumeric() || matches!(c, '_' | '$'))
        .last()
        .map_or(at, |(start, _)| start)
}

/// The key of one build and the hash it registers with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeKey {
    key: String,
    hash: Option<String>,
    /// The short names of the keyed names, at `Advanced` without pseudo-names
    /// ([`RuntimeKey::shortened`]).
    names: Option<ShortNames>,
}

impl RuntimeKey {
    /// The key of release output whose files are `texts`, in order, with the
    /// placeholders in them, written at `level`; `runtime_key` is the build
    /// option `:runtime-key` (`--runtime-key`,
    /// [`crate::ApplicationOptions::runtime_key`]), which changes the key of
    /// output that is otherwise the same. The output registers the hash of
    /// the same inputs.
    pub fn release<'a>(
        texts: impl IntoIterator<Item = &'a str>,
        level: OptimizationLevel,
        runtime_key: Option<&str>,
    ) -> Self {
        let mut fingerprint = Fingerprint::new("light-cljs release runtime key 1");
        fingerprint.text(level.name()).optional_text(runtime_key);
        for text in texts {
            fingerprint.present().text(text);
        }
        fingerprint.absent();
        let digest = fingerprint.finish();
        Self {
            key: base36_key(&digest),
            hash: Some(hex(&digest[8..16])),
            names: None,
        }
    }

    /// The key of the dev build whose identity is `identity` (its build of
    /// light-cljs.edn and its entry namespace, `crate::dev_build::dev_key`):
    /// derived from the whole runtime source and the identity, so it depends
    /// on neither the build's sources nor an edit, and the dev session keeps
    /// it for every unit, reload and REPL form it writes against the page
    /// whose `runtime.js` loaded with it. Two different dev builds on one
    /// page have two keys, so they keep their values and their dev state
    /// apart; two copies of one build share it. The page registers the hash
    /// of the same inputs, so two different builds whose keys match by
    /// accident are caught as they load.
    pub fn dev(identity: &str) -> Self {
        let mut fingerprint = Fingerprint::new("light-cljs dev runtime key 2");
        fingerprint.digest(runtime_digest()).text(identity);
        let digest = fingerprint.finish();
        Self {
            key: base36_key(&digest),
            hash: Some(hex(&digest[8..16])),
            names: None,
        }
    }

    /// This key, writing each keyed name of `texts` (the output's files, with
    /// the placeholders in them) in short: `$`, the key and a code of one
    /// or two letters, the most used first, and the table of the protocol
    /// names' codes for the runtime's `protocolSlot` ([`NAMES_PLACEHOLDER`]).
    /// The output writer shortens `Advanced` output without pseudo-names
    /// (docs/value-layout.md, section 1, "Short names at `:advanced`"); the
    /// names depend on the text alone, so two copies of one build share
    /// them, as they share the key.
    pub fn shortened<'a>(mut self, texts: impl IntoIterator<Item = &'a str>) -> Self {
        self.names = Some(ShortNames::of(texts));
        self
    }

    /// This key for the units written against a page that has loaded its
    /// runtime: they hold no hash placeholder, so the substitution leaves a
    /// string that spells it as it is.
    pub fn units(&self) -> Self {
        Self {
            key: self.key.clone(),
            hash: None,
            names: None,
        }
    }

    /// The eight characters after the `$` of every keyed name.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The hash the runtime registers with the key, if it writes one.
    pub fn hash(&self) -> Option<&str> {
        self.hash.as_deref()
    }

    /// The key and hash the output writer wrote into `text`, output that
    /// holds a runtime: the arguments of its `runtimeKey` registration.
    pub fn written(text: &str) -> Option<Self> {
        let at = text.find("Symbol.for(\"light-cljs.runtime-keys\")")?;
        let rest = &text[at..];
        let start = rest.find("(\"$")? + 3;
        let key = rest.get(start..start + KEY_LENGTH)?;
        let after = rest[start + KEY_LENGTH..].strip_prefix('"')?;
        let after = after.trim_start_matches([',', ' ']).strip_prefix('"')?;
        let hash = &after[..after.find('"')?];
        Some(Self {
            key: key.to_owned(),
            hash: Some(hash.to_owned()),
            names: None,
        })
    }

    /// Each short name this key writes, `$`, the key and a code, with the
    /// whole keyed name it stands for, the stem, `$` and the key: a property
    /// renaming report. Empty without short names.
    pub fn short_names(&self) -> Vec<(String, String)> {
        let Some(names) = &self.names else {
            return Vec::new();
        };
        let mut pairs = names
            .codes
            .iter()
            .map(|(stem, code)| {
                (
                    format!("${}{code}", self.key),
                    format!("{stem}${}", self.key),
                )
            })
            .collect::<Vec<_>>();
        pairs.sort();
        pairs
    }

    /// The bytes this key's substitution adds to `text` (removes, when
    /// negative): with short names, a fragment of the output's text.
    pub(crate) fn written_growth(&self, text: &str) -> isize {
        let mut added = 0isize;
        self.substitute(text, |_, delta| added += delta);
        added
    }

    /// `text` with the key and the hash written for their placeholders.
    pub fn apply(&self, text: &str) -> String {
        self.substitute(text, |_, _| {})
    }

    /// [`RuntimeKey::apply`] undone: `text` with this key and hash written
    /// back as their placeholders, for comparing the text of outputs whose
    /// keys differ.
    pub fn unapply(&self, text: &str) -> String {
        let text = text.replace(&format!("${}", self.key), KEY_PLACEHOLDER);
        match &self.hash {
            Some(hash) => text.replace(&format!("\"{hash}\""), &format!("\"{HASH_PLACEHOLDER}\"")),
            None => text,
        }
    }

    /// [`RuntimeKey::apply`] of `text`, whose source map is `mapping`: each
    /// segment after a placeholder on its line moves by the length the
    /// substitution added.
    pub fn apply_mapped(&self, text: &str, mapping: &mut SourceMapping) -> String {
        let mut shifts = Vec::new();
        let mut cursor = Position::default();
        let written = self.substitute(text, |at, delta| {
            let (line, column) = cursor.advance(text, at);
            shifts.push((line, column, delta));
        });
        mapping.shift_columns(&shifts);
        written
    }

    /// `text` with each placeholder at the end of a name replaced, calling
    /// `found` with the byte offset of each in `text` and the length its
    /// replacement adds, in order.
    fn substitute(&self, text: &str, mut found: impl FnMut(usize, isize)) -> String {
        let mut out = String::with_capacity(text.len() + text.len() / 64);
        let mut copied = 0;
        let mut from = 0;
        while let Some(offset) = text[from..].find("$L") {
            let at = from + offset;
            from = at + 2;
            let rest = &text[at..];
            let (placeholder, replacement) = if rest.starts_with(KEY_PLACEHOLDER) {
                (KEY_PLACEHOLDER, Some(self.key.as_str()))
            } else if rest.starts_with(HASH_PLACEHOLDER) {
                (HASH_PLACEHOLDER, self.hash.as_deref())
            } else if rest.starts_with(NAMES_PLACEHOLDER) {
                (
                    NAMES_PLACEHOLDER,
                    self.names.as_ref().map(|names| names.table.as_str()),
                )
            } else {
                continue;
            };
            let Some(replacement) = replacement else {
                continue;
            };
            if !name_end(&rest[placeholder.len()..]) {
                continue;
            }
            // A keyed name with a short name is written whole as it: the
            // stem before the placeholder goes too.
            let short = (placeholder == KEY_PLACEHOLDER)
                .then_some(self.names.as_ref())
                .flatten()
                .and_then(|names| {
                    let start = stem_start(text, at);
                    names.codes.get(&text[start..at]).map(|code| (start, code))
                });
            if let Some((start, code)) = short {
                out.push_str(&text[copied..start]);
                let written = out.len();
                out.push('$');
                out.push_str(&self.key);
                out.push_str(code);
                let end = at + placeholder.len();
                // Segments after the name move; those in it stay.
                found(
                    end - 1,
                    (out.len() - written) as isize - (end - start) as isize,
                );
                copied = end;
                from = copied;
                continue;
            }
            out.push_str(&text[copied..at]);
            let written = out.len();
            if placeholder == KEY_PLACEHOLDER {
                out.push('$');
            }
            out.push_str(replacement);
            found(
                at,
                (out.len() - written) as isize - placeholder.len() as isize,
            );
            copied = at + placeholder.len();
            from = copied;
        }
        out.push_str(&text[copied..]);
        out
    }
}

/// The length of `text` once the output writer has written a key and a
/// hash for its placeholders, whatever the key: four bytes more than each
/// key placeholder at a name's end, ten more than the hash's. Size reports
/// count runtime text this way.
pub fn written_len(text: &str) -> usize {
    text.len() + growth(text)
}

/// The bytes [`written_len`] adds to `text`.
pub(crate) fn growth(text: &str) -> usize {
    if !text.contains("$L") {
        return 0;
    }
    let key = RuntimeKey {
        key: "0".repeat(KEY_LENGTH),
        hash: Some("0".repeat(16)),
        names: None,
    };
    let mut added = 0isize;
    key.substitute(text, |_, delta| added += delta);
    added as usize
}

/// `text`, output that may hold a runtime, with the key and hash its
/// runtime registers written back as their placeholders
/// ([`RuntimeKey::unapply`]); output without a runtime is returned as is.
pub fn unkeyed(text: &str) -> String {
    match RuntimeKey::written(text) {
        Some(key) => key.unapply(text),
        None => text.to_owned(),
    }
}

/// Whether text that starts with `rest` ends a name before it: its first
/// character, if any, cannot continue a JavaScript identifier.
fn name_end(rest: &str) -> bool {
    rest.chars()
        .next()
        .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '\u{200c}' | '\u{200d}')))
}

/// The digest of the whole runtime source, from which every dev key is
/// derived with the build's identity ([`RuntimeKey::dev`]).
fn runtime_digest() -> &'static [u8; 32] {
    static DIGEST: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    DIGEST.get_or_init(|| {
        let mut fingerprint = Fingerprint::new("light-cljs dev runtime 1");
        fingerprint.text(crate::runtime_manifest::RUNTIME);
        fingerprint.finish()
    })
}

/// [`KEY_LENGTH`] base-36 digits of the first eight bytes of `digest`.
fn base36_key(digest: &[u8; 32]) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut value = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let mut key = [0u8; KEY_LENGTH];
    for digit in key.iter_mut().rev() {
        *digit = DIGITS[(value % 36) as usize];
        value /= 36;
    }
    String::from_utf8(key.to_vec()).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A walk over a text that counts lines and UTF-16 columns as source maps
/// do (`crate::source_map`).
#[derive(Default)]
struct Position {
    at: usize,
    line: u32,
    column: u32,
    carriage: bool,
}

impl Position {
    /// The line and column of byte `to`, at or after the last position.
    fn advance(&mut self, text: &str, to: usize) -> (u32, u32) {
        for ch in text[self.at..to].chars() {
            match ch {
                '\n' if self.carriage => {}
                '\n' | '\r' | '\u{2028}' | '\u{2029}' => {
                    self.line += 1;
                    self.column = 0;
                }
                _ => self.column += ch.len_utf16() as u32,
            }
            self.carriage = ch == '\r';
        }
        self.at = to;
        (self.line, self.column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_replaced_only_at_the_end_of_a_name() {
        let key = RuntimeKey {
            key: "q7kx0abc".into(),
            hash: Some("0123456789abcdef".into()),
            names: None,
        };
        assert_eq!(
            key.apply("a.cljs$core$ISeq$$LKEY = b.store$LKEY; s + \"$LKEY\"; \"$LHASH\""),
            "a.cljs$core$ISeq$$q7kx0abc = b.store$q7kx0abc; s + \"$q7kx0abc\"; \"0123456789abcdef\""
        );
        assert_eq!(
            key.apply("$LKEYS $LKEY_x $LKEY$ $LHASHED $L"),
            "$LKEYS $LKEY_x $LKEY$ $LHASHED $L"
        );
        let units = RuntimeKey { hash: None, ..key };
        assert_eq!(
            units.apply("x.y$LKEY \"$LHASH\""),
            "x.y$q7kx0abc \"$LHASH\""
        );
    }

    #[test]
    fn shortened_names_keep_the_key_and_rank_by_use() {
        let text = "a.cnt$LKEY + b.cnt$LKEY + \"cnt$LKEY\" in c; d.cljs$core$ISeq$_first$arity$1$LKEY(d); \
                    e.cljs$core$ISeq$$LKEY; f.cljs$lang$kind$$LKEY; g.cljs$lang$kind$$LKEY; \
                    s + \"$LKEY\"; \"x\" + \"$$LKEY\"; globalThis.__lightDev$LKEY; h.x$y$LKEY; \"$LNAMES\"; \"$LHASH\"";
        let key = RuntimeKey {
            key: "q7kx0abc".into(),
            hash: Some("0123456789abcdef".into()),
            names: None,
        }
        .shortened([text]);
        // The most used first; the key's own spelling, a stem built at run
        // time, a page global and a name of no keyed shape stay keyed whole;
        // the table names the slot and marker that `protocolSlot` can name.
        assert_eq!(
            key.apply(text),
            "a.$q7kx0abca + b.$q7kx0abca + \"$q7kx0abca\" in c; d.$q7kx0abcd(d); \
             e.$q7kx0abcc; f.$q7kx0abcb; g.$q7kx0abcb; \
             s + \"$q7kx0abc\"; \"x\" + \"$$q7kx0abc\"; globalThis.__lightDev$q7kx0abc; h.x$y$q7kx0abc; \
             \" cljs$core$ISeq$ c cljs$core$ISeq$_first$arity$1 d \"; \"0123456789abcdef\""
        );
        // Size reports count what the substitution writes.
        assert_eq!(
            key.written_growth(text),
            key.apply(text).len() as isize - text.len() as isize
        );
        // The codes run through one letter, then two.
        assert_eq!(
            [0, 25, 26, 51, 52, 53, 52 * 53 - 1].map(short_code),
            ["a", "z", "A", "Z", "aa", "ab", "ZZ"]
        );
        // A source map's segments after a short name move by what it
        // changed; one at its start stays.
        let code = "f(a.cljs$core$ISeq$_first$arity$1$LKEY, b)";
        let mut marks = crate::source_map::Marks::default();
        for (output, byte) in [(0, 0), (2, 3), (40, 7)] {
            marks.position(output, 0, byte);
        }
        let mut mapping = marks.finish(code);
        let short = RuntimeKey {
            names: None,
            ..key.clone()
        }
        .shortened([code]);
        assert_eq!(short.apply_mapped(code, &mut mapping), "f(a.$q7kx0abca, b)");
        assert_eq!(mapping.original(0, 2), Some((0, 3)));
        assert_eq!(mapping.original(0, 16), Some((0, 7)));
        assert_eq!(mapping.original(0, 15), Some((0, 3)));
        // Without short names the table stays the placeholder.
        let whole = RuntimeKey { names: None, ..key };
        assert!(whole.apply(text).contains("e.cljs$core$ISeq$$q7kx0abc; "));
        assert!(
            whole
                .apply(text)
                .ends_with("\"$LNAMES\"; \"0123456789abcdef\"")
        );
    }

    #[test]
    fn mapped_text_moves_the_segments_after_a_placeholder_on_its_line() {
        let code = "a.x$LKEY = f(1), \"$LHASH\", y.$LKEY;\nb.z$LKEY(c)";
        let mut marks = crate::source_map::Marks::default();
        for (output, byte) in [(0, 0), (11, 5), (36, 9), (44, 12)] {
            marks.position(output, 0, byte);
        }
        let mut mapping = marks.finish(code);
        let key = RuntimeKey {
            key: "q7kx0abc".into(),
            hash: Some("0123456789abcdef".into()),
            names: None,
        };
        let written = key.apply_mapped(code, &mut mapping);
        assert_eq!(
            written,
            "a.x$q7kx0abc = f(1), \"0123456789abcdef\", y.$q7kx0abc;\nb.z$q7kx0abc(c)"
        );
        // `f` moves by the key's four bytes; `b` starts its own line; `(c)`
        // moves by four.
        assert_eq!(mapping.original(0, 14), Some((0, 0)));
        assert_eq!(mapping.original(0, 15), Some((0, 5)));
        assert_eq!(mapping.original(1, 0), Some((0, 9)));
        assert_eq!(mapping.original(1, 3), Some((0, 9)));
        assert_eq!(mapping.original(1, 12), Some((0, 12)));
        // Past the hash (16 characters for 6) and the second key.
        assert_eq!(&written[52..53], ";");
        assert_eq!(mapping.original(0, 52), Some((0, 5)));
    }

    #[test]
    fn keys_are_eight_base36_characters_of_their_inputs() {
        let a = RuntimeKey::release(["(() => 1)()"], OptimizationLevel::Advanced, None);
        assert_eq!(a.key().len(), KEY_LENGTH);
        assert!(
            a.key()
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        );
        assert_eq!(
            a,
            RuntimeKey::release(["(() => 1)()"], OptimizationLevel::Advanced, None)
        );
        for other in [
            RuntimeKey::release(["(() => 2)()"], OptimizationLevel::Advanced, None),
            RuntimeKey::release(["(() => 1)()"], OptimizationLevel::None, None),
            RuntimeKey::release(["(() => 1)()"], OptimizationLevel::Advanced, Some("x")),
            RuntimeKey::release(["(() => 1)", "()"], OptimizationLevel::Advanced, None),
        ] {
            assert_ne!(a.key(), other.key());
            assert_ne!(a.hash(), other.hash());
        }
        // A dev key is one per build identity (light-cljs-1045).
        assert_eq!(RuntimeKey::dev("a"), RuntimeKey::dev("a"));
        assert_ne!(RuntimeKey::dev("a").key(), RuntimeKey::dev("b").key());
        assert_ne!(RuntimeKey::dev("a").hash(), RuntimeKey::dev("b").hash());
        let units = RuntimeKey::dev("a").units();
        assert_eq!(units.key(), RuntimeKey::dev("a").key());
        assert_eq!(units.hash(), None);
    }
}
