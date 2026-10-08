use light_cljs::{TokenKind, Tokenizer};
use std::{fs, path::Path};

fn scan_dir(root: &Path, files: &mut usize, bytes: &mut usize, tokens: &mut usize) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != ".git" {
                scan_dir(&path, files, bytes, tokens);
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("clj" | "cljs" | "cljc")
        ) {
            let source = fs::read_to_string(&path).unwrap();
            *files += 1;
            *bytes += source.len();
            let mut end = 0;
            for token in Tokenizer::new(&source) {
                assert!(
                    source.get(token.span.clone()).is_some(),
                    "{}: {token:?}",
                    path.display()
                );
                assert!(token.span.start >= end && token.span.start < token.span.end);
                assert!(
                    !matches!(token.kind, TokenKind::Error(_)),
                    "{}: {token:?}",
                    path.display()
                );
                end = token.span.end;
                *tokens += 1;
            }
        }
    }
}

#[test]
#[ignore = "requires the gitignored research/clojurescript checkout"]
fn clojurescript_checkout_has_no_lexical_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("research/clojurescript");
    let (mut files, mut bytes, mut tokens) = (0, 0, 0);
    scan_dir(&root, &mut files, &mut bytes, &mut tokens);
    assert!(files > 100);
    eprintln!("Scanned {files} source files, {bytes} bytes, {tokens} tokens");
}
