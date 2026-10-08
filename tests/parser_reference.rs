//! Independent output captured from the JVM vendored reader, so normal tests
//! need neither Java nor the gitignored research checkout.
use light_cljs::{NodeKind, parse};

#[test]
fn literal_syntax_and_utf16_values_match_reference_snapshot() {
    let snapshot = include_str!("reference/parser_literals.tsv");
    let mut count = 0;
    for line in snapshot.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let bytes: Vec<_> = fields[0]
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let source = String::from_utf8(bytes).unwrap();
        let (category, value) = match parse(&source) {
            Err(_) => ("error", None),
            Ok(tree) => {
                assert_eq!(tree.roots.len(), 1, "{source:?}");
                match &tree.nodes[tree.roots[0].0].kind {
                    NodeKind::Nil => ("nil", None),
                    NodeKind::Boolean(_) => ("boolean", None),
                    NodeKind::Symbol => ("symbol", None),
                    NodeKind::Keyword { .. } => ("keyword", None),
                    NodeKind::Number(_) => ("number", None),
                    NodeKind::String(value) => ("string", Some(value.to_utf16(&source))),
                    NodeKind::Character(value) => {
                        ("character", Some(value.encode_utf16(&mut [0; 2]).to_vec()))
                    }
                    _ => ("other", None),
                }
            }
        };
        assert_eq!(category, fields[1], "{source:?}");
        if let Some(value) = value {
            let expected: Vec<_> = fields[2]
                .split(',')
                .map(|hex| u16::from_str_radix(hex, 16).unwrap())
                .collect();
            assert_eq!(value, expected, "{source:?}");
        }
        count += 1;
    }
    assert!(count >= 90);
}
