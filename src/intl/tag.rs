//! BCP 47 / Unicode locale identifiers. Written fresh using the pinned
//! QuickJS intl/intl.c and intl/locale.c algorithms as semantic references.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageTag {
    pub language: String,
    pub script: Option<String>,
    pub region: Option<String>,
    pub variants: Vec<String>,
    pub attributes: BTreeSet<String>,
    pub keywords: BTreeMap<String, String>,
    extensions: BTreeMap<String, String>,
    private: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidLanguageTag;

pub fn language(s: &str) -> bool {
    matches!(s.len(), 2..=3 | 5..=8) && alpha(s)
}
pub fn script(s: &str) -> bool {
    s.len() == 4 && alpha(s)
}
pub fn region(s: &str) -> bool {
    (s.len() == 2 && alpha(s)) || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
}
fn alpha(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphabetic())
}
fn alnum(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_alphanumeric())
}
fn variant(s: &str) -> bool {
    alnum(s) && (matches!(s.len(), 5..=8) || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit()))
}
pub fn unicode_type(s: &str) -> bool {
    s.split('-').all(|p| matches!(p.len(), 3..=8) && alnum(p))
}
fn title(s: &str) -> String {
    let mut s = s.to_ascii_lowercase();
    s[..1].make_ascii_uppercase();
    s
}

pub fn keyword_alias<'a>(key: &str, value: &'a str) -> &'a str {
    match (key, value) {
        ("co", "phonebook") => "phonebk",
        ("co", "dictionary") => "dict",
        ("ca", "islamicc") => "islamic-civil",
        ("ca", "ethiopic-amete-alem") => "ethioaa",
        ("ks", "primary") => "level1",
        ("ks", "tertiary") => "level3",
        ("kb" | "kc" | "kh" | "kk" | "kn", "yes") => "",
        ("ms", "imperial") => "uksystem",
        ("tz", "cnckg") => "cnsha",
        ("tz", "eire") => "iedub",
        ("tz", "est") => "papty",
        ("tz", "gmt0") => "gmt",
        ("tz", "uct" | "zulu") => "utc",
        ("rg" | "sd", "no23") => "no50",
        ("rg" | "sd", "cn11") => "cnbj",
        ("rg" | "sd", "cz10a") => "cz110",
        ("rg" | "sd", "fra" | "frg") => "frges",
        ("rg" | "sd", "lud") => "lucl",
        (_, "true") => "",
        _ => value,
    }
}

impl LanguageTag {
    pub fn parse(input: &str) -> Result<Self, InvalidLanguageTag> {
        let bad = InvalidLanguageTag;
        if input.is_empty() || input.len() >= 512 || !input.is_ascii() {
            return Err(bad);
        }
        let lower = input.to_ascii_lowercase();
        let parts: Vec<_> = lower.split('-').collect();
        if parts.len() >= 64 || parts.iter().any(|s| s.is_empty()) || !language(parts[0]) {
            return Err(bad);
        }
        let mut tag = Self {
            language: parts[0].into(),
            script: None,
            region: None,
            variants: vec![],
            attributes: BTreeSet::new(),
            keywords: BTreeMap::new(),
            extensions: BTreeMap::new(),
            private: None,
        };
        let mut i = 1;
        if i < parts.len() && script(parts[i]) {
            tag.script = Some(title(parts[i]));
            i += 1;
        }
        if i < parts.len() && region(parts[i]) {
            tag.region = Some(parts[i].to_ascii_uppercase());
            i += 1;
        }
        while i < parts.len() && parts[i].len() != 1 {
            if !variant(parts[i]) || tag.variants.iter().any(|v| v == parts[i]) {
                return Err(bad);
            }
            tag.variants.push(parts[i].into());
            i += 1;
        }
        let mut seen = BTreeSet::new();
        while i < parts.len() {
            let singleton = parts[i];
            if singleton.len() != 1 || !alnum(singleton) || !seen.insert(singleton) {
                return Err(bad);
            }
            i += 1;
            let start = i;
            if singleton == "x" {
                if i == parts.len() || parts[i..].iter().any(|s| s.len() > 8 || !alnum(s)) {
                    return Err(bad);
                }
                tag.private = Some(parts[i..].join("-"));
                break;
            }
            while i < parts.len() && parts[i].len() != 1 {
                i += 1;
            }
            if start == i {
                return Err(bad);
            }
            let ext = &parts[start..i];
            if singleton == "u" {
                let mut j = 0;
                while j < ext.len() && ext[j].len() != 2 {
                    if !unicode_type(ext[j]) {
                        return Err(bad);
                    }
                    tag.attributes.insert(ext[j].into());
                    j += 1;
                }
                while j < ext.len() {
                    let key = ext[j];
                    if key.len() != 2 || !alnum(key) || !key.as_bytes()[1].is_ascii_alphabetic() {
                        return Err(bad);
                    }
                    j += 1;
                    let begin = j;
                    while j < ext.len() && ext[j].len() != 2 {
                        j += 1;
                    }
                    let value = ext[begin..j].join("-");
                    // The pellet explicitly accepts these legacy type aliases,
                    // including phonebook (nine letters), before validating length.
                    let value = keyword_alias(key, &value);
                    if !value.is_empty() && !unicode_type(value) {
                        return Err(bad);
                    }
                    tag.keywords
                        .entry(key.into())
                        .or_insert_with(|| value.into());
                }
            } else {
                if ext.iter().any(|s| !matches!(s.len(), 2..=8) || !alnum(s)) {
                    return Err(bad);
                }
                let value = if singleton == "t" {
                    canonical_transform(ext)?
                } else {
                    ext.join("-")
                };
                tag.extensions.insert(singleton.into(), value);
            }
        }
        tag.canonicalize();
        Ok(tag)
    }

    fn canonicalize(&mut self) {
        match self.language.as_str() {
            "cmn" => self.language = "zh".into(),
            "iw" | "heb" => self.language = "he".into(),
            "ji" => self.language = "yi".into(),
            "in" => self.language = "id".into(),
            "mo" => self.language = "ro".into(),
            "aar" => self.language = "aa".into(),
            "ces" => self.language = "cs".into(),
            "sh" => {
                self.language = "sr".into();
                self.script.get_or_insert_with(|| "Latn".into());
            }
            "cnr" => {
                self.language = "sr".into();
                self.region.get_or_insert_with(|| "ME".into());
            }
            _ => {}
        }
        if let Some(r) = &mut self.region {
            *r = match r.as_str() {
                "DD" => "DE",
                "SU" | "810" if self.language == "hy" || self.script.as_deref() == Some("Armn") => {
                    "AM"
                }
                "SU" | "810" => "RU",
                "CS" => "RS",
                "NT" => "SA",
                "554" => "NZ",
                _ => r.as_str(),
            }
            .into();
        }
        if self.language == "sgn" && self.region.as_deref() == Some("GR") {
            self.language = "gss".into();
            self.region = None;
        }
        for (lang, var, replacement) in [
            ("hy", "arevela", "hy"),
            ("hy", "arevmda", "hyw"),
            ("art", "lojban", "jbo"),
            ("cel", "gaulish", "xtg"),
            ("zh", "guoyu", "zh"),
            ("zh", "hakka", "hak"),
            ("zh", "xiang", "hsn"),
        ] {
            if self.language == lang && self.variants.iter().any(|v| v == var) {
                self.language = replacement.into();
                self.variants.retain(|v| v != var);
                break;
            }
        }
        if self.variants.iter().any(|v| v == "heploc") {
            self.variants.retain(|v| v != "hepburn");
            for v in &mut self.variants {
                if v == "heploc" {
                    *v = "alalc97".into();
                }
            }
        }
        self.variants.sort();
        self.variants.dedup();
    }

    pub fn base_name(&self) -> String {
        let mut s = self.language.clone();
        for p in self
            .script
            .iter()
            .chain(self.region.iter())
            .chain(self.variants.iter())
        {
            s.push('-');
            s.push_str(p);
        }
        s
    }

    /// Reapply aliases after the language/script/region options replace components.
    pub fn apply_base_aliases(&mut self) {
        self.canonicalize();
    }

    pub fn set_script(&mut self, value: &str) {
        self.script = Some(title(value));
    }
}

fn canonical_transform(parts: &[&str]) -> Result<String, InvalidLanguageTag> {
    let key = |s: &str| {
        s.len() == 2 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1].is_ascii_digit()
    };
    let first_key = parts.iter().position(|s| key(s)).unwrap_or(parts.len());
    let mut out = if first_key > 0 {
        LanguageTag::parse(&parts[..first_key].join("-"))?
            .base_name()
            .to_ascii_lowercase()
    } else {
        String::new()
    };
    let mut fields = BTreeMap::new();
    let mut i = first_key;
    while i < parts.len() {
        if !key(parts[i]) {
            return Err(InvalidLanguageTag);
        }
        let k = parts[i];
        i += 1;
        let start = i;
        while i < parts.len() && !key(parts[i]) {
            if !unicode_type(parts[i]) {
                return Err(InvalidLanguageTag);
            }
            i += 1;
        }
        if i == start {
            return Err(InvalidLanguageTag);
        }
        fields.entry(k).or_insert_with(|| {
            parts[start..i]
                .iter()
                .map(|&v| if v == "names" { "prprname" } else { v })
                .collect::<Vec<_>>()
                .join("-")
        });
    }
    for (k, v) in fields {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(k);
        out.push('-');
        out.push_str(&v);
    }
    if out.is_empty() {
        return Err(InvalidLanguageTag);
    }
    Ok(out)
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base_name())?;
        let mut extensions = self.extensions.clone();
        if !self.attributes.is_empty() || !self.keywords.is_empty() {
            let mut parts: Vec<_> = self.attributes.iter().map(String::as_str).collect();
            for (key, value) in &self.keywords {
                parts.push(key);
                if !value.is_empty() {
                    parts.push(value);
                }
            }
            extensions.insert("u".into(), parts.join("-"));
        }
        for (key, value) in extensions {
            write!(f, "-{key}-{value}")?;
        }
        if let Some(private) = &self.private {
            write!(f, "-x-{private}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intl_language_tag_parse_and_canonicalize() {
        for (input, expected) in [
            ("EN-latn-us", "en-Latn-US"),
            ("de-u-co-phonebook", "de-u-co-phonebk"),
            ("en-u-nu-latn-ca-gregory", "en-u-ca-gregory-nu-latn"),
            ("sh", "sr-Latn"),
            ("in-ID", "id-ID"),
            ("en-554", "en-NZ"),
            ("en-u-kn-true", "en-u-kn"),
            (
                "en-u-kn-yes-ms-imperial-tz-cnckg",
                "en-u-kn-ms-uksystem-tz-cnsha",
            ),
            ("en-t-a0-names", "en-t-a0-prprname"),
            ("en-u-zzz-aaa-ca-islamicc", "en-u-aaa-zzz-ca-islamic-civil"),
            ("en-b-foo-a-bar-x-Z-A", "en-a-bar-b-foo-x-z-a"),
            ("ja-Latn-hepburn-heploc", "ja-Latn-alalc97"),
            ("en-t-DE-latn-de-h0-hybrid", "en-t-de-latn-de-h0-hybrid"),
        ] {
            assert_eq!(
                LanguageTag::parse(input).unwrap().to_string(),
                expected,
                "{input}"
            );
        }
        for invalid in [
            "",
            "en-",
            "x",
            "123",
            "abcd",
            "en_US",
            "en-abc",
            "en-u",
            "en-a-a-foo",
            "en-u-a1-foo",
            "en-t-h0",
            "en-t-de-h0",
            "en-abcde-ABCDE",
            "en-a-foo-a-bar",
            "en-x",
            "en-x-123456789",
            "en-ü",
        ] {
            assert!(LanguageTag::parse(invalid).is_err(), "{invalid}");
        }
    }
}
