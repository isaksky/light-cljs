//! Intl namespace, BCP 47 locale identifiers, and pinned locale data.
//! Service-specific ResolveLocale algorithms remain separate from canonical
//! tag parsing and data lookup (see docs/architecture.md, "Intl locale data").

pub mod data;

pub use data::locale::LocaleData;

/// Locale used by runs without an embedding host, and as the final data root.
pub const DEFAULT_LOCALE: &str = "en-US";

/// The agreed initial service locale coverage. A tag may use parent data.
pub const LOCALES: &[&str] = &[
    "en",
    "en-US",
    "en-GB",
    "de",
    "fr",
    "es",
    "ja",
    "zh",
    "zh-Hant",
    "zh-Hans-CN",
    "ar",
    "ru",
    "pt-BR",
    "tr",
    "zh-TW",
];

/// Read a locale as an unrecorded leaf host effect. `None` represents a run
/// without a host. This does not canonicalize or validate the host's tag.
pub fn default_locale(host: Option<&mut dyn crate::Host>) -> String {
    match host {
        Some(host) => crate::host::call_default_locale(host),
        None => DEFAULT_LOCALE.to_owned(),
    }
}

/// Look up canonical tags from most specific to least specific. Unsupported
/// tags return `None`: the caller decides when to use its default locale.
/// `en-GB` has exact data, `en-US` uses `en`, `pt-BR` uses `pt`, and
/// `zh-Hans-CN` uses `zh-Hans`. Language-tag validation is not performed here.
pub fn lookup(mut tag: &str) -> Option<&'static LocaleData> {
    loop {
        if let Some(data) = data::locale::locale_data_table
            .iter()
            .find(|data| data.tag() == Some(tag))
        {
            return Some(data);
        }
        let (parent, _) = tag.rsplit_once('-')?;
        tag = parent;
        // RFC 4647 lookup removes a trailing extension singleton with its data.
        if tag.rsplit('-').next().is_some_and(|part| part.len() == 1) {
            tag = tag.rsplit_once('-')?.0;
        }
    }
}

/// Data fallback after the caller has tried its requested locale list. This
/// keeps unsupported detection in `lookup` distinct from default resolution.
pub fn resolve<'a>(
    requested: impl IntoIterator<Item = &'a str>,
    default: &str,
) -> &'static LocaleData {
    requested
        .into_iter()
        .find_map(lookup)
        .or_else(|| lookup(default))
        .or_else(|| lookup(DEFAULT_LOCALE))
        .expect("the pinned snapshot contains en root data")
}

/// Interpret one NUL-terminated string in a byte pool. Embedded separators and
/// the final compiler-added terminator remain in the generated representation.
/// Bounds, terminators and UTF-8 are checked without unsafe code.
pub fn pool_string(pool: &[u8], index: usize) -> Option<&str> {
    let item = pool.split_inclusive(|&byte| byte == 0).nth(index)?;
    std::str::from_utf8(item.strip_suffix(&[0])?).ok()
}

/// Read the first string in a generated table's selected byte pool.
pub fn table_string<'a>(table: &[Option<&'a [u8]>], index: usize) -> Option<&'a str> {
    pool_string((*table.get(index)?)?, 0)
}

impl LocaleData {
    pub fn tag(&self) -> Option<&'static str> {
        pool_string(self.locale?, 0)
    }

    pub fn decimal_separator(&self) -> Option<&'static str> {
        table_string(
            data::numbers::decimal_tab,
            self.symbols.decimal_idx as usize,
        )
    }

    pub fn group_separator(&self) -> Option<&'static str> {
        table_string(data::numbers::group_tab, self.symbols.group_idx as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intl_locale_data_lookup() {
        let de = lookup("de").unwrap();
        assert_eq!(de.decimal_separator(), Some(","));
        assert_eq!(de.group_separator(), Some("."));
        assert_eq!(lookup("fr").unwrap().group_separator(), Some("\u{202f}"));
        assert_eq!(lookup("ru").unwrap().group_separator(), Some("\u{a0}"));
        for (tag, record) in [
            ("en-GB", "en-GB"),
            ("en-US", "en"),
            ("pt-BR", "pt"),
            ("zh-Hant", "zh-Hant"),
            ("zh-TW", "zh-TW"),
            ("zh-Hans-CN", "zh-Hans"),
            ("en-GB-u-nu-latn", "en-GB"),
        ] {
            assert_eq!(lookup(tag).unwrap().tag(), Some(record));
        }
        for tag in LOCALES {
            assert!(lookup(tag).is_some(), "missing {tag}");
        }
        assert!(lookup("xx").is_none());
        assert!(lookup("xx-en").is_none());
        assert_eq!(resolve(["xx", "fr"], "de").tag(), Some("fr"));
        assert_eq!(resolve(["xx"], "de").tag(), Some("de"));
        assert_eq!(resolve(["xx"], "xx").tag(), Some("en"));
        assert_eq!(resolve([], "en-US").tag(), Some("en"));
    }

    #[test]
    fn intl_default_locale_from_host() {
        let rt = crate::Runtime::new();
        let mut host = crate::host::sim::SimHost::new(&rt);
        host.set_default_locale("fr");
        assert_eq!(default_locale(Some(&mut host)), "fr");
        host.set_default_locale("ja");
        assert_eq!(default_locale(Some(&mut host)), "ja");
        assert_eq!(default_locale(None), "en-US");
        assert_eq!(default_locale(Some(&mut crate::host::NoHost)), "en-US");
        assert!(!crate::host::inside_effect());
    }

    #[test]
    fn intl_default_locale_is_a_guarded_leaf() {
        struct Probe(crate::Task);
        impl crate::Host for Probe {
            fn effect(&mut self, _: crate::HostCall) -> Result<crate::Primitive, String> {
                unreachable!()
            }
            fn default_locale(&mut self) -> String {
                assert!(crate::host::inside_effect());
                assert!(matches!(
                    self.0.run(),
                    Err(crate::RunError::InsideHostEffect)
                ));
                "fr".to_owned()
            }
        }
        let rt = crate::Runtime::new();
        let code = crate::frontend::compile_script("locale-leaf", "1").unwrap();
        let mut host = Probe(rt.spawn(&code, &[]).unwrap());
        assert_eq!(default_locale(Some(&mut host)), "fr");
        assert!(!crate::host::inside_effect());
        assert!(matches!(
            host.0.run().unwrap(),
            crate::RunOutcome::Complete(_)
        ));
        assert!(host.0.dependency_log().edges().is_empty());
    }

    #[test]
    fn intl_repacked_locale_references() {
        use data::{dates::*, numbers::*};
        for locale in data::locale::locale_data_table {
            assert!(locale.decimal_separator().is_some());
            assert!(locale.group_separator().is_some());
            for (table, index, count) in [
                (month_long_tab, locale.month_long_idx, 12),
                (month_short_tab, locale.month_short_idx, 12),
                (month_narrow_tab, locale.month_narrow_idx, 12),
                (weekday_long_tab, locale.weekday_long_idx, 7),
                (weekday_short_tab, locale.weekday_short_idx, 7),
                (weekday_narrow_tab, locale.weekday_narrow_idx, 7),
                (era_tab, locale.era_idx, 6),
                (ampm_tab, locale.ampm_idx, 2),
            ] {
                let pool = table[index as usize].unwrap();
                for item in 0..count {
                    assert!(pool_string(pool, item).is_some(), "{:?}", locale.tag());
                }
            }
            for index in [
                locale.datePatternShort_idx,
                locale.datePatternMedium_idx,
                locale.datePatternLong_idx,
                locale.datePatternFull_idx,
            ] {
                assert!(table_string(date_pattern_tab, index as usize).is_some());
            }
            for index in [
                locale.timePatternShort_idx,
                locale.timePatternMedium_idx,
                locale.timePatternLong_idx,
                locale.timePatternFull_idx,
            ] {
                assert!(table_string(time_pattern_tab, index as usize).is_some());
            }
            for index in [
                locale.patterns.dec_pos,
                locale.patterns.dec_neg,
                locale.patterns.cur_pos,
                locale.patterns.cur_neg,
                locale.patterns.acc_pos,
                locale.patterns.acc_neg,
                locale.patterns.pct_pos,
                locale.patterns.pct_neg,
            ] {
                assert!(table_string(pattern_tab, index as usize).is_some());
            }
            assert_eq!(
                locale.compactPatterns.unwrap().len(),
                locale.numCompactPatterns as usize
            );
            let end = locale.currency_offset as usize + 2 * locale.num_currencies as usize;
            assert!(end <= locale_currency_data.len());
        }
    }

    #[test]
    fn intl_byte_pool_accessors() {
        assert_eq!(pool_string(b"one\0deux\0\0", 0), Some("one"));
        assert_eq!(pool_string(b"one\0deux\0\0", 1), Some("deux"));
        assert_eq!(pool_string(b"one\0deux\0\0", 2), Some(""));
        assert_eq!(pool_string(b"one\0", 1), None);
        assert_eq!(pool_string(b"unterminated", 0), None);
        assert_eq!(pool_string(b"\xff\0", 0), None);
        assert_eq!(table_string(&[None], 0), None);
        assert_eq!(table_string(&[], 0), None);
        let fr = lookup("fr").unwrap();
        let months = data::dates::month_long_tab[fr.month_long_idx as usize].unwrap();
        assert_eq!(pool_string(months, 1), Some("février"));
        assert_eq!(pool_string(months, 11), Some("décembre"));
        assert_eq!(data::likely_subtags::likely_subtags_count, 7788);
        assert_eq!(data::display_names::display_names_count, 2);
        assert_eq!(data::locale::locale_data_count, 14);
    }
}

pub mod likely;
pub(crate) mod locale;
pub(crate) mod number_format;
pub mod tag;

pub(crate) mod collator;
pub(crate) mod plural_rules;
pub(crate) mod service;

pub(crate) mod date_time_format;
pub(crate) mod list_format;

pub(crate) mod relative_time_format;
