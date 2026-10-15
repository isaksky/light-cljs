//! Just enough JSON for the tests that read it: contract rows
//! (tests/compiler/npm_packages.rs), and the records of the Node driver and
//! the `chunks.json` of chunked builds (tests/compiler/chunks.rs).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}
impl Json {
    pub fn parse(text: &str) -> Json {
        let mut chars = text.chars().peekable();
        let value = Self::value(&mut chars);
        assert!(chars.all(char::is_whitespace), "trailing JSON: {text}");
        value
    }
    fn value(chars: &mut std::iter::Peekable<std::str::Chars>) -> Json {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        match chars.next().expect("JSON value") {
            'n' => Self::literal(chars, "ull", Json::Null),
            't' => Self::literal(chars, "rue", Json::Bool(true)),
            'f' => Self::literal(chars, "alse", Json::Bool(false)),
            '"' => Json::String(Self::string(chars)),
            '[' => {
                let mut items = Vec::new();
                loop {
                    while chars.next_if(|c| c.is_whitespace()).is_some() {}
                    if chars.next_if_eq(&']').is_some() {
                        break Json::Array(items);
                    }
                    chars.next_if_eq(&',');
                    items.push(Self::value(chars));
                }
            }
            '{' => {
                let mut entries = Vec::new();
                loop {
                    while chars.next_if(|c| c.is_whitespace() || *c == ',').is_some() {}
                    if chars.next_if_eq(&'}').is_some() {
                        break Json::Object(entries);
                    }
                    assert_eq!(chars.next(), Some('"'));
                    let key = Self::string(chars);
                    while chars.next_if(|c| c.is_whitespace()).is_some() {}
                    assert_eq!(chars.next(), Some(':'));
                    entries.push((key, Self::value(chars)));
                }
            }
            first => {
                let mut number = String::from(first);
                while let Some(c) = chars.next_if(|c| c.is_ascii_digit() || "+-.eE".contains(*c)) {
                    number.push(c);
                }
                Json::Number(number.parse().unwrap())
            }
        }
    }
    fn literal(chars: &mut std::iter::Peekable<std::str::Chars>, rest: &str, value: Json) -> Json {
        assert!(rest.chars().all(|c| chars.next() == Some(c)));
        value
    }
    fn string(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut result = String::new();
        let mut units = Vec::new();
        loop {
            match chars.next().expect("unterminated JSON string") {
                '"' => break,
                '\\' => match chars.next().unwrap() {
                    'u' => {
                        let hex: String = chars.by_ref().take(4).collect();
                        units.push(u16::from_str_radix(&hex, 16).unwrap());
                        if !chars.clone().take(2).eq("\\u".chars()) {
                            result.push_str(&String::from_utf16(&units).unwrap());
                            units.clear();
                        }
                        continue;
                    }
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    'r' => result.push('\r'),
                    'b' => result.push('\u{8}'),
                    'f' => result.push('\u{c}'),
                    other => result.push(other),
                },
                c => result.push(c),
            }
        }
        result
    }
    pub fn get(&self, key: &str) -> &Json {
        let Json::Object(entries) = self else {
            panic!("expected object for {key}")
        };
        &entries
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .1
    }
    pub fn str(&self) -> &str {
        let Json::String(text) = self else {
            panic!("expected string: {self:?}")
        };
        text
    }
    pub fn items(&self) -> &[Json] {
        let Json::Array(items) = self else {
            panic!("expected array: {self:?}")
        };
        items
    }
}
