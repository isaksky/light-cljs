//! Likely-subtag operations on parsed locale identifiers. Service locale
//! coverage is separate from this supplemental CLDR table.
use super::{data::likely_subtags::likely_subtags_tab, pool_string, tag::LanguageTag};

/// Ordered lookup in the pinned table. Read both NUL-delimited strings rather
/// than interpreting the entire entry as a single C string.
pub fn lookup(key: &str) -> Option<&'static str> {
    let index = likely_subtags_tab
        .binary_search_by(|entry| {
            entry
                .and_then(|pool| pool_string(pool, 0))
                .unwrap_or("")
                .cmp(key)
        })
        .ok()?;
    pool_string(likely_subtags_tab[index]?, 1)
}

fn add(tag: &LanguageTag) -> Option<LanguageTag> {
    let has_language = tag.language != "und";
    let script = tag.script.as_deref().filter(|&s| s != "Zzzz");
    let region = tag.region.as_deref().filter(|&s| s != "ZZ");
    if has_language && script.is_some() && region.is_some() {
        return Some(tag.clone());
    }
    let language = tag.language.as_str();
    // Match the pinned QuickJS search: an unspecified language first tries
    // und-script-region, then script, region and language. Unknown languages
    // do not fall back to und: no match leaves the original tag intact.
    let mut found = None;
    if !has_language && let (Some(script), Some(region)) = (script, region) {
        found = lookup(&format!("und-{script}-{region}"));
    }
    if found.is_none()
        && let Some(script) = script
    {
        found = lookup(&format!("{language}-{script}"));
    }
    if found.is_none()
        && let Some(region) = region
    {
        found = lookup(&format!("{language}-{region}"));
    }
    let found = found.or_else(|| lookup(language))?;
    let mut parts = found.split('-');
    let (lang, sc, rg) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let mut result = tag.clone();
    if !has_language {
        result.language = lang.into();
    }
    result.script = Some(script.unwrap_or(sc).into());
    result.region = Some(region.unwrap_or(rg).into());
    Some(result)
}

/// Add likely subtags while retaining variants, all extensions and private use.
/// A complete language/script/region triple and an unknown language stay intact.
pub fn maximize(tag: &LanguageTag) -> LanguageTag {
    add(tag).unwrap_or_else(|| tag.clone())
}

/// Remove subtags only when maximizing round-trips to the same maximal tag.
/// Prefer language alone, then language-region, then language-script.
pub fn minimize(tag: &LanguageTag) -> LanguageTag {
    let Some(maximal) = add(tag) else {
        return tag.clone();
    };
    for (script, region) in [
        (None, None),
        (None, maximal.region.clone()),
        (maximal.script.clone(), None),
    ] {
        let mut trial = maximal.clone();
        trial.script = script;
        trial.region = region;
        if add(&trial).as_ref() == Some(&maximal) {
            return trial;
        }
    }
    maximal
}

#[cfg(test)]
mod tests {
    use super::*;

    // QuickJS test_locale rows adapted from tests/test_intl.js at
    // 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
    // Copyright (c) 2017-2021 Fabrice Bellard
    // Copyright (c) 2017-2021 Charlie Gordon
    //
    // Permission is hereby granted, free of charge, to any person obtaining a copy
    // of this software and associated documentation files (the "Software"), to deal
    // in the Software without restriction, including without limitation the rights
    // to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    // copies of the Software, and to permit persons to whom the Software is
    // furnished to do so, subject to the following conditions:
    //
    // The above copyright notice and this permission notice shall be included in
    // all copies or substantial portions of the Software.
    //
    // THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    // IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    // FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
    // THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    // LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    // OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
    // THE SOFTWARE.
    #[test]
    fn likely_subtags_add_and_remove() {
        for (input, expected) in [
            ("en", "en-Latn-US"),
            ("de", "de-Latn-DE"),
            ("ja", "ja-Jpan-JP"),
            ("zh", "zh-Hans-CN"),
            ("zh-Hant", "zh-Hant-TW"),
            ("ru", "ru-Cyrl-RU"),
            ("fr", "fr-Latn-FR"),
            ("ar", "ar-Arab-EG"),
            ("pt-BR", "pt-Latn-BR"),
            ("und", "en-Latn-US"),
            ("xx", "xx"),
            ("und-Hebr-US", "yi-Hebr-US"),
            ("und-Hebr-DE", "he-Hebr-DE"),
            ("en-Zzzz-ZZ", "en-Latn-US"),
            ("en-GB", "en-Latn-GB"),
            ("xx-Latn", "xx-Latn"),
            ("xx-Abcd-ZZ", "xx-Abcd-ZZ"),
            ("en-Cyrl-GB", "en-Cyrl-GB"),
            ("und-Zzzz-ZZ", "en-Latn-US"),
        ] {
            assert_eq!(
                maximize(&LanguageTag::parse(input).unwrap()).to_string(),
                expected,
                "{input}"
            );
        }
        for (input, expected) in [
            ("en", "en"),
            ("de", "de"),
            ("ja", "ja"),
            ("zh-Hant", "zh-TW"),
            ("en-GB", "en-GB"),
            ("en-Latn-US", "en"),
            ("en-Cyrl-US", "en-Cyrl"),
            ("xx", "xx"),
            ("xx-Latn", "xx-Latn"),
            ("und", "en"),
        ] {
            assert_eq!(
                minimize(&LanguageTag::parse(input).unwrap()).to_string(),
                expected,
                "{input}"
            );
        }
        let tag = LanguageTag::parse("de-1901-a-foobar-u-attr-ca-gregory-kn-x-private").unwrap();
        assert_eq!(
            maximize(&tag).to_string(),
            "de-Latn-DE-1901-a-foobar-u-attr-ca-gregory-kn-x-private"
        );
        assert_eq!(minimize(&maximize(&tag)), tag);
    }

    #[test]
    fn likely_subtags_ordered_pool_lookup() {
        assert_eq!(likely_subtags_tab.len(), 7788);
        let mut previous = "";
        for entry in likely_subtags_tab {
            let pool = entry.unwrap();
            let key = pool_string(pool, 0).unwrap();
            let value = pool_string(pool, 1).unwrap();
            assert!(previous < key, "unordered key {key}");
            assert_eq!(lookup(key), Some(value));
            let tag = LanguageTag::parse(value).unwrap();
            assert!(tag.script.is_some() && tag.region.is_some());
            previous = key;
        }
        for key in ["", "a", "en-XX", "xx", "zzzzzzzz"] {
            assert_eq!(lookup(key), None, "{key}");
        }
        assert_eq!(lookup("en"), Some("en-Latn-US"));
        assert_eq!(lookup("zh-Hant"), Some("zh-Hant-TW"));
    }
}
