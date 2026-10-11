use std::{fs, path::Path};

use light_cljs::parse;

#[test]
fn pinned_reader_sources_accept_syntax_with_exact_body_byte_spans() {
    use light_cljs::{Parser, Tokenizer};
    let output = std::process::Command::new("python3")
        .args([
            "-B",
            "tests/reference/generate_reader_coverage.py",
            "--verify",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut files = 0;
    let mut bodies = 0;
    for row in include_str!("reference/reader-sources.tsv")
        .lines()
        .filter(|r| !r.starts_with('#'))
    {
        let fields: Vec<_> = row.split('\t').collect();
        let source = String::from_utf8(
            fields[3]
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|hex| u8::from_str_radix(std::str::from_utf8(hex).unwrap(), 16).unwrap())
                .collect(),
        )
        .unwrap();
        let expected: Vec<_> = fields[2]
            .split(',')
            .map(|range| {
                let (start, end) = range.split_once(':').unwrap();
                start.parse::<usize>().unwrap()..end.parse::<usize>().unwrap()
            })
            .collect();
        let tree = parse(&source).unwrap_or_else(|e| panic!("{}: {e}", fields[0]));
        let actual: Vec<_> = tree
            .roots
            .iter()
            .map(|r| tree.nodes[r.0].span.clone())
            .collect();
        assert_eq!(actual, expected, "{}", fields[0]);
        for node in &tree.nodes {
            assert!(source.get(node.span.clone()).is_some());
        }
        let mut tokenizer = Tokenizer::new(&source);
        let mut end = 0;
        for token in tokenizer.by_ref() {
            assert!(token.span.start >= end && token.span.end > token.span.start);
            assert!(source.get(token.span.clone()).is_some());
            end = token.span.end;
        }
        assert_eq!(tokenizer.next(), None);
        let mut parser = Parser::new(&source);
        for span in expected {
            let id = parser.read_form().unwrap().unwrap();
            assert_eq!(parser.nodes()[id.0].span, span);
            bodies += 1;
        }
        for _ in 0..3 {
            assert_eq!(parser.read_form(), Ok(None));
        }
        files += 1;
    }
    assert_eq!((files, bodies), (7, 137));
}

fn visit(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != ".git" {
                visit(&entry.path(), files);
            }
        } else if matches!(
            entry.path().extension().and_then(|s| s.to_str()),
            Some("clj" | "cljs" | "cljc")
        ) {
            files.push(entry.path());
        }
    }
}

#[test]
#[ignore = "requires the gitignored research/clojurescript checkout"]
fn parse_clojurescript_checkout() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("research/clojurescript");
    let mut files = Vec::new();
    visit(&root, &mut files);
    files.sort();
    let (mut bytes, mut nodes, mut roots) = (0, 0, 0);
    let mut errors = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).unwrap();
        bytes += source.len();
        match parse(&source) {
            Ok(tree) => {
                nodes += tree.nodes.len();
                roots += tree.roots.len();
                for node in tree.nodes {
                    assert!(source.get(node.span).is_some(), "{}", file.display());
                }
            }
            Err(error) => errors.push(format!("{}: {error:?}", file.display())),
        }
    }
    assert!(files.len() > 100);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    eprintln!(
        "Parsed {} files, {bytes} bytes, {nodes} nodes, {roots} top-level forms",
        files.len()
    );
}
