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
//!   source, so every unit written against a running page has the key the
//!   page loaded with.
//!
//! The runtime's `runtimeKey` unit registers `key -> hash` in the page-wide
//! registry `globalThis[Symbol.for("light-cljs.runtime-keys")]` as it loads,
//! and throws when the key is registered with another hash: the
//! [`HASH_PLACEHOLDER`] it holds is written as the hash of the output (of
//! the build's identity for a dev build).
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

/// The characters of a key.
pub const KEY_LENGTH: usize = 8;

/// The key of one build and the hash it registers with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeKey {
    key: String,
    hash: Option<String>,
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
        }
    }

    /// The key of every dev build of this compiler: derived from the whole
    /// runtime source, so it depends on neither a build's sources nor an
    /// edit, and every unit, reload and REPL form written against a page
    /// has the key the page's `runtime.js` loaded with. The page registers
    /// the hash of `identity`, the build's identity (its build of
    /// light-cljs.edn and its entry namespace,
    /// `crate::dev_build::runtime_file`), so two different dev builds on
    /// one page, which share the key, are caught as they load.
    pub fn dev(identity: &str) -> Self {
        let mut fingerprint = Fingerprint::new("light-cljs dev build identity 1");
        fingerprint.text(identity);
        Self {
            key: dev_key().to_owned(),
            hash: Some(hex(&fingerprint.finish()[..8])),
        }
    }

    /// The dev key ([`RuntimeKey::dev`]) for the units written against a
    /// page that has loaded its runtime: they hold no hash placeholder.
    pub fn dev_units() -> Self {
        Self {
            key: dev_key().to_owned(),
            hash: None,
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
        })
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
            } else {
                continue;
            };
            let Some(replacement) = replacement else {
                continue;
            };
            if !name_end(&rest[placeholder.len()..]) {
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

/// The key of every dev build ([`RuntimeKey::dev`]).
fn dev_key() -> &'static str {
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        let mut fingerprint = Fingerprint::new("light-cljs dev runtime key 1");
        fingerprint.text(crate::runtime_manifest::RUNTIME);
        base36_key(&fingerprint.finish())
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
        assert_eq!(RuntimeKey::dev("a").key(), RuntimeKey::dev("b").key());
        assert_ne!(RuntimeKey::dev("a").hash(), RuntimeKey::dev("b").hash());
        assert_eq!(RuntimeKey::dev_units().key(), RuntimeKey::dev("a").key());
    }
}
